use clap::{Parser, Subcommand, ValueEnum};
use s3_packet_capture::Result;
use s3_packet_capture::crypto::{self, GameKey, PiaKey};
use s3_packet_capture::decode::Decoder;
use s3_packet_capture::pcap::{Reader, UdpPacket};
use s3_packet_capture::preview::WavePreviewer;
use s3_packet_capture::render::print_row;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "s3-packet-capture",
    about = "Splatoon 3 11.3.0 LAN Play packet capture and authenticated decoder"
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
    #[arg(long, global = true, value_enum, default_value_t = DisplayMode::Normal, help = "Display mode: normal hides Empty, BroadcastReliableProtocol, and untyped UnreliableProtocol")]
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
        help = "Hide repetitive RTT and unconfirmed Protocol 5 rows"
    )]
    hide_cycle: bool,
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
        #[arg(long)]
        output: PathBuf,
    },
    /// Decode an existing classic PCAP without network I/O.
    Inspect { pcap: PathBuf },
    /// Capture through tcpdump, save exact PCAP bytes, and decode live.
    Capture {
        #[arg(long)]
        interface: String,
        #[arg(long)]
        output: PathBuf,
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
    if source_count != 1 {
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
        unreachable!("source count was checked")
    };
    let pia: Option<(Ipv4Addr, PiaKey)> = match (&cli.pia_key_file, cli.peer) {
        (Some(path), Some(peer)) => Some((peer, crypto::read_pia_key(path)?)),
        (None, None) => None,
        _ => return Err("--pia-key-file requires --peer and vice versa".into()),
    };
    Ok(Decoder::new(game, pia, cli.broadcast_ip))
}

fn process<R: Read>(
    input: R,
    cli: &Cli,
    decoder: &mut Decoder,
    target: Option<&Target>,
) -> Result<()> {
    let mut reader = Reader::new(input)?;
    let mut packets = 0u64;
    let mut authenticated = 0u64;
    let mut missing_keys = 0u64;
    let mut authentication_failures = 0u64;
    let mut malformed = 0u64;
    let mut previewer = WavePreviewer::default();
    while let Some(record) = reader.next_record()? {
        match record {
            Ok(packet) => {
                packets += 1;
                for mut row in decoder.process(&packet, cli.details) {
                    row.direction = target.map(|value| direction(&packet, value).into());
                    row.wave_preview = previewer.observe(&row);
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
                    if cli.mode == DisplayMode::Normal
                        && (matches!(row.protocol, Some(0 | 11))
                            || row.protocol == Some(5) && row.kind == "UnreliableProtocol")
                    {
                        continue;
                    }
                    if cli.hide_cycle
                        && (row.protocol == Some(3)
                            || row.protocol == Some(5)
                                && row.layout.as_deref() != Some("confirmed_fields"))
                    {
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

struct ChildGuard(Option<Child>);
struct SignalPid(Arc<Mutex<Option<i32>>>);
impl Drop for SignalPid {
    fn drop(&mut self) {
        if let Ok(mut pid) = self.0.lock() {
            *pid = None;
        }
    }
}
impl ChildGuard {
    fn try_wait(&mut self, signal_pid: &SignalPid) -> Result<Option<std::process::ExitStatus>> {
        let mut pid = signal_pid
            .0
            .lock()
            .map_err(|_| "capture signal lock poisoned")?;
        let active = pid.take();
        let result = self
            .0
            .as_mut()
            .ok_or("capture child missing")?
            .try_wait()
            .map_err(|e| format!("tcpdump wait: {e}"));
        if matches!(result, Ok(None)) {
            *pid = active;
        }
        result
    }

    fn wait(&mut self, signal_pid: &SignalPid) -> Result<std::process::ExitStatus> {
        loop {
            if let Some(status) = self.try_wait(signal_pid)? {
                return Ok(status);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0
            && child.try_wait().ok().flatten().is_none()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn validate_interface(interface: &str) -> Result<()> {
    if interface.is_empty()
        || interface.starts_with('-')
        || interface.len() > 64
        || !interface
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err("interface name invalid".into());
    }
    let output = Command::new("/sbin/ifconfig")
        .arg(interface)
        .output()
        .map_err(|e| format!("ifconfig: {e}"))?;
    if !output.status.success() {
        return Err("capture interface unavailable".into());
    }
    Ok(())
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
) -> Result<()> {
    validate_interface(interface)?;
    let file = private_output(output)?;
    let child = Command::new("/usr/sbin/tcpdump")
        .args([
            "-i",
            interface,
            "-s",
            "0",
            "-U",
            "-w",
            "-",
            "udp and (port 35000 or port 49152)",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn();
    let child = match child {
        Ok(child) => child,
        Err(error) => {
            drop(file);
            let _ = std::fs::remove_file(output);
            return Err(format!("tcpdump start: {error}"));
        },
    };
    let mut guard = ChildGuard(Some(child));
    let pid = Arc::new(Mutex::new(Some(guard.0.as_ref().unwrap().id() as i32)));
    let _pid_guard = SignalPid(pid.clone());
    let stopped = Arc::new(AtomicBool::new(false));
    let flag = stopped.clone();
    let signal_pid = pid.clone();
    let handler = ctrlc::set_handler(move || {
        let repeated = flag.swap(true, Ordering::SeqCst);
        #[cfg(unix)]
        if let Ok(child_pid) = signal_pid.lock()
            && let Some(child_pid) = *child_pid
        {
            unsafe {
                libc::kill(
                    child_pid,
                    if repeated {
                        libc::SIGKILL
                    } else {
                        libc::SIGINT
                    },
                );
            }
        }
    });
    if let Err(error) = handler {
        drop(file);
        let _ = std::fs::remove_file(output);
        return Err(format!("capture signal handler: {error}"));
    }
    let stdout = guard
        .0
        .as_mut()
        .unwrap()
        .stdout
        .take()
        .ok_or("tcpdump stdout unavailable")?;
    let result = process(
        Tee {
            source: stdout,
            sink: file,
        },
        cli,
        decoder,
        target,
    );
    if let Err(error) = result {
        // tcpdump may close its pipe just before its exit status becomes
        // visible. Give an early startup failure (such as BPF permission
        // denial) a brief chance to report its actual status.
        let mut child_status = guard.try_wait(&_pid_guard).ok().flatten();
        for _ in 0..3 {
            if child_status.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
            child_status = guard.try_wait(&_pid_guard).ok().flatten();
        }
        if std::fs::metadata(output).is_ok_and(|metadata| metadata.len() == 0) {
            let _ = std::fs::remove_file(output);
        }
        return Err(match child_status {
            Some(status) if !status.success() => {
                format!("tcpdump exited with {status}; see its error above ({error})")
            },
            Some(status) => format!("tcpdump exited with {status}; decode failed: {error}"),
            None => format!("capture decode failed: {error}; saved PCAP may be partial"),
        });
    }
    let status = guard.wait(&_pid_guard)?;
    if !status.success() && !stopped.load(Ordering::SeqCst) {
        return Err(format!(
            "tcpdump exited with {status}; saved PCAP may be partial"
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
    match &cli.command {
        Mode::ExportTable { .. } => unreachable!(),
        Mode::Inspect { pcap } => process(
            File::open(pcap).map_err(|e| format!("PCAP unavailable: {e}"))?,
            &cli,
            &mut decoder,
            target.as_ref(),
        ),
        Mode::Capture { interface, output } => {
            capture(&cli, &mut decoder, target.as_ref(), interface, output)
        },
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("s3-packet-capture: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn arrows_are_relative_to_the_explicit_target() {
        let target_mac = parse_mac("02:00:00:00:00:01").unwrap();
        assert_eq!(target_mac, [0x02, 0, 0, 0, 0, 1]);
        assert!(parse_mac("02:00:00:00:00").is_err());
        assert!(parse_mac("02:00:00:00:00:gg").is_err());
        let mut packet = UdpPacket {
            record: 1,
            timestamp_ns: 0,
            source: Ipv4Addr::new(192, 0, 2, 1),
            destination: Ipv4Addr::new(192, 0, 2, 2),
            source_port: 49152,
            destination_port: 49152,
            source_mac: target_mac,
            destination_mac: [1; 6],
            payload: vec![],
        };
        assert_eq!(direction(&packet, &Target::Mac(target_mac)), "->");
        assert_eq!(direction(&packet, &Target::Ip(packet.destination)), "<-");
        packet.source_mac = [1; 6];
        packet.destination_mac = target_mac;
        assert_eq!(direction(&packet, &Target::Mac(target_mac)), "<-");
        packet.destination_mac = [2; 6];
        assert_eq!(direction(&packet, &Target::Mac(target_mac)), "--");
    }

    struct OneByte(Cursor<Vec<u8>>);
    impl Read for OneByte {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let count = buf.len().min(1);
            self.0.read(&mut buf[..count])
        }
    }

    #[test]
    fn tee_saves_exact_stream_despite_partial_reads() {
        let payload = b"pcap stream in small chunks".to_vec();
        let mut tee = Tee {
            source: OneByte(Cursor::new(payload.clone())),
            sink: Vec::new(),
        };
        let mut decoded = vec![];
        tee.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, payload);
        assert_eq!(tee.sink, payload);
    }

    #[test]
    fn capture_output_is_private_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("capture.pcap");
        private_output(&path).unwrap();
        assert!(private_output(&path).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
