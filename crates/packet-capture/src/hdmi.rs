//! Owned FFmpeg/AVFoundation capture with native-size, minimally encoded PNGs.
//!
//! Filename timestamps are FFmpeg's input-arrival wall-clock timestamps in
//! UNIX microseconds. They are not hardware exposure or game-frame timestamps.
use crate::Result;
use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_PROGRESS_LINE: usize = 4096;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(8);
const FRAME_TIMEOUT: Duration = Duration::from_secs(3);
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub(crate) struct VideoConfig {
    pub device: String,
    pub framerate: u32,
    pub video_size: String,
    pub ffmpeg: PathBuf,
}

impl VideoConfig {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.device.trim().is_empty()
            || self.device.starts_with('-')
            || self.device.len() > 512
            || self.device.contains(':')
            || self.device.chars().any(char::is_control)
        {
            return Err(
                "HDMI device must be a video device name or index without an audio suffix".into(),
            );
        }
        if !(1..=240).contains(&self.framerate) {
            return Err("HDMI framerate must be between 1 and 240".into());
        }
        let dimensions = self.video_size.split_once('x').and_then(|(width, height)| {
            let width = width.parse::<u32>().ok()?;
            let height = height.parse::<u32>().ok()?;
            Some((width, height))
        });
        if !dimensions.is_some_and(|(width, height)| {
            (1..=16_384).contains(&width) && (1..=16_384).contains(&height)
        }) {
            return Err(
                "HDMI video size must be WIDTHxHEIGHT with dimensions from 1 to 16384".into(),
            );
        }
        if self.ffmpeg.as_os_str().is_empty() {
            return Err("FFmpeg executable path is empty".into());
        }
        Ok(())
    }
}

fn capture_command(config: &VideoConfig, screenshots: &Path, segment: u64) -> Result<Command> {
    config.validate()?;
    let directory = screenshots
        .to_str()
        .ok_or("HDMI screenshot path must be UTF-8")?
        .replace('%', "%%");
    let pattern = Path::new(&directory).join(format!("segment-{segment:06}-%019d.png"));
    let mut command = Command::new(&config.ffmpeg);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // A terminal Ctrl-C belongs to InkScope's foreground group. Keep this
        // owned recorder alive until InkScope explicitly forwards the stop so
        // an orderly shutdown cannot look like an unexpected media failure.
        command.process_group(0);
    }
    command.args([
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "warning",
        "-n",
        "-stats_period",
        "0.1",
        "-progress",
        "pipe:1",
        "-f",
        "avfoundation",
        "-framerate",
        &config.framerate.to_string(),
        "-video_size",
        &config.video_size,
        "-pixel_format",
        "uyvy422",
        "-use_wallclock_as_timestamps",
        "1",
        "-copyts",
        "-i",
        &format!("{}:none", config.device),
        "-an",
        "-sn",
        "-dn",
        "-c:v",
        "png",
        "-pix_fmt",
        "rgb24",
        "-compression_level",
        "0",
        "-pred",
        "none",
        "-fps_mode",
        "passthrough",
        "-enc_time_base",
        "1:1000000",
        "-frame_pts",
        "1",
        "-atomic_writing",
        "1",
        "-f",
        "image2",
    ]);
    command.arg(pattern);
    Ok(command)
}

fn prepare_screenshots_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err("HDMI screenshot path must be a real directory".into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o777 != 0o700 {
                    return Err(
                        "HDMI screenshot directory must have private 0700 permissions".into(),
                    );
                }
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            #[cfg(unix)]
            let mut builder = fs::DirBuilder::new();
            #[cfg(not(unix))]
            let builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder
                .create(path)
                .map_err(|error| format!("create HDMI screenshot directory: {error}"))?;
        },
        Err(error) => return Err(format!("inspect HDMI screenshot directory: {error}")),
    }
    Ok(())
}

#[derive(Default)]
struct Progress {
    frames_reported: u64,
    last_frame_at: Option<Instant>,
    last_output_timestamp_us: Option<i64>,
    finished: bool,
    eof: bool,
    error: Option<String>,
}

fn update_progress(progress: &mut Progress, line: &[u8]) -> Result<()> {
    if line.len() > MAX_PROGRESS_LINE {
        return Err("FFmpeg progress line exceeds 4096 bytes".into());
    }
    let line = std::str::from_utf8(line)
        .map_err(|_| "FFmpeg progress is not UTF-8")?
        .trim();
    let Some((name, value)) = line.split_once('=') else {
        return Ok(());
    };
    match name {
        "frame" => {
            let frames = value
                .trim()
                .parse::<u64>()
                .map_err(|_| "invalid FFmpeg frame count")?;
            if frames < progress.frames_reported {
                return Err("FFmpeg frame count moved backwards".into());
            }
            if frames > progress.frames_reported {
                progress.last_frame_at = Some(Instant::now());
            }
            progress.frames_reported = frames;
        },
        "out_time_us" if value != "N/A" => {
            let timestamp = value
                .parse::<i64>()
                .map_err(|_| "invalid FFmpeg output timestamp")?;
            progress.last_output_timestamp_us = Some(timestamp);
        },
        "progress" if value == "end" => progress.finished = true,
        _ => {},
    }
    Ok(())
}

fn progress_error(progress: &Arc<Mutex<Progress>>, message: String) {
    if let Ok(mut state) = progress.lock() {
        state.error.get_or_insert(message);
    }
}

fn progress_reader(
    mut stdout: ChildStdout,
    progress: Arc<Mutex<Progress>>,
    stopped: Arc<AtomicBool>,
) {
    let mut pending = Vec::with_capacity(512);
    let mut bytes = [0; 1024];
    while !stopped.load(Ordering::Acquire) {
        match stdout.read(&mut bytes) {
            Ok(0) => {
                if let Ok(mut state) = progress.lock() {
                    if !pending.is_empty()
                        && let Err(error) = update_progress(&mut state, &pending)
                    {
                        state.error.get_or_insert(error);
                    }
                    state.eof = true;
                }
                return;
            },
            Ok(count) => {
                for byte in &bytes[..count] {
                    if *byte == b'\n' {
                        let result = progress
                            .lock()
                            .map_err(|_| "FFmpeg progress lock poisoned".to_owned())
                            .and_then(|mut state| update_progress(&mut state, &pending));
                        pending.clear();
                        if let Err(error) = result {
                            progress_error(&progress, error);
                            return;
                        }
                    } else {
                        pending.push(*byte);
                        if pending.len() > MAX_PROGRESS_LINE {
                            progress_error(
                                &progress,
                                "FFmpeg progress line exceeds 4096 bytes".into(),
                            );
                            return;
                        }
                    }
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            },
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {},
            Err(error) => {
                progress_error(&progress, format!("read FFmpeg progress: {error}"));
                return;
            },
        }
    }
}

#[cfg(unix)]
fn nonblocking(pipe: &impl std::os::fd::AsRawFd) -> Result<()> {
    let descriptor = pipe.as_raw_fd();
    // This is the newly owned pipe, never a shared user descriptor.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(format!(
            "configure FFmpeg progress pipe: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn nonblocking<T>(pipe: &T) -> Result<()> {
    let _ = pipe;
    Err("HDMI capture currently requires macOS AVFoundation".into())
}

/// Explicit device inventory only; this never runs as part of capture startup.
pub(crate) fn list_devices(program: &Path) -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Err("HDMI device listing currently requires macOS AVFoundation".into());
    }
    let mut child = Command::new(program)
        .args([
            "-hide_banner",
            "-nostdin",
            "-f",
            "avfoundation",
            "-list_devices",
            "true",
            "-i",
            "",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("FFmpeg device listing start: {error}"))?;
    let result = (|| {
        let mut stderr = child
            .stderr
            .take()
            .ok_or("FFmpeg device listing stderr unavailable")?;
        nonblocking(&stderr)?;
        let mut output = Vec::new();
        let mut bytes = [0; 4096];
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut exit = None;
        loop {
            match stderr.read(&mut bytes) {
                Ok(0) if exit.is_some() => break,
                Ok(count) => {
                    if output.len() + count > 65_536 {
                        return Err("FFmpeg device listing exceeds 65536 bytes".into());
                    }
                    output.extend_from_slice(&bytes[..count]);
                },
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if exit.is_some() {
                        break;
                    }
                },
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(format!("read FFmpeg device listing: {error}")),
            }
            if exit.is_none() {
                exit = child
                    .try_wait()
                    .map_err(|error| format!("FFmpeg device listing wait: {error}"))?;
            }
            if Instant::now() >= deadline {
                return Err("FFmpeg device listing timed out after 10 seconds".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = String::from_utf8_lossy(&output);
        eprint!("{output}");
        let status = exit.ok_or("FFmpeg device listing exit status unavailable")?;
        if output.contains("AVFoundation video devices:")
            && (status.success() || matches!(status.code(), Some(1 | 251)))
        {
            Ok(())
        } else {
            Err(format!("FFmpeg device listing failed with {status}"))
        }
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn unix_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[derive(Default)]
struct PngInventory {
    count: u64,
    first_timestamp_us: Option<u64>,
    last_timestamp_us: Option<u64>,
}

fn png_inventory(directory: &Path, segment: u64) -> Result<PngInventory> {
    let prefix = format!("segment-{segment:06}-");
    let mut inventory = PngInventory::default();
    for entry in
        fs::read_dir(directory).map_err(|error| format!("read HDMI screenshots: {error}"))?
    {
        let entry = entry.map_err(|error| format!("read HDMI screenshot entry: {error}"))?;
        if !entry
            .file_type()
            .map_err(|error| format!("inspect HDMI screenshot: {error}"))?
            .is_file()
        {
            continue;
        }
        let name = entry.file_name();
        let Some(timestamp) = name
            .to_str()
            .and_then(|name| name.strip_prefix(&prefix))
            .and_then(|name| name.strip_suffix(".png"))
            .and_then(|value| value.parse::<u64>().ok())
        else {
            continue;
        };
        inventory.count += 1;
        inventory.first_timestamp_us = Some(
            inventory
                .first_timestamp_us
                .map_or(timestamp, |old| old.min(timestamp)),
        );
        inventory.last_timestamp_us = Some(
            inventory
                .last_timestamp_us
                .map_or(timestamp, |old| old.max(timestamp)),
        );
    }
    Ok(inventory)
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct HdmiSummary {
    pub requested_framerate: u32,
    pub requested_video_size: String,
    pub frames_reported: u64,
    pub png_files: u64,
    pub frame_count_matches_files: bool,
    pub first_filename_timestamp_us: Option<u64>,
    pub last_filename_timestamp_us: Option<u64>,
    pub started_unix_ns: u64,
    pub ended_unix_ns: u64,
    pub last_output_timestamp_us: Option<i64>,
    pub exit_code: Option<i32>,
    pub exit_success: bool,
    pub stop_requested: bool,
    pub stopped_cleanly: bool,
    pub progress_ended: bool,
    pub forced_termination: bool,
    pub progress_error: Option<String>,
    pub filename_timestamp_clock: &'static str,
    pub filename_time_base_numerator: u32,
    pub filename_time_base_denominator: u32,
}

pub(crate) struct HdmiCapture {
    child: Option<Child>,
    progress: Arc<Mutex<Progress>>,
    reader_stopped: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
    config: VideoConfig,
    screenshots: PathBuf,
    segment: u64,
    started_at: Instant,
    started_unix_ns: u64,
    summary: Option<HdmiSummary>,
}

impl HdmiCapture {
    pub(crate) fn start(config: &VideoConfig, screenshots: &Path, segment: u64) -> Result<Self> {
        if !cfg!(target_os = "macos") {
            return Err("HDMI capture currently requires macOS AVFoundation".into());
        }
        let mut command = capture_command(config, screenshots, segment)?;
        prepare_screenshots_directory(screenshots)?;
        let started_unix_ns = unix_ns();
        let started_at = Instant::now();
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("FFmpeg start: {error}"))?;
        let progress = Arc::new(Mutex::new(Progress::default()));
        let reader_stopped = Arc::new(AtomicBool::new(false));
        let setup = (|| {
            let stdout = child.stdout.take().ok_or("FFmpeg stdout unavailable")?;
            nonblocking(&stdout)?;
            let reader_progress = progress.clone();
            let reader_flag = reader_stopped.clone();
            std::thread::Builder::new()
                .name("inkscope-hdmi-progress".into())
                .spawn(move || progress_reader(stdout, reader_progress, reader_flag))
                .map_err(|error| format!("start FFmpeg progress reader: {error}"))
        })();
        let reader = match setup {
            Ok(reader) => reader,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            },
        };
        Ok(Self {
            child: Some(child),
            progress,
            reader_stopped,
            reader: Some(reader),
            config: config.clone(),
            screenshots: screenshots.to_path_buf(),
            segment,
            started_at,
            started_unix_ns,
            summary: None,
        })
    }

    pub(crate) fn poll(&mut self) -> Result<()> {
        let child = self
            .child
            .as_mut()
            .ok_or("HDMI capture is already stopped")?;
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("FFmpeg poll: {error}"))?
        {
            return Err(format!("FFmpeg exited unexpectedly with {status}"));
        }
        let progress = self
            .progress
            .lock()
            .map_err(|_| "FFmpeg progress lock poisoned")?;
        if let Some(error) = &progress.error {
            return Err(error.clone());
        }
        if progress.eof || progress.finished {
            return Err("FFmpeg progress stream ended before capture was stopped".into());
        }
        if let Some(last_frame) = progress.last_frame_at {
            if last_frame.elapsed() > FRAME_TIMEOUT {
                return Err("HDMI capture stalled: no newly saved frames for 3 seconds".into());
            }
        } else if self.started_at.elapsed() > STARTUP_TIMEOUT {
            return Err("HDMI capture produced no frames within 8 seconds".into());
        }
        Ok(())
    }

    pub(crate) fn stop(&mut self) -> Result<HdmiSummary> {
        if let Some(summary) = &self.summary {
            return Ok(summary.clone());
        }
        let stop_requested = self
            .child
            .as_mut()
            .ok_or("HDMI capture child unavailable")?
            .try_wait()
            .map_err(|error| format!("FFmpeg wait: {error}"))?
            .is_none();
        let (status, forced_termination) = if let Some(child) = self.child.as_mut() {
            stop_owned_child(child)?
        } else {
            return Err("HDMI capture child unavailable".into());
        };
        // The child has closed its progress pipe; let the reader consume the
        // final report, but do not block shutdown on another inherited writer.
        let deadline = Instant::now() + Duration::from_millis(200);
        while self
            .reader
            .as_ref()
            .is_some_and(|reader| !reader.is_finished())
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.reader_stopped.store(true, Ordering::Release);
        if let Some(reader) = self.reader.take() {
            reader
                .join()
                .map_err(|_| "FFmpeg progress reader panicked")?;
        }
        self.child.take();
        let inventory = png_inventory(&self.screenshots, self.segment)?;
        let progress = self
            .progress
            .lock()
            .map_err(|_| "FFmpeg progress lock poisoned")?;
        let summary = HdmiSummary {
            requested_framerate: self.config.framerate,
            requested_video_size: self.config.video_size.clone(),
            frames_reported: progress.frames_reported,
            png_files: inventory.count,
            frame_count_matches_files: inventory.count == progress.frames_reported,
            first_filename_timestamp_us: inventory.first_timestamp_us,
            last_filename_timestamp_us: inventory.last_timestamp_us,
            started_unix_ns: self.started_unix_ns,
            ended_unix_ns: unix_ns(),
            last_output_timestamp_us: progress.last_output_timestamp_us,
            exit_code: status.code(),
            exit_success: status.success(),
            stop_requested,
            stopped_cleanly: stop_requested
                && !forced_termination
                && progress.finished
                && progress.error.is_none()
                && (status.success() || status.code() == Some(255)),
            progress_ended: progress.finished,
            forced_termination,
            progress_error: progress.error.clone(),
            filename_timestamp_clock: "ffmpeg_input_arrival_unix_microseconds_not_exposure_time",
            filename_time_base_numerator: 1,
            filename_time_base_denominator: 1_000_000,
        };
        self.summary = Some(summary.clone());
        Ok(summary)
    }
}

fn stop_owned_child(child: &mut Child) -> Result<(ExitStatus, bool)> {
    if let Some(status) = child
        .try_wait()
        .map_err(|error| format!("FFmpeg wait: {error}"))?
    {
        return Ok((status, false));
    }
    #[cfg(unix)]
    unsafe {
        // capture_command created this exact group; no user process group is
        // discovered or reused here. Include any recorder-owned descendants.
        libc::kill(-(child.id() as i32), libc::SIGINT);
    }
    let deadline = Instant::now() + STOP_TIMEOUT;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("FFmpeg wait: {error}"))?
        {
            return Ok((status, false));
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    #[cfg(unix)]
    let killed = if unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) } == 0 {
        Ok(())
    } else {
        child.kill()
    };
    #[cfg(not(unix))]
    let killed = child.kill();
    if let Err(error) = killed {
        // Exit can race the final signal. Reap it rather than reporting a
        // failure solely because the just-exited owned process was absent.
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("FFmpeg wait: {error}"))?
        {
            return Ok((status, false));
        }
        return Err(format!("stop FFmpeg: {error}"));
    }
    child
        .wait()
        .map(|status| (status, true))
        .map_err(|error| format!("reap FFmpeg: {error}"))
}

impl Drop for HdmiCapture {
    fn drop(&mut self) {
        if self.stop().is_err() {
            if let Some(child) = self.child.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            self.reader_stopped.store(true, Ordering::Release);
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
        }
    }
}

#[cfg(test)]
#[path = "../test_support/unit/hdmi.rs"]
mod tests;
