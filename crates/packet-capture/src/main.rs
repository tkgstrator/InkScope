mod capture_backend;
mod control;
mod hdmi;
mod in_game;
mod in_game_capture;
mod visualizer;

use clap::{Parser, Subcommand, ValueEnum};
use inkscope::Result;
use inkscope::crypto::{self, GameKey, PiaKey};
use inkscope::decode::{Decoder, Row};
use inkscope::pcap::{Reader, UdpPacket};
use inkscope::pia_cache::ManagedPiaCache;
use inkscope::preview::WavePreviewer;
use inkscope::render::print_row;
use inkscope::spawn::SpawnTracker;
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
    #[arg(long, global = true, value_enum, default_value_t = DisplayMode::Normal, help = "Display mode: normal hides routine rows; debug shows all; salmon shows known Salmon Run rows and filters flat PCAP output (without HDMI)")]
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
    Salmon,
}

#[derive(Subcommand)]
enum Mode {
    /// List macOS AVFoundation video devices without starting packet capture.
    HdmiDevices {
        #[arg(long, default_value = "ffmpeg")]
        ffmpeg: PathBuf,
    },
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
        #[arg(
            long,
            required_unless_present = "hdmi_device",
            conflicts_with = "hdmi_device",
            help = "New classic PCAP output path (without HDMI recording)"
        )]
        output: Option<PathBuf>,
        #[arg(
            long,
            help = "macOS AVFoundation HDMI video device name or index; save in-game UUID sessions"
        )]
        hdmi_device: Option<String>,
        #[arg(
            long,
            requires = "hdmi_device",
            help = "In-game session root (default: captures/in_game)"
        )]
        in_game_dir: Option<PathBuf>,
        #[arg(
            long,
            default_value_t = 60,
            help = "Requested HDMI input frame rate; no output frame duplication"
        )]
        hdmi_framerate: u32,
        #[arg(
            long,
            default_value = "1920x1080",
            help = "HDMI input mode, not an output resize"
        )]
        hdmi_video_size: String,
        #[arg(
            long,
            default_value = "ffmpeg",
            help = "FFmpeg executable used for HDMI capture"
        )]
        ffmpeg: PathBuf,
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

fn is_managed_pia_cache(path: &Path) -> bool {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".config/inkscope/pia_key.bin") == path)
        .unwrap_or(false)
}

fn missing_managed_pia_cache(cli: &Cli, path: &Path) -> bool {
    matches!(cli.command, Mode::Capture { .. })
        && is_managed_pia_cache(path)
        && matches!(
            std::fs::symlink_metadata(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        )
}

fn track_loaded_cache(path: &Path, peer: Ipv4Addr, decoder: &Decoder) -> Result<ManagedPiaCache> {
    if let Some(key) = decoder.manual_pia_key(peer) {
        ManagedPiaCache::loaded(path.to_path_buf(), &key.0)
    } else {
        ManagedPiaCache::absent(path.to_path_buf())
    }
}

struct PiaCacheMonitor {
    cache: ManagedPiaCache,
    peer: Ipv4Addr,
    failures: u8,
    notified: bool,
}

impl PiaCacheMonitor {
    fn start(cli: &Cli, decoder: &Decoder) -> Result<Option<Self>> {
        if !matches!(cli.command, Mode::Capture { .. }) {
            return Ok(None);
        }
        let (Some(path), Some(peer)) = (&cli.pia_key_file, cli.peer) else {
            return Ok(None);
        };
        if !is_managed_pia_cache(path) {
            return Ok(None);
        }
        let cache = track_loaded_cache(path, peer, decoder)?;
        Ok(Some(Self {
            cache,
            peer,
            failures: 0,
            notified: false,
        }))
    }

    fn use_authenticated_key(&mut self, decoder: &mut Decoder, key: [u8; 16]) -> Result<()> {
        match self.cache.save_verified(&key) {
            Ok(true) => eprintln!("PiaKey cache updated for peer {}", self.peer),
            Ok(false) => {},
            Err(error) => eprintln!("PiaKey cache update failed: {error}"),
        }
        decoder.replace_manual_pia(self.peer, PiaKey(key));
        self.failures = 0;
        self.notified = false;
        Ok(())
    }

    fn observe(
        &mut self,
        manual_result: Option<bool>,
        packet: &UdpPacket,
        rows: &[Row],
        decoder: &mut Decoder,
    ) -> Result<()> {
        if packet.source == self.peer
            && packet.source_port == 35000
            && rows
                .iter()
                .any(|row| row.kind == "BrowseReply" && row.status == "proof_verified")
            && let Some(key) = decoder.verified_pia_key(self.peer).map(|key| key.0)
        {
            return self.use_authenticated_key(decoder, key);
        }
        match manual_result {
            Some(true) => self.failures = 0,
            Some(false) => {
                if let Some(key) = decoder.authenticated_candidate_key(packet, self.peer) {
                    return self.use_authenticated_key(decoder, key);
                }
                self.failures = self.failures.saturating_add(1);
                if self.failures >= 3 && !self.notified {
                    let removal = self.cache.remove_if_unchanged();
                    decoder.discard_manual_pia();
                    self.notified = true;
                    match removal {
                        Ok(true) => eprintln!(
                            "保存した PiaKey で現在の部屋を復号できません。pia_key.bin を削除しました。部屋に入り直してください。"
                        ),
                        Ok(false) => eprintln!(
                            "保存した PiaKey で現在の部屋を復号できません。鍵ファイルが別途変更されたため削除せず、次回起動時に読み直してください。"
                        ),
                        Err(error) => eprintln!(
                            "保存した PiaKey で現在の部屋を復号できません。鍵ファイルを削除できませんでした: {error}"
                        ),
                    }
                }
            },
            None => {},
        }
        Ok(())
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
        (Some(path), Some(peer)) => {
            if missing_managed_pia_cache(cli, path) {
                None
            } else {
                Some((peer, crypto::read_pia_key(path)?))
            }
        },
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
            || protocol == Some(3)
            || protocol == Some(11) && kind != "PrivateSetting"
            || protocol == Some(5) && kind == "UnreliableProtocol")
    {
        return false;
    }
    !hide_cycle
        || !((protocol == Some(3) && !has_capture_delay)
            || protocol == Some(5) && layout != Some("confirmed_fields"))
}

fn salmon_row(row: &Row) -> bool {
    row.status == "authenticated"
        && (row.kind.starts_with("Coop")
            || row.kind.starts_with("PlayerNetEvent::Coop")
            || matches!(
                row.kind.as_str(),
                "OnlineCoopSetting"
                    | "PrivateSetting"
                    | "PlayerNetEvent::Revival"
                    | "PlayerNetEvent::TroubleDie"
            ))
}

#[derive(Default)]
struct CaptureOutputs<'a> {
    salmon: Option<File>,
    game: Option<&'a mut in_game_capture::Recorder>,
}

fn process<R: Read>(
    input: R,
    cli: &Cli,
    decoder: &mut Decoder,
    target: Option<&Target>,
    publisher: Option<&mut visualizer::Publisher>,
    control: Option<&control::Control>,
    salmon_output: Option<File>,
) -> Result<()> {
    process_inner(
        input,
        cli,
        decoder,
        target,
        publisher,
        control,
        CaptureOutputs {
            salmon: salmon_output,
            game: None,
        },
    )
}

fn process_inner<R: Read>(
    input: R,
    cli: &Cli,
    decoder: &mut Decoder,
    target: Option<&Target>,
    publisher: Option<&mut visualizer::Publisher>,
    control: Option<&control::Control>,
    mut outputs: CaptureOutputs<'_>,
) -> Result<()> {
    let mut reader = if outputs.salmon.is_some() || outputs.game.is_some() {
        Reader::new_with_raw_records(input)?
    } else {
        Reader::new(input)?
    };
    if let Some(game) = outputs.game.as_mut() {
        game.set_header(reader.global_header());
    }
    if let Some(output) = outputs.salmon.as_mut() {
        output
            .write_all(reader.global_header())
            .map_err(|e| format!("save PCAP header: {e}"))?;
    }
    let mut packets = 0u64;
    let mut authenticated = 0u64;
    let mut missing_keys = 0u64;
    let mut authentication_failures = 0u64;
    let mut malformed = 0u64;
    let mut previewer = WavePreviewer::default();
    let mut spawn_tracker = SpawnTracker::default();
    let mut pia_cache = match PiaCacheMonitor::start(cli, decoder) {
        Ok(cache) => cache,
        Err(error) => {
            eprintln!("PiaKey cache monitoring disabled: {error}");
            None
        },
    };
    let mut publisher = publisher;
    while let Some(record) = reader.next_record()? {
        match record {
            Ok(packet) => {
                packets += 1;
                if let Some(control) = control {
                    control.apply_pending(decoder);
                }
                let manual_result = pia_cache
                    .as_ref()
                    .and_then(|_| decoder.manual_pia_authentication(&packet));
                let mut rows = decoder.process(&packet, cli.details || outputs.game.is_some());
                if let Some(cache) = pia_cache.as_mut() {
                    cache.observe(manual_result, &packet, &rows, decoder)?;
                }
                if let Some(game) = outputs.game.as_mut() {
                    game.observe(&packet, &rows, reader.last_record_bytes())?;
                    if !cli.details {
                        for row in &mut rows {
                            row.authenticated_payload_hex = None;
                            row.fields.retain(|field| {
                                !matches!(
                                    field.name.as_str(),
                                    "HeaderDestinationId" | "HeaderSourceId"
                                )
                            });
                        }
                    }
                }
                if rows.iter().any(salmon_row)
                    && let Some(output) = outputs.salmon.as_mut()
                {
                    output
                        .write_all(reader.last_record_bytes())
                        .map_err(|e| format!("save Salmon Run PCAP record: {e}"))?;
                }
                for row in &mut rows {
                    row.direction = target.map(|value| direction(&packet, value).into());
                    row.wave_preview = previewer.observe(row);
                    spawn_tracker.observe(row);
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
                    if cli.mode == DisplayMode::Salmon && !salmon_row(&row) {
                        continue;
                    }
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
    // Reconcile stream EOF with the capture child's status before finalizing.
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
    output: Option<&Path>,
    publisher: Option<&mut visualizer::Publisher>,
    control: Option<&control::Control>,
) -> Result<()> {
    let (mut command, tool_name) = capture_backend::host_command(interface)?;
    let file = output.map(private_output).transpose()?;
    let child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            drop(file);
            if let Some(output) = output {
                let _ = std::fs::remove_file(output);
            }
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
        if let Some(output) = output {
            let _ = std::fs::remove_file(output);
        }
        return Err(format!("capture signal handler: {error}"));
    }
    if let Some(control) = control {
        control.set_running(true);
    }
    let mut game_recorder = None;
    let result = if let Mode::Capture {
        hdmi_device: Some(device),
        in_game_dir,
        hdmi_framerate,
        hdmi_video_size,
        ffmpeg,
        ..
    } = &cli.command
    {
        let config = in_game_capture::Config {
            root: in_game_dir
                .clone()
                .unwrap_or_else(|| PathBuf::from("captures/in_game")),
            video: hdmi::VideoConfig {
                device: device.clone(),
                framerate: *hdmi_framerate,
                video_size: hdmi_video_size.clone(),
                ffmpeg: ffmpeg.clone(),
            },
        };
        game_recorder = Some(in_game_capture::Recorder::start(config, guard.0.clone())?);
        process_inner(
            stdout,
            cli,
            decoder,
            target,
            publisher,
            control,
            CaptureOutputs {
                salmon: None,
                game: game_recorder.as_mut(),
            },
        )
    } else if cli.mode == DisplayMode::Salmon {
        process(stdout, cli, decoder, target, publisher, control, file)
    } else {
        process(
            Tee {
                source: stdout,
                sink: file.ok_or("flat capture requires --output")?,
            },
            cli,
            decoder,
            target,
            publisher,
            control,
            None,
        )
    };
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
        let output_len = output.and_then(|output| std::fs::metadata(output).ok().map(|m| m.len()));
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
        let removed = remove_invalid && output.is_some_and(|p| std::fs::remove_file(p).is_ok());
        if stopped.load(Ordering::SeqCst) && error.contains("truncated PCAP record") {
            if output.is_some() && output_len.is_none_or(|length| length < 24) {
                return Err("capture stopped before a complete PCAP header".into());
            }
            eprintln!("capture stopped; saved PCAP may be partial: {error}");
            if let Some(game) = game_recorder.as_mut() {
                game.finish("capture_stopped", true)?;
            }
            if let Some(output) = output {
                eprintln!("PCAP saved: {}", output.display());
            }
            return Ok(());
        }
        let file_note = if removed {
            "no PCAP saved".to_owned()
        } else if let Some(output) = output {
            format!("PCAP saved at {}; it may be partial", output.display())
        } else {
            "in-game session files retained; recording may be incomplete".into()
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
    if let Some(game) = game_recorder.as_mut() {
        let stopped = stopped.load(Ordering::SeqCst);
        game.finish(
            if stopped {
                "capture_stopped"
            } else {
                "capture_backend_ended"
            },
            !stopped,
        )?;
    }
    if let Some(output) = output {
        eprintln!("PCAP saved: {}", output.display());
    }
    Ok(())
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if let Mode::HdmiDevices { ffmpeg } = &cli.command {
        return hdmi::list_devices(ffmpeg);
    }
    if matches!(
        &cli.command,
        Mode::Capture {
            hdmi_device: Some(_),
            ..
        }
    ) && !cfg!(target_os = "macos")
    {
        return Err("HDMI capture currently requires macOS AVFoundation".into());
    }
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
        Mode::ExportTable { .. } | Mode::HdmiDevices { .. } => unreachable!(),
        Mode::Inspect { pcap } => process(
            File::open(pcap).map_err(|e| format!("PCAP unavailable: {e}"))?,
            &cli,
            &mut decoder,
            target.as_ref(),
            publisher.as_mut(),
            None,
            None,
        ),
        Mode::Capture {
            interface, output, ..
        } => capture(
            &cli,
            &mut decoder,
            target.as_ref(),
            interface,
            output.as_deref(),
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
