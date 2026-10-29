use super::*;
use inkscope::decode::Row;
use std::net::Ipv4Addr;

fn config(root: &Path) -> Config {
    Config {
        root: root.to_owned(),
        video: VideoConfig {
            device: "0".into(),
            framerate: 60,
            video_size: "1920x1080".into(),
            ffmpeg: "ffmpeg".into(),
        },
    }
}

#[test]
fn session_preserves_raw_pcap_and_usable_decoded_rows_without_raw_payloads() {
    let root = tempfile::tempdir().unwrap();
    let header = [0u8; 24];
    let mut session = Session::create(&config(root.path()), &header, 123).unwrap();
    let path = session.path.clone();
    assert!(path.join("screeshots").is_dir());
    let packet = UdpPacket {
        record: 42,
        timestamp_ns: 123,
        source: Ipv4Addr::new(192, 0, 2, 1),
        destination: Ipv4Addr::new(192, 0, 2, 2),
        source_port: 49152,
        destination_port: 49152,
        source_mac: [0; 6],
        destination_mac: [0; 6],
        payload: vec![],
    };
    let row = Row {
        record: 42,
        timestamp_ns: 123,
        source: "192.0.2.1:49152".into(),
        destination: "192.0.2.2:49152".into(),
        source_mac: String::new(),
        destination_mac: String::new(),
        direction: None,
        kind: "OnlineCoopSetting".into(),
        status: "authenticated".into(),
        layout: Some("confirmed_fields".into()),
        protocol: Some(8),
        port: None,
        chunk: Some(0),
        fields: vec![],
        authenticated_payload_hex: Some("must-not-be-in-dataset".into()),
        wave_preview: None,
    };
    session
        .write_packet(&packet, b"original record", &[row])
        .unwrap();
    session.finish("test_stop", false).unwrap();
    assert_eq!(
        std::fs::read(path.join("capture.pcap")).unwrap(),
        [header.as_slice(), b"original record"].concat()
    );
    let text = std::fs::read_to_string(path.join("packets.jsonl")).unwrap();
    assert!(!text.contains("must-not-be-in-dataset"));
    let saved: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(saved["saved_record"], 1);
    assert_eq!(saved["capture_record"], 42);
    assert_eq!(saved["rows"][0]["kind"], "OnlineCoopSetting");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["end_reason"], "test_stop");
    assert_eq!(manifest["packet_count"], 1);
}

#[test]
fn session_uuids_do_not_reuse_paths_and_outputs_are_private() {
    let root = tempfile::tempdir().unwrap();
    let a = Session::create(&config(root.path()), &[0; 24], 1).unwrap();
    let b = Session::create(&config(root.path()), &[0; 24], 1).unwrap();
    assert_ne!(a.path, b.path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&a.path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(a.path.join("capture.pcap"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn armed_session_silence_closes_without_waiting_for_another_packet() {
    let root = tempfile::tempdir().unwrap();
    let session = Session::create(&config(root.path()), &[0; 24], 1).unwrap();
    let path = session.path.clone();
    let mut state = State {
        session: Some(session),
        error: None,
    };
    assert!(
        state
            .tick_at(Instant::now() + SESSION_IDLE_TIMEOUT + Duration::from_secs(1))
            .unwrap()
    );
    assert!(state.session.is_none());
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["end_reason"], "session_idle_timeout");
    assert_eq!(manifest["incomplete"], true);
}

#[test]
fn pending_timeout_reset_is_not_consumed_before_session_lock() {
    let root = tempfile::tempdir().unwrap();
    let state = Arc::new(Mutex::new(State {
        session: None,
        error: None,
    }));
    let reset = Arc::new(AtomicBool::new(true));
    let mut recorder = Recorder {
        config: config(root.path()),
        header: [0; 24],
        gate: Gate::default(),
        state: state.clone(),
        stop: Arc::new(AtomicBool::new(false)),
        reset_gate: reset.clone(),
        monitor: None,
    };
    let packet = UdpPacket {
        record: 1,
        timestamp_ns: 0,
        source: Ipv4Addr::new(192, 0, 2, 1),
        destination: Ipv4Addr::new(192, 0, 2, 2),
        source_port: 49152,
        destination_port: 49152,
        source_mac: [0; 6],
        destination_mac: [0; 6],
        payload: vec![],
    };
    let lock = state.lock().unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        ready_tx.send(()).unwrap();
        recorder.observe(&packet, &[], &[]).unwrap();
    });
    ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let pending_while_locked = reset.load(Ordering::SeqCst);
    drop(lock);
    thread.join().unwrap();
    assert!(
        pending_while_locked,
        "timeout reset must be applied atomically with the session decision"
    );
}
