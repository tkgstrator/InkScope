mod capture_backend;
mod control;
mod visualizer;

use clap::{Parser, Subcommand, ValueEnum};
use inkscope::Result;
use inkscope::crypto::{self, GameKey, PiaKey};
use inkscope::decode::Decoder;
use inkscope::pcap::{Reader, UdpPacket};
use inkscope::preview::WavePreviewer;
use inkscope::render::print_row;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "inkscope",
    about = "Splatoon 3 LAN Play packet capture and decoder"
)]
struct Cli {
    #[command(subcommand)]
    command: Mode,
    #[arg(
        long,
        global = true,
        help = "Private 272-byte v1 key-generation material file"
    )]
    random_table: Option<PathBuf>,
    #[arg(
        long,
        visible_alias = "game_key",
        global = true,
        help = "GameKey as 32 hex digits (visible in process arguments and shell history)"
    )]
    game_key: Option<String>,
    #[arg(long, global = true, help = "Private 16-byte raw GameKey file")]
    game_key_file: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Load GameKey from the pinned Splatoon 3 11.3.0 ELF"
    )]
    elf: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Private 16-byte raw PiaKey file for a mid-session capture"
    )]
    pia_key_file: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "IPv4 peer for the explicitly provided PiaKey"
    )]
    peer: Option<Ipv4Addr>,
    #[arg(
        long,
        global = true,
        help = "Broadcast IPv4 used in BrowseRequest and BrowseReply nonces"
    )]
    broadcast_ip: Option<Ipv4Addr>,
    #[arg(
        long,
        global = true,
        conflicts_with = "target_ip",
        help = "Target machine MAC address for direction arrows"
    )]
    target_mac: Option<String>,
    #[arg(
        long,
        global = true,
        conflicts_with = "target_mac",
        help = "Target machine IPv4 for direction arrows"
    )]
    target_ip: Option<Ipv4Addr>,
    #[arg(long, global = true, help = "Print one JSON object per row")]
    jsonl: bool,
    #[arg(long, global = true, value_enum, default_value_t = DisplayMode::Normal, help = "Display mode: normal hides Empty, generic BroadcastReliableProtocol, and untyped UnreliableProtocol; recognized PrivateSetting stays visible")]
    mode: DisplayMode,
    #[arg(
        long,
        global = true,
        help = "Include field bit ranges and authenticated chunk bytes"
    )]
    details: bool,
    #[arg(
        long,
        global = true,
        help = "Hide RTT requests and unmatched responses plus unconfirmed Protocol 5 rows; matched delay responses stay visible"
    )]
    hide_cycle: bool,
    #[arg(
        long,
        global = true,
        help = "Send capture events to a loopback InkScope Visualizer WebSocket; token comes from INKSCOPE_VISUALIZER_TOKEN"
    )]
    visualizer_url: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum DisplayMode {
    Normal,
    Debug,
}

#[derive(Subcommand)]
enum Mode {
    /// Extract private key-generation material from the exact pinned ELF.
    ExportTable {
        #[arg(long, help = "New output path for the private 272-byte random table")]
        output: PathBuf,
    },
    /// Decode an existing classic PCAP without network I/O.
    Inspect {
        #[arg(help = "Existing classic PCAP path")]
        pcap: PathBuf,
    },
    /// Capture from a host interface, save classic PCAP bytes, and decode live.
    Capture {
        #[arg(long, help = "Interface name (or dumpcap -D number on Windows)")]
        interface: String,
        #[arg(long, help = "New classic PCAP output path")]
        output: PathBuf,
        #[arg(
            long,
            help = "Optional 127.0.0.1 control HTTP port; requires INKSCOPE_CONTROL_TOKEN"
        )]
        control_port: Option<u16>,
    },
}

enum Target {
    Mac([u8; 6]),
    Ip(Ipv4Addr),
}

fn parse_mac(value: &str) -> Result<[u8; 6]> {
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() != 6 || parts.iter().any(|part| part.len() != 2) {
        return Err("target MAC must have six two-digit hexadecimal octets".into());
    }
    let mut mac = [0u8; 6];
    for (byte, part) in mac.iter_mut().zip(parts) {
        *byte = u8::from_str_radix(part, 16)
            .map_err(|_| "target MAC must have six two-digit hexadecimal octets")?;
    }
    Ok(mac)
}

fn target(cli: &Cli) -> Result<Option<Target>> {
    if let Some(mac) = &cli.target_mac {
        Ok(Some(Target::Mac(parse_mac(mac)?)))
    } else {
        Ok(cli.target_ip.map(Target::Ip))
    }
}

fn startup_key_source(cli: &Cli) -> Option<&'static str> {
    if cli.random_table.is_some() {
        Some("random_table")
    } else if cli.game_key.is_some() {
        Some("cli")
    } else if cli.game_key_file.is_some() {
        Some("file")
    } else if cli.elf.is_some() {
        Some("elf")
    } else {
        None
    }
}

fn direction(packet: &UdpPacket, target: &Target) -> &'static str {
    let (source, destination) = match target {
        Target::Mac(mac) => (packet.source_mac == *mac, packet.destination_mac == *mac),
        Target::Ip(ip) => (packet.source == *ip, packet.destination == *ip),
    };
    match (source, destination) {
        (true, false) => "->",
        (false, true) => "<-",
        (true, true) => "<->",
        (false, false) => "--",
    }
}

fn decoder(cli: &Cli) -> Result<Decoder> {
    let source_count = [
        cli.random_table.is_some(),
        cli.game_key.is_some(),
        cli.game_key_file.is_some(),
        cli.elf.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    let keyless_control_capture = source_count == 0
        && matches!(
            cli.command,
            Mode::Capture {
                control_port: Some(_),
                ..
            }
        );
    if source_count != 1 && !keyless_control_capture {
        return Err(
            "specify exactly one of --random-table, --game-key, --game-key-file, or --elf".into(),
        );
    }
    let game: Option<GameKey> = if let Some(path) = &cli.random_table {
        let material = crypto::read_random_table(path)?;
        Some(crypto::generate_game_key(&material))
    } else if let Some(value) = &cli.game_key {
        Some(crypto::read_game_key_hex(value)?)
    } else if let Some(path) = &cli.game_key_file {
        Some(crypto::read_game_key(path)?)
    } else if let Some(path) = &cli.elf {
        Some(crypto::derive_game_key(path)?)
    } else {
        None
    };
    let pia: Option<(Ipv4Addr, PiaKey)> = match (&cli.pia_key_file, cli.peer) {
        (Some(path), Some(peer)) => Some((peer, crypto::read_pia_key(path)?)),
        (None, None) => None,
        _ => return Err("--pia-key-file requires --peer and vice versa".into()),
    };
    Ok(Decoder::new(game, pia, cli.broadcast_ip))
}

fn visible_in_stream(
    mode: DisplayMode,
    hide_cycle: bool,
    protocol: Option<u8>,
    kind: &str,
    layout: Option<&str>,
    has_capture_delay: bool,
) -> bool {
    if mode == DisplayMode::Normal
        && (protocol == Some(0)
            || protocol == Some(11) && kind != "PrivateSetting"
            || protocol == Some(5) && kind == "UnreliableProtocol")
    {
        return false;
    }
    !hide_cycle
        || !((protocol == Some(3) && !has_capture_delay)
            || protocol == Some(5) && layout != Some("confirmed_fields"))
}

fn process<R: Read>(
    input: R,
    cli: &Cli,
    decoder: &mut Decoder,
    target: Option<&Target>,
    publisher: Option<&mut visualizer::Publisher>,
    control: Option<&control::Control>,
) -> Result<()> {
    let mut reader = Reader::new(input)?;
    let mut packets = 0u64;
    let mut authenticated = 0u64;
    let mut missing_keys = 0u64;
    let mut authentication_failures = 0u64;
    let mut malformed = 0u64;
    let mut previewer = WavePreviewer::default();
    let mut publisher = publisher;
    while let Some(record) = reader.next_record()? {
        match record {
            Ok(packet) => {
                packets += 1;
                if let Some(control) = control {
                    control.apply_pending(decoder);
                }
                let mut rows = decoder.process(&packet, cli.details);
                for row in &mut rows {
                    row.direction = target.map(|value| direction(&packet, value).into());
                    row.wave_preview = previewer.observe(row);
                    match row.status.as_str() {
                        "authenticated" | "proof_verified" => authenticated += 1,
                        "key_missing" | "key_or_broadcast_missing" | "game_key_missing" => {
                            missing_keys += 1
                        },
                        "authentication_failed" | "proof_failed" => authentication_failures += 1,
                        "endpoint_mismatch" | "unsupported_layout" => malformed += 1,
                        s if s.contains("malformed") || s.contains("invalid") => malformed += 1,
                        _ => {},
                    }
                }
                if let Some(publisher) = publisher.as_deref_mut() {
                    publisher.emit(&packet, &rows);
                }
                for row in rows {
                    if !visible_in_stream(
                        cli.mode,
                        cli.hide_cycle,
                        row.protocol,
                        &row.kind,
                        row.layout.as_deref(),
                        row.fields
                            .iter()
                            .any(|field| field.name == "CaptureResponseDelayMs"),
                    ) {
                        continue;
                    }
                    print_row(&row, cli.jsonl, cli.details)?;
                }
            },
            Err(error) => {
                malformed += 1;
                eprintln!("{error}");
            },
        }
    }
    eprintln!(
        "UDP packets={packets}, verified rows={authenticated}, missing keys={missing_keys}, authentication failures={authentication_failures}, malformed/unsupported frames={malformed}"
    );
    Ok(())
}

struct Tee<R: Read, W: Write> {
    source: R,
    sink: W,
}
impl<R: Read, W: Write> Read for Tee<R, W> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let count = self.source.read(buf)?;
        if count > 0 {
            self.sink.write_all(&buf[..count])?;
        }
        Ok(count)
    }
}

struct ChildGuard(Arc<Mutex<Child>>);
impl ChildGuard {
    fn try_wait(&self) -> Result<Option<std::process::ExitStatus>> {
        self.0
            .lock()
            .map_err(|_| "capture child lock poisoned")?
            .try_wait()
            .map_err(|e| format!("capture wait: {e}"))
    }

    fn wait(&self) -> Result<std::process::ExitStatus> {
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Ok(mut child) = self.0.lock()
            && child.try_wait().ok().flatten().is_none()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn stop_child(child: &mut Child, repeated: bool) {
    if !matches!(child.try_wait(), Ok(None)) {
        return;
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(
            child.id() as i32,
            if repeated {
                libc::SIGKILL
            } else {
                libc::SIGINT
            },
        );
    }
    #[cfg(windows)]
    {
        if repeated {
            let _ = child.kill();
        }
    }
}

fn private_output(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|e| format!("capture output must be a new writable file: {e}"))
}

fn capture(
    cli: &Cli,
    decoder: &mut Decoder,
    target: Option<&Target>,
    interface: &str,
    output: &Path,
    publisher: Option<&mut visualizer::Publisher>,
    control: Option<&control::Control>,
) -> Result<()> {
    let (mut command, tool_name) = capture_backend::host_command(interface)?;
    let file = private_output(output)?;
    let child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            drop(file);
            let _ = std::fs::remove_file(output);
            return Err(format!("{tool_name} start: {error}"));
        },
    };
    let stdout = child
        .stdout
        .take()
        .ok_or("capture tool stdout unavailable")?;
    let guard = ChildGuard(Arc::new(Mutex::new(child)));
    let stopped = Arc::new(AtomicBool::new(false));
    let flag = stopped.clone();
    let signal_child = guard.0.clone();
    let handler = ctrlc::set_handler(move || {
        let repeated = flag.swap(true, Ordering::SeqCst);
        if let Ok(mut child) = signal_child.lock() {
            stop_child(&mut child, repeated);
        }
        #[cfg(windows)]
        if !repeated {
            let timed_child = signal_child.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(3));
                if let Ok(mut child) = timed_child.lock()
                    && matches!(child.try_wait(), Ok(None))
                {
                    let _ = child.kill();
                }
            });
        }
    });
    if let Err(error) = handler {
        drop(file);
        let _ = std::fs::remove_file(output);
        return Err(format!("capture signal handler: {error}"));
    }
    if let Some(control) = control {
        control.set_running(true);
    }
    let result = process(
        Tee {
            source: stdout,
            sink: file,
        },
        cli,
        decoder,
        target,
        publisher,
        control,
    );
    if let Err(error) = result {
        // The capture tool may close its pipe just before its exit status becomes
        // visible. Give an early startup failure (such as BPF permission
        // denial) a brief chance to report its actual status.
        let mut child_status = guard.try_wait().ok().flatten();
        for _ in 0..3 {
            if child_status.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
            child_status = guard.try_wait().ok().flatten();
        }
        if error.contains("truncated PCAP record") && !stopped.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(100));
        }
        let output_len = std::fs::metadata(output)
            .map(|metadata| metadata.len())
            .ok();
        let header_error = [
            "empty PCAP",
            "truncated PCAP record",
            "pcapng is unsupported",
            "invalid classic PCAP magic",
            "unsupported PCAP version",
            "unsupported PCAP link type",
            "PCAP snaplen outside supported bounds",
        ]
        .iter()
        .any(|message| error.starts_with(message));
        let remove_invalid =
            output_len.is_some_and(|length| length < 24 || length == 24 && header_error);
        let removed = remove_invalid && std::fs::remove_file(output).is_ok();
        if stopped.load(Ordering::SeqCst) && error.contains("truncated PCAP record") {
            if output_len.is_none_or(|length| length < 24) {
                return Err("capture stopped before a complete PCAP header".into());
            }
            eprintln!("capture stopped; saved PCAP may be partial: {error}");
            eprintln!("PCAP saved: {}", output.display());
            return Ok(());
        }
        let file_note = if removed {
            "no PCAP saved".to_owned()
        } else {
            format!("PCAP saved at {}; it may be partial", output.display())
        };
        return Err(match child_status {
            Some(status) => {
                format!("{tool_name} exited with {status}; decode failed: {error}; {file_note}")
            },
            None => format!("capture decode failed: {error}; {file_note}"),
        });
    }
    let status = guard.wait()?;
    if !status.success() && !stopped.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
    }
    if !status.success() && !stopped.load(Ordering::SeqCst) {
        return Err(format!(
            "{tool_name} exited with {status}; saved PCAP may be partial"
        ));
    }
    if !status.success() {
        eprintln!("capture stopped; saved PCAP may be partial");
    }
    eprintln!("PCAP saved: {}", output.display());
    Ok(())
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if let Mode::ExportTable { output } = &cli.command {
        if cli.random_table.is_some() || cli.game_key.is_some() || cli.game_key_file.is_some() {
            return Err("export-table accepts only --elf as a key source".into());
        }
        let elf = cli.elf.as_ref().ok_or("export-table requires --elf")?;
        crypto::export_random_table(elf, output)?;
        eprintln!("private random table saved: {}", output.display());
        return Ok(());
    }
    let mut decoder = decoder(&cli)?;
    let target = target(&cli)?;
    let mut publisher = visualizer::Publisher::start(
        cli.visualizer_url.as_deref(),
        matches!(cli.command, Mode::Inspect { .. }),
    )?;
    let control_port = match &cli.command {
        Mode::Capture { control_port, .. } => *control_port,
        _ => None,
    };
    let control = control_port
        .map(|port| {
            control::Control::start(
                port,
                startup_key_source(&cli),
                publisher
                    .as_ref()
                    .map(visualizer::Publisher::connection_state),
            )
        })
        .transpose()?;
    let result = match &cli.command {
        Mode::ExportTable { .. } => unreachable!(),
        Mode::Inspect { pcap } => process(
            File::open(pcap).map_err(|e| format!("PCAP unavailable: {e}"))?,
            &cli,
            &mut decoder,
            target.as_ref(),
            publisher.as_mut(),
            None,
        ),
        Mode::Capture {
            interface, output, ..
        } => capture(
            &cli,
            &mut decoder,
            target.as_ref(),
            interface,
            output,
            publisher.as_mut(),
            control.as_ref(),
        ),
    };
    if let Some(control) = control {
        control.set_running(false);
        control.finish();
    }
    if let Some(publisher) = publisher {
        publisher.finish();
    }
    result
}

fn main() {
    if let Err(error) = run() {
        eprintln!("inkscope: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
#[path = "../test_support/unit/main.rs"]
mod tests;
