//! Packet-controlled, wave-scoped HDMI sessions. No keys are written to datasets.
use crate::hdmi::{HdmiCapture, VideoConfig};
use crate::in_game::{Gate, VideoAction};
use inkscope::Result;
use inkscope::decode::Row;
use inkscope::pcap::UdpPacket;
use serde_json::{Value, json};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const VIDEO_IDLE_TIMEOUT: Duration = Duration::from_secs(2);
const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) struct Config {
    pub root: PathBuf,
    pub video: VideoConfig,
}

fn unix_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(u64::MAX as u128) as u64
}

fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|e| format!("create private {}: {e}", path.display()))
}

struct Session {
    path: PathBuf,
    pcap: File,
    decoded: File,
    manifest: Value,
    video: Option<HdmiCapture>,
    segment: u64,
    packet_count: u64,
    last_packet: Instant,
    closed: bool,
}

impl Session {
    fn create(config: &Config, header: &[u8; 24], trigger_ns: u64) -> Result<Self> {
        std::fs::create_dir_all(&config.root).map_err(|e| format!("in-game root: {e}"))?;
        let path = config.root.join(uuid::Uuid::new_v4().to_string());
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&path)
            .map_err(|e| format!("new in-game UUID directory: {e}"))?;
        builder
            .create(path.join("screeshots"))
            .map_err(|e| format!("screenshots directory: {e}"))?;
        let mut pcap = private_file(&path.join("capture.pcap"))?;
        pcap.write_all(header)
            .map_err(|e| format!("in-game PCAP header: {e}"))?;
        let decoded = private_file(&path.join("packets.jsonl"))?;
        let session = Self {
            manifest: json!({
                "schema_version": 1, "session_id": path.file_name().unwrap().to_string_lossy(),
                "status": "recording", "start_trigger": "authenticated_OnlineCoopSetting",
                "trigger_packet_timestamp_ns": trigger_ns, "created_unix_ns": unix_ns(),
                "requested_video": {"device": config.video.device, "input_mode": config.video.video_size,
                    "framerate": config.video.framerate, "resize": false, "png_compression_level": 0},
                "boundary": "confirmed CoopSequenceNetState until CoopSeqRoundFinishEvent; next Sequence resumes",
                "video_idle_timeout_seconds": VIDEO_IDLE_TIMEOUT.as_secs(),
                "session_idle_timeout_seconds": SESSION_IDLE_TIMEOUT.as_secs(),
                "frame_filename_clock": "FFmpeg input receive wallclock UNIX microseconds, not hardware exposure or GameFrame",
                "packet_clock": "original PCAP timestamps, preserved without rewriting",
                "clock_alignment": "host wallclock only; HDMI and network latency not calibrated",
                "decoder_version": env!("CARGO_PKG_VERSION"),
                "key_material_included": false,
                "pcap_redecode": "requires separately retained room PiaKey; packets.jsonl keeps decoded fields",
                "packet_count": 0, "segments": [], "game_result": "Unknown"
            }),
            path,
            pcap,
            decoded,
            video: None,
            segment: 0,
            packet_count: 0,
            last_packet: Instant::now(),
            closed: false,
        };
        session.save_manifest()?;
        eprintln!("In-game session: {}", session.path.display());
        Ok(session)
    }

    fn save_manifest(&self) -> Result<()> {
        let temp = self.path.join("manifest.json.tmp");
        // Only this session owns this temporary path; a previous failed write is retained.
        let mut file = private_file(&temp)?;
        serde_json::to_writer_pretty(&mut file, &self.manifest).map_err(|e| e.to_string())?;
        file.write_all(b"\n").map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        std::fs::rename(temp, self.path.join("manifest.json"))
            .map_err(|e| format!("save manifest: {e}"))
    }

    fn write_packet(&mut self, packet: &UdpPacket, raw: &[u8], rows: &[Row]) -> Result<()> {
        self.pcap
            .write_all(raw)
            .map_err(|e| format!("in-game PCAP: {e}"))?;
        let mut decoded_rows = Vec::new();
        for row in rows.iter().filter(|r| r.status == "authenticated") {
            let mut value = serde_json::to_value(row).map_err(|e| e.to_string())?;
            value
                .as_object_mut()
                .unwrap()
                .remove("authenticated_payload_hex");
            decoded_rows.push(value);
        }
        self.packet_count += 1;
        serde_json::to_writer(
            &mut self.decoded,
            &json!({
                "saved_record": self.packet_count, "capture_record": packet.record,
                "packet_timestamp_ns": packet.timestamp_ns, "decoded_available_unix_ns": unix_ns(),
                "rows": decoded_rows
            }),
        )
        .map_err(|e| format!("in-game decoded rows: {e}"))?;
        self.decoded.write_all(b"\n").map_err(|e| e.to_string())?;
        // RTT/ACK traffic can continue in the lobby after a lost finish event.
        if rows.iter().any(|row| {
            row.status == "authenticated"
                && row.layout.as_deref() == Some("confirmed_fields")
                && (row.kind == "CoopSequenceNetState"
                    || row.kind == "CoopSeqPlayerNetState"
                    || row.kind.starts_with("CoopEnemy")
                    || row.kind.starts_with("CoopItem"))
        }) {
            self.last_packet = Instant::now();
        }
        Ok(())
    }

    fn stop_video(&mut self, reason: &str, packet_ns: Option<u64>) -> Result<()> {
        if let Some(mut video) = self.video.take() {
            let ended = unix_ns();
            let result = video.stop();
            let entry = match &result {
                Ok(summary) => json!({"segment": self.segment, "end_reason": reason,
                    "end_observed_unix_ns": ended, "trigger_packet_timestamp_ns": packet_ns,
                    "video": summary}),
                Err(error) => json!({"segment": self.segment, "end_reason": reason,
                    "end_observed_unix_ns": ended, "error": error}),
            };
            self.manifest["segments"]
                .as_array_mut()
                .unwrap()
                .push(entry);
            self.save_manifest()?;
            let summary = result?;
            if !summary.stopped_cleanly {
                return Err("HDMI segment did not stop cleanly; see manifest".into());
            }
            if summary.frames_reported == 0 {
                return Err("HDMI segment produced no frames; see manifest".into());
            }
            if !summary.frame_count_matches_files || summary.progress_error.is_some() {
                return Err("HDMI saved PNG count/progress mismatch; see manifest".into());
            }
        }
        Ok(())
    }

    fn finish(&mut self, reason: &str, incomplete: bool) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        let video_result = self.stop_video(reason, None);
        self.manifest["status"] = json!("closed");
        self.manifest["end_reason"] = json!(reason);
        self.manifest["ended_unix_ns"] = json!(unix_ns());
        self.manifest["incomplete"] = json!(incomplete || video_result.is_err());
        self.manifest["packet_count"] = json!(self.packet_count);
        self.pcap.flush().map_err(|e| e.to_string())?;
        self.decoded.flush().map_err(|e| e.to_string())?;
        self.save_manifest()?;
        self.closed = true;
        video_result
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.finish("recording_aborted", true);
    }
}

struct State {
    session: Option<Session>,
    error: Option<String>,
}

impl State {
    fn close(&mut self, reason: &str, incomplete: bool) -> Result<()> {
        if let Some(mut session) = self.session.take() {
            session.finish(reason, incomplete)?;
        }
        Ok(())
    }

    fn tick_at(&mut self, now: Instant) -> Result<bool> {
        if let Some(session) = self.session.as_mut() {
            if let Some(video) = session.video.as_mut() {
                video.poll()?;
            }
            let elapsed = now.saturating_duration_since(session.last_packet);
            let reason = if session.video.is_some() && elapsed >= VIDEO_IDLE_TIMEOUT {
                Some("gameplay_packet_timeout")
            } else if elapsed >= SESSION_IDLE_TIMEOUT {
                Some("session_idle_timeout")
            } else {
                None
            };
            if let Some(reason) = reason {
                self.close(reason, true)?;
                return Ok(true);
            }
        }
        Ok(false)
    }
}

pub(crate) struct Recorder {
    config: Config,
    header: [u8; 24],
    gate: Gate,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    reset_gate: Arc<AtomicBool>,
    monitor: Option<JoinHandle<()>>,
}

impl Recorder {
    pub(crate) fn start(config: Config, child: Arc<Mutex<Child>>) -> Result<Self> {
        config.video.validate()?;
        let state = Arc::new(Mutex::new(State {
            session: None,
            error: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let reset_gate = Arc::new(AtomicBool::new(false));
        let (monitor_state, monitor_stop, reset) =
            (state.clone(), stop.clone(), reset_gate.clone());
        let monitor = std::thread::Builder::new()
            .name("inkscope-hdmi-monitor".into())
            .spawn(move || {
                while !monitor_stop.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(100));
                    let Ok(mut state) = monitor_state.lock() else {
                        break;
                    };
                    match state.tick_at(Instant::now()) {
                        Ok(true) => {
                            reset.store(true, Ordering::SeqCst);
                        },
                        Ok(false) => {},
                        Err(error) => {
                            eprintln!("HDMI recording failed: {error}");
                            let _ = state.close("hdmi_error", true);
                            state.error = Some(error);
                            // Wake the blocked PCAP reader by stopping only our capture child.
                            if let Ok(mut child) = child.lock() {
                                crate::stop_child(&mut child, false);
                            }
                            break;
                        },
                    }
                }
            })
            .map_err(|e| format!("HDMI monitor: {e}"))?;
        Ok(Self {
            config,
            header: [0; 24],
            gate: Gate::default(),
            state,
            stop,
            reset_gate,
            monitor: Some(monitor),
        })
    }

    pub(crate) fn set_header(&mut self, header: &[u8; 24]) {
        self.header = *header;
    }

    pub(crate) fn observe(&mut self, packet: &UdpPacket, rows: &[Row], raw: &[u8]) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "in-game state lock poisoned")?;
        if self.reset_gate.swap(false, Ordering::SeqCst) {
            self.gate.reset();
        }
        let decision = self.gate.observe(packet, rows);
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        if let Some(reason) = decision.close_session {
            state.close(reason, false)?;
        }
        if decision.start_session {
            state.session = Some(Session::create(
                &self.config,
                &self.header,
                packet.timestamp_ns,
            )?);
        }
        if let Some(session) = state.session.as_mut() {
            if decision.save_packet {
                session.write_packet(packet, raw, rows)?;
            }
            match decision.video {
                VideoAction::Start => {
                    session.segment += 1;
                    session.video = Some(HdmiCapture::start(
                        &self.config.video,
                        &session.path.join("screeshots"),
                        session.segment,
                    )?);
                },
                VideoAction::Stop => {
                    session.stop_video("round_finish", Some(packet.timestamp_ns))?
                },
                VideoAction::None => {},
            }
        }
        Ok(())
    }

    pub(crate) fn finish(&mut self, reason: &str, incomplete: bool) -> Result<()> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(monitor) = self.monitor.take() {
            let _ = monitor.join();
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| "in-game state lock poisoned")?;
        state.close(reason, incomplete)?;
        if let Some(error) = state.error.take() {
            return Err(error);
        }
        Ok(())
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(monitor) = self.monitor.take() {
            let _ = monitor.join();
        }
        if let Ok(mut state) = self.state.lock() {
            let _ = state.close("capture_aborted", true);
        }
    }
}

#[cfg(test)]
#[path = "../test_support/unit/in_game_capture.rs"]
mod tests;
