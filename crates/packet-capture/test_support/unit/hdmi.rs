use super::*;
use std::time::{Duration, Instant};

fn config() -> VideoConfig {
    VideoConfig {
        device: "HDMI capture device".into(),
        framerate: 60,
        video_size: "1920x1080".into(),
        ffmpeg: PathBuf::from("ffmpeg"),
    }
}

#[test]
fn video_configuration_rejects_invalid_device_and_capture_modes() {
    assert!(config().validate().is_ok());
    for device in ["", " ", "-i", "0:none", "HDMI\nother", "x\0y"] {
        let mut value = config();
        value.device = device.into();
        assert!(value.validate().is_err(), "accepted {device:?}");
    }
    for framerate in [0, 241, u32::MAX] {
        let mut value = config();
        value.framerate = framerate;
        assert!(value.validate().is_err());
    }
    for size in [
        "",
        "0x1080",
        "1920x0",
        "1920",
        "1920:1080",
        "1920x1080;bad",
        "999999x1",
    ] {
        let mut value = config();
        value.video_size = size.into();
        assert!(value.validate().is_err(), "accepted {size:?}");
    }
}

#[test]
fn ffmpeg_command_preserves_resolution_and_names_arrival_time_in_microseconds() {
    let command = capture_command(&config(), Path::new("private/screeshots"), 7).unwrap();
    let args: Vec<_> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    for pair in [
        ["-f", "avfoundation"],
        ["-framerate", "60"],
        ["-video_size", "1920x1080"],
        ["-use_wallclock_as_timestamps", "1"],
        ["-i", "HDMI capture device:none"],
        ["-c:v", "png"],
        ["-compression_level", "0"],
        ["-pred", "none"],
        ["-fps_mode", "passthrough"],
        ["-enc_time_base", "1:1000000"],
        ["-frame_pts", "1"],
        ["-atomic_writing", "1"],
        ["-progress", "pipe:1"],
    ] {
        assert!(
            args.windows(2).any(|window| window == pair),
            "missing {pair:?}"
        );
    }
    assert!(args.iter().any(|arg| arg == "-copyts"));
    assert!(
        !args
            .iter()
            .any(|arg| matches!(arg.as_str(), "-r" | "-vf" | "-s"))
    );
    assert_eq!(
        args.last().unwrap(),
        "private/screeshots/segment-000007-%019d.png"
    );
}

#[test]
fn progress_parser_reports_frames_and_rejects_unbounded_lines() {
    let mut progress = Progress::default();
    update_progress(&mut progress, b"frame=12").unwrap();
    update_progress(&mut progress, b"out_time_us=1793212345678900").unwrap();
    update_progress(&mut progress, b"progress=end").unwrap();
    assert_eq!(progress.frames_reported, 12);
    assert_eq!(
        progress.last_output_timestamp_us,
        Some(1_793_212_345_678_900)
    );
    assert!(progress.finished);
    assert!(update_progress(&mut progress, b"frame=invalid").is_err());
    assert!(update_progress(&mut progress, &vec![b'x'; MAX_PROGRESS_LINE + 1]).is_err());
}

#[test]
fn png_inventory_counts_only_complete_files_for_the_owned_segment() {
    let root = tempfile::tempdir().unwrap();
    for name in [
        "segment-000007-0001793212345678900.png",
        "segment-000007-0001793212345695567.png",
        "segment-000007-0001793212345712234.png.tmp",
        "segment-000008-0001793212345712234.png",
    ] {
        std::fs::write(root.path().join(name), b"filename inventory fixture").unwrap();
    }
    let inventory = png_inventory(root.path(), 7).unwrap();
    assert_eq!(inventory.count, 2);
    assert_eq!(inventory.first_timestamp_us, Some(1_793_212_345_678_900));
    assert_eq!(inventory.last_timestamp_us, Some(1_793_212_345_695_567));
}

#[cfg(target_os = "macos")]
fn fake_ffmpeg(script: &str) -> (tempfile::TempDir, VideoConfig) {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let program = directory.path().join("ffmpeg");
    std::fs::write(&program, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut value = config();
    value.ffmpeg = program;
    (directory, value)
}

#[cfg(target_os = "macos")]
fn wait_for_progress(capture: &mut HdmiCapture) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        capture.poll().unwrap();
        if capture.progress.lock().unwrap().frames_reported > 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("fake ffmpeg did not emit progress");
}

#[cfg(target_os = "macos")]
fn private_images() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

#[cfg(target_os = "macos")]
#[test]
fn capture_reports_progress_and_stops_its_child_idempotently() {
    let (_program, value) = fake_ffmpeg(
        "trap 'printf \"progress=end\\n\"; exit 0' INT TERM\nprintf 'frame=2\\nout_time_us=1793212345678900\\nprogress=continue\\n'\nwhile :; do sleep 0.05; done",
    );
    let images = private_images();
    let mut capture = HdmiCapture::start(&value, images.path(), 1).unwrap();
    wait_for_progress(&mut capture);
    let summary = capture.stop().unwrap();
    assert_eq!(summary.frames_reported, 2);
    assert_eq!(summary.requested_framerate, 60);
    assert!(summary.exit_success);
    assert!(summary.stop_requested);
    assert!(summary.stopped_cleanly);
    assert!(summary.progress_ended);
    assert!(!summary.forced_termination);
    assert!(summary.ended_unix_ns >= summary.started_unix_ns);
    assert_eq!(capture.stop().unwrap().frames_reported, 2);
}

#[cfg(target_os = "macos")]
#[test]
fn missing_executable_and_early_exit_are_reported() {
    let images = private_images();
    let mut value = config();
    value.ffmpeg = images.path().join("missing-ffmpeg");
    assert!(HdmiCapture::start(&value, images.path(), 0).is_err());
    let (_program, value) = fake_ffmpeg("exit 23");
    let mut capture = HdmiCapture::start(&value, images.path(), 0).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Err(error) = capture.poll() {
            assert!(error.contains("23"), "{error}");
            break;
        }
        assert!(Instant::now() < deadline, "early exit not reported");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!capture.stop().unwrap().exit_success);
}

#[cfg(target_os = "macos")]
#[test]
fn requested_ffmpeg_interrupt_255_is_a_clean_stop_with_a_final_report() {
    let (_program, value) = fake_ffmpeg(
        "trap 'printf \"progress=end\\n\"; exit 255' INT TERM\nprintf 'frame=1\\nprogress=continue\\n'\nwhile :; do sleep 0.05; done",
    );
    let images = private_images();
    let mut capture = HdmiCapture::start(&value, images.path(), 0).unwrap();
    wait_for_progress(&mut capture);
    let summary = capture.stop().unwrap();
    assert_eq!(summary.exit_code, Some(255));
    assert!(!summary.exit_success);
    assert!(summary.stop_requested);
    assert!(summary.stopped_cleanly);
}

#[cfg(target_os = "macos")]
#[test]
fn stalled_frames_are_reported_even_with_an_alive_child() {
    let (_program, value) = fake_ffmpeg(
        "trap 'exit 0' INT TERM\nprintf 'frame=1\\nprogress=continue\\n'\nwhile :; do sleep 0.05; done",
    );
    let images = private_images();
    let mut capture = HdmiCapture::start(&value, images.path(), 0).unwrap();
    wait_for_progress(&mut capture);
    capture.progress.lock().unwrap().last_frame_at = Some(Instant::now() - Duration::from_secs(30));
    assert!(capture.poll().unwrap_err().contains("stalled"));
    capture.stop().unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn drop_reaps_only_the_owned_child() {
    let (_program, value) = fake_ffmpeg(
        "trap 'exit 0' INT TERM\nprintf 'frame=1\\nprogress=continue\\n'\nwhile :; do sleep 0.05; done",
    );
    let images = private_images();
    let mut capture = HdmiCapture::start(&value, images.path(), 0).unwrap();
    wait_for_progress(&mut capture);
    let pid = capture.child.as_ref().unwrap().id();
    drop(capture);
    assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

#[cfg(target_os = "macos")]
#[test]
fn foreground_group_interrupt_does_not_signal_hdmi_child() {
    use std::os::unix::process::CommandExt;

    const WRAPPER_DIRECTORY: &str = "INKSCOPE_HDMI_TEST_SIGNAL_DIRECTORY";
    const WRAPPER_FFMPEG: &str = "INKSCOPE_HDMI_TEST_SIGNAL_FFMPEG";
    if let Some(directory) = std::env::var_os(WRAPPER_DIRECTORY) {
        // This branch runs in a disposable test subprocess, never the cargo
        // runner. Its handler models InkScope's foreground Ctrl-C handler.
        let directory = PathBuf::from(directory);
        let interrupted = Arc::new(AtomicBool::new(false));
        let signal = interrupted.clone();
        ctrlc::set_handler(move || signal.store(true, Ordering::SeqCst)).unwrap();
        let mut value = config();
        value.ffmpeg = PathBuf::from(std::env::var_os(WRAPPER_FFMPEG).unwrap());
        let mut capture = HdmiCapture::start(&value, &directory.join("screeshots"), 0).unwrap();
        wait_for_progress(&mut capture);
        std::fs::write(
            directory.join("ready"),
            capture.child.as_ref().unwrap().id().to_string(),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        while !interrupted.load(Ordering::SeqCst) {
            assert!(
                Instant::now() < deadline,
                "disposable wrapper did not receive SIGINT"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        // The terminal-equivalent signal must reach the wrapper but leave the
        // owned FFmpeg alive until the explicit stop path forwards a signal.
        for _ in 0..20 {
            capture
                .poll()
                .expect("foreground group signal reached HDMI child");
            std::thread::sleep(Duration::from_millis(5));
        }
        let summary = capture.stop().unwrap();
        assert!(summary.stop_requested && summary.stopped_cleanly);
        std::fs::write(
            directory.join("complete"),
            b"owned child stopped explicitly",
        )
        .unwrap();
        return;
    }

    struct WrapperGuard {
        child: Child,
        media_pid: Option<u32>,
    }
    impl Drop for WrapperGuard {
        fn drop(&mut self) {
            // Every possible target below was created by this test. Never send
            // a process-group signal to the harness or a pre-existing group.
            if let Some(pid) = self.media_pid {
                let group = unsafe { libc::getpgid(pid as i32) };
                if group == pid as i32 {
                    unsafe {
                        libc::kill(-group, libc::SIGKILL);
                    }
                } else if group == self.child.id() as i32 {
                    unsafe {
                        libc::kill(pid as i32, libc::SIGKILL);
                    }
                }
            }
            if matches!(self.child.try_wait(), Ok(None)) {
                let pid = self.child.id() as i32;
                if unsafe { libc::getpgid(pid) } == pid {
                    unsafe {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                } else {
                    let _ = self.child.kill();
                }
            }
            let _ = self.child.wait();
        }
    }

    let (_program, value) = fake_ffmpeg(
        "trap 'printf \"progress=end\\n\"; exit 255' INT TERM\nprintf 'frame=1\\nprogress=continue\\n'\nfor iteration in $(seq 1 200); do sleep 0.05; done",
    );
    let directory = private_images();
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "hdmi::tests::foreground_group_interrupt_does_not_signal_hdmi_child",
            "--nocapture",
        ])
        .env(WRAPPER_DIRECTORY, directory.path())
        .env(WRAPPER_FFMPEG, &value.ffmpeg)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut wrapper = WrapperGuard {
        child,
        media_pid: None,
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let ready = directory.path().join("ready");
    while !ready.exists() {
        assert!(
            wrapper.child.try_wait().unwrap().is_none(),
            "wrapper exited before readiness"
        );
        assert!(Instant::now() < deadline, "wrapper readiness timeout");
        std::thread::sleep(Duration::from_millis(5));
    }
    let media_pid = std::fs::read_to_string(ready)
        .unwrap()
        .parse::<u32>()
        .unwrap();
    wrapper.media_pid = Some(media_pid);
    let wrapper_pid = wrapper.child.id() as i32;
    assert_eq!(unsafe { libc::getpgid(wrapper_pid) }, wrapper_pid);
    assert_ne!(wrapper_pid, unsafe { libc::getpgrp() });
    assert_eq!(
        unsafe { libc::getpgid(media_pid as i32) },
        media_pid as i32,
        "FFmpeg must own a separate process group before any group signal is sent"
    );
    assert_ne!(media_pid as i32, wrapper_pid);
    assert_eq!(unsafe { libc::kill(-wrapper_pid, libc::SIGINT) }, 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = wrapper.child.try_wait().unwrap() {
            assert!(status.success(), "wrapper failed: {status}");
            break;
        }
        assert!(Instant::now() < deadline, "wrapper completion timeout");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(directory.path().join("complete").is_file());
    wrapper.media_pid = None; // Wrapper confirmed that it reaped this child.
}

#[cfg(unix)]
#[test]
fn screenshots_directory_must_be_private() {
    use std::os::unix::fs::PermissionsExt;
    let parent = tempfile::tempdir().unwrap();
    let images = parent.path().join("screeshots");
    prepare_screenshots_directory(&images).unwrap();
    assert_eq!(
        std::fs::metadata(&images).unwrap().permissions().mode() & 0o777,
        0o700
    );
    std::fs::set_permissions(&images, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(prepare_screenshots_directory(&images).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn device_listing_accepts_ffmpeg_query_exit_one_only_with_a_device_header() {
    for code in [1, 251] {
        let (_program, value) = fake_ffmpeg(&format!(
            "printf 'AVFoundation video devices:\\n[0] HDMI capture device\\n' >&2\nexit {code}"
        ));
        list_devices(&value.ffmpeg).unwrap();
    }
    let (_program, value) =
        fake_ffmpeg("printf 'Unknown input format: avfoundation\\n' >&2\nexit 1");
    assert!(list_devices(&value.ffmpeg).is_err());
}
