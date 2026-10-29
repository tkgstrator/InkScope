use super::*;
use aes::Aes128;
use aes_gcm::aead::{
    AeadInOut, KeyInit,
    consts::{U8, U12},
};
use aes_gcm::{AesGcm, Nonce};
use std::io::Cursor;

#[test]
fn normal_stream_keeps_recognized_private_setting() {
    assert!(visible_in_stream(
        DisplayMode::Normal,
        false,
        Some(11),
        "PrivateSetting",
        Some("confirmed_fields"),
        false
    ));
    assert!(!visible_in_stream(
        DisplayMode::Normal,
        false,
        Some(11),
        "BroadcastReliableProtocol",
        Some("type_only"),
        false
    ));
    assert!(visible_in_stream(
        DisplayMode::Debug,
        false,
        Some(11),
        "BroadcastReliableProtocol",
        Some("type_only"),
        false
    ));
    assert!(!visible_in_stream(
        DisplayMode::Normal,
        true,
        Some(3),
        "RTTProtocol",
        Some("confirmed_fields"),
        false
    ));
    assert!(!visible_in_stream(
        DisplayMode::Normal,
        true,
        Some(3),
        "RTTProtocol",
        Some("confirmed_fields"),
        true
    ));
    assert!(!visible_in_stream(
        DisplayMode::Normal,
        false,
        Some(3),
        "RTTProtocol",
        Some("confirmed_fields"),
        true
    ));
    assert!(visible_in_stream(
        DisplayMode::Debug,
        false,
        Some(3),
        "RTTProtocol",
        Some("confirmed_fields"),
        true
    ));
    assert!(visible_in_stream(
        DisplayMode::Debug,
        true,
        Some(3),
        "RTTProtocol",
        Some("confirmed_fields"),
        true
    ));
}

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

#[test]
fn salmon_mode_keeps_only_authenticated_known_salmon_rows() {
    let cli = Cli::try_parse_from([
        "inkscope",
        "capture",
        "--interface",
        "test0",
        "--output",
        "salmon.pcap",
        "--mode",
        "salmon",
    ])
    .unwrap();
    assert!(cli.mode == DisplayMode::Salmon);
    let mut row = Row {
        record: 1,
        timestamp_ns: 0,
        source: String::new(),
        destination: String::new(),
        source_mac: String::new(),
        destination_mac: String::new(),
        direction: None,
        kind: "CoopSeqPlayerNetState".into(),
        status: "authenticated".into(),
        layout: Some("confirmed_fields".into()),
        protocol: Some(5),
        port: None,
        chunk: Some(0),
        fields: vec![],
        authenticated_payload_hex: None,
        wave_preview: None,
    };
    assert!(salmon_row(&row));
    row.status = "key_missing".into();
    assert!(!salmon_row(&row));
    row.status = "authenticated".into();
    row.kind = "UnreliableProtocol".into();
    assert!(!salmon_row(&row));
    row.kind = "OnlineCoopSetting".into();
    row.protocol = Some(8);
    assert!(salmon_row(&row));
    for kind in ["PlayerNetEvent::Revival", "PlayerNetEvent::TroubleDie"] {
        row.kind = kind.into();
        assert!(
            salmon_row(&row),
            "Salmon lifecycle must survive filtered capture"
        );
        row.status = "authentication_failed".into();
        assert!(!salmon_row(&row));
        row.status = "authenticated".into();
    }
    row.kind = "PlayerNetEvent::RevivalExtra".into();
    assert!(!salmon_row(&row));
    row.kind = "PlayerNetEvent::Attack".into();
    assert!(!salmon_row(&row));
    row.kind = "BrowseReply".into();
    row.status = "proof_verified".into();
    assert!(!salmon_row(&row));
}

fn test_pia_frame(protocol: u8, payload: &[u8], sequence: u16) -> Vec<u8> {
    test_pia_frame_with_port(protocol, payload, sequence, None)
}

fn test_pia_frame_with_port(
    protocol: u8,
    payload: &[u8],
    sequence: u16,
    port: Option<u8>,
) -> Vec<u8> {
    let mut body = vec![if port.is_some() { 0x0e } else { 0x06 }];
    body.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    body.push(protocol);
    if let Some(port) = port {
        body.push(port);
    }
    body.extend_from_slice(payload);
    let padding = (16 - body.len() % 16) % 16;
    let mut wire = vec![0u8; 29];
    wire[..5].copy_from_slice(b"\x32\xab\x98\x64\x8f");
    wire[5] = (padding as u8) << 4;
    wire[10..12].copy_from_slice(&sequence.to_be_bytes());
    wire[13..21].copy_from_slice(&[9; 8]);
    body.resize(body.len() + padding, 0xff);
    let mut nonce = [0u8; 12];
    nonce[..4].copy_from_slice(&[192, 0, 2, 11]);
    nonce[4..].copy_from_slice(&[9; 8]);
    let cipher = AesGcm::<Aes128, U12, U8>::new_from_slice(&[7; 16]).unwrap();
    let tag = cipher
        .encrypt_inout_detached(&Nonce::from(nonce), b"", body.as_mut_slice().into())
        .unwrap();
    wire[21..29].copy_from_slice(&tag);
    wire.extend(body);

    let mut frame = vec![0u8; 14 + 20 + 8];
    frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    frame[14] = 0x45;
    frame[16..18].copy_from_slice(&(28u16 + wire.len() as u16).to_be_bytes());
    frame[23] = 17;
    frame[26..30].copy_from_slice(&[192, 0, 2, 11]);
    frame[30..34].copy_from_slice(&[192, 0, 2, 10]);
    frame[34..36].copy_from_slice(&49152u16.to_be_bytes());
    frame[36..38].copy_from_slice(&49152u16.to_be_bytes());
    frame[38..40].copy_from_slice(&(8u16 + wire.len() as u16).to_be_bytes());
    frame.extend(wire);
    frame
}

#[test]
fn salmon_mode_saves_only_original_matching_pcap_records() {
    let non_salmon = test_pia_frame(3, &[0; 21], 1);
    let mut state = vec![0u8; 16];
    state[..8].copy_from_slice(&(1u64 | (1107u64 << 13) | (1u64 << 45)).to_le_bytes());
    let salmon = test_pia_frame(5, &state, 2);
    let mut failed_auth = test_pia_frame(5, &state, 3);
    failed_auth[14 + 20 + 8 + 21] ^= 0x80;
    let mut input = vec![0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0];
    input.extend([0; 8]);
    input.extend(65535u32.to_le_bytes());
    input.extend(1u32.to_le_bytes());
    let mut expected = input.clone();
    for (sequence, frame) in [(1u32, non_salmon), (2, salmon), (3, failed_auth)] {
        let mut record = vec![];
        for word in [sequence, 0, frame.len() as u32, frame.len() as u32] {
            record.extend(word.to_le_bytes());
        }
        record.extend(frame);
        input.extend(&record);
        if sequence == 2 {
            expected.extend(record);
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("salmon.pcap");
    let cli = Cli::try_parse_from([
        "inkscope",
        "capture",
        "--interface",
        "test0",
        "--output",
        "salmon.pcap",
        "--mode",
        "salmon",
    ])
    .unwrap();
    let mut decoder = Decoder::new(
        None,
        Some((Ipv4Addr::new(192, 0, 2, 11), PiaKey([7; 16]))),
        None,
    );
    process(
        Cursor::new(input),
        &cli,
        &mut decoder,
        None,
        None,
        None,
        Some(private_output(&output).unwrap()),
    )
    .unwrap();
    assert_eq!(std::fs::read(&output).unwrap(), expected);
    let mut reader = Reader::new(File::open(output).unwrap()).unwrap();
    let packet = reader.next_record().unwrap().unwrap().unwrap();
    let mut readback_decoder = Decoder::new(
        None,
        Some((Ipv4Addr::new(192, 0, 2, 11), PiaKey([7; 16]))),
        None,
    );
    let rows = readback_decoder.process(&packet, false);
    assert_eq!(rows[0].kind, "CoopSeqPlayerNetState");
    assert_eq!(rows[0].status, "authenticated");
    assert!(reader.next_record().unwrap().is_none());
}

#[test]
fn only_opt_in_capture_can_start_without_a_key() {
    let controlled = Cli::try_parse_from([
        "inkscope",
        "capture",
        "--interface",
        "test0",
        "--output",
        "capture.pcap",
        "--control-port",
        "10253",
    ])
    .unwrap();
    assert!(decoder(&controlled).is_ok());

    let ordinary = Cli::try_parse_from([
        "inkscope",
        "capture",
        "--interface",
        "test0",
        "--output",
        "capture.pcap",
    ])
    .unwrap();
    assert!(decoder(&ordinary).is_err());

    let inspect = Cli::try_parse_from(["inkscope", "inspect", "capture.pcap"]).unwrap();
    assert!(decoder(&inspect).is_err());
    assert!(
        Cli::try_parse_from([
            "inkscope",
            "inspect",
            "capture.pcap",
            "--control-port",
            "10253",
        ])
        .is_err()
    );
}

#[test]
fn stale_managed_pia_key_requires_three_peer_failures_and_keeps_capture_running() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pia_key.bin");
    std::fs::write(&path, [7u8; 16]).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let cache = ManagedPiaCache::loaded(path.clone(), &[7; 16]).unwrap();
    let peer = Ipv4Addr::new(192, 0, 2, 11);
    let mut monitor = PiaCacheMonitor {
        cache,
        peer,
        failures: 0,
        notified: false,
    };
    let mut decoder = Decoder::new(None, Some((peer, PiaKey([7; 16]))), None);
    let packet = UdpPacket {
        record: 1,
        timestamp_ns: 0,
        source: peer,
        destination: Ipv4Addr::new(192, 0, 2, 10),
        source_port: 49152,
        destination_port: 49152,
        source_mac: [0; 6],
        destination_mac: [0; 6],
        payload: vec![],
    };
    monitor
        .observe(Some(false), &packet, &[], &mut decoder)
        .unwrap();
    monitor
        .observe(Some(true), &packet, &[], &mut decoder)
        .unwrap();
    monitor
        .observe(Some(false), &packet, &[], &mut decoder)
        .unwrap();
    monitor
        .observe(Some(false), &packet, &[], &mut decoder)
        .unwrap();
    assert!(path.exists());
    monitor
        .observe(Some(false), &packet, &[], &mut decoder)
        .unwrap();
    assert!(!path.exists());
    assert!(monitor.notified);
    assert_eq!(decoder.manual_pia_authentication(&packet), None);
}

#[test]
fn cache_file_error_does_not_stop_packet_processing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pia_key.bin");
    std::fs::write(&path, [7u8; 16]).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let cache = ManagedPiaCache::loaded(path.clone(), &[7; 16]).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let peer = Ipv4Addr::new(192, 0, 2, 11);
    let mut monitor = PiaCacheMonitor {
        cache,
        peer,
        failures: 0,
        notified: false,
    };
    let mut decoder = Decoder::new(None, Some((peer, PiaKey([7; 16]))), None);
    let packet = UdpPacket {
        record: 1,
        timestamp_ns: 0,
        source: peer,
        destination: Ipv4Addr::new(192, 0, 2, 10),
        source_port: 49152,
        destination_port: 49152,
        source_mac: [0; 6],
        destination_mac: [0; 6],
        payload: vec![],
    };
    for _ in 0..3 {
        monitor
            .observe(Some(false), &packet, &[], &mut decoder)
            .unwrap();
    }
    assert!(monitor.notified);
    assert!(path.is_dir());
    assert_eq!(decoder.manual_pia_authentication(&packet), None);
}

#[test]
fn monitor_rejects_a_cache_replaced_after_decoder_loaded_its_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pia_key.bin");
    std::fs::write(&path, [7u8; 16]).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let peer = Ipv4Addr::new(192, 0, 2, 11);
    let decoder = Decoder::new(None, Some((peer, PiaKey([7; 16]))), None);
    assert!(track_loaded_cache(&path, peer, &decoder).is_ok());
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, [8u8; 16]).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert!(track_loaded_cache(&path, peer, &decoder).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), [8u8; 16]);
}

#[test]
fn hdmi_capture_uses_session_directories_instead_of_a_flat_pcap() {
    let cli = Cli::try_parse_from([
        "inkscope",
        "capture",
        "--interface",
        "test0",
        "--hdmi-device",
        "HDMI capture device",
    ])
    .expect("HDMI capture should not require a flat --output file");
    assert!(matches!(cli.command, Mode::Capture { output: None, .. }));
    assert!(
        Cli::try_parse_from([
            "inkscope",
            "capture",
            "--interface",
            "test0",
            "--hdmi-device",
            "0",
            "--output",
            "flat.pcap",
        ])
        .is_err()
    );
    assert!(
        Cli::try_parse_from([
            "inkscope",
            "capture",
            "--interface",
            "test0",
            "--in-game-dir",
            "captures/in_game",
        ])
        .is_err()
    );
    assert!(
        Cli::try_parse_from(["inkscope", "inspect", "old.pcap", "--hdmi-device", "0",]).is_err()
    );
}

#[test]
fn decoded_capture_backed_payloads_open_the_gameplay_gate() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/in-game-payloads-v1.json")).unwrap();
    let mut stream = vec![0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0];
    stream.extend([0; 8]);
    stream.extend(65535u32.to_le_bytes());
    stream.extend(1u32.to_le_bytes());
    for (i, entry) in fixture["entries"].as_array().unwrap().iter().enumerate() {
        let frame = test_pia_frame_with_port(
            entry["protocol"].as_u64().unwrap() as u8,
            &hex::decode(entry["payload_hex"].as_str().unwrap()).unwrap(),
            i as u16 + 1,
            Some(entry["port"].as_u64().unwrap() as u8),
        );
        for n in [i as u32 + 1, 0, frame.len() as u32, frame.len() as u32] {
            stream.extend(n.to_le_bytes());
        }
        stream.extend(frame);
    }
    let mut reader = Reader::new(Cursor::new(stream)).unwrap();
    let mut decoder = Decoder::new(
        None,
        Some((Ipv4Addr::new(192, 0, 2, 11), PiaKey([7; 16]))),
        None,
    );
    let mut gate = in_game::Gate::default();
    let setting = reader.next_record().unwrap().unwrap().unwrap();
    let setting_rows = decoder.process(&setting, true);
    assert!(gate.observe(&setting, &setting_rows).start_session);
    let state = reader.next_record().unwrap().unwrap().unwrap();
    let state_rows = decoder.process(&state, true);
    assert_eq!(
        gate.observe(&state, &state_rows).video,
        in_game::VideoAction::Start
    );
}

#[test]
#[cfg(unix)]
fn backend_failure_after_complete_record_keeps_session_incomplete() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/in-game-payloads-v1.json")).unwrap();
    let setting = &fixture["entries"][0];
    let frame = test_pia_frame_with_port(
        8,
        &hex::decode(setting["payload_hex"].as_str().unwrap()).unwrap(),
        1,
        Some(100),
    );
    let mut stream = vec![0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0];
    stream.extend([0; 8]);
    stream.extend(65535u32.to_le_bytes());
    stream.extend(1u32.to_le_bytes());
    for n in [1u32, 0, frame.len() as u32, frame.len() as u32] {
        stream.extend(n.to_le_bytes());
    }
    stream.extend(frame);
    let dir = tempfile::tempdir().unwrap();
    let cli = Cli::try_parse_from([
        "inkscope",
        "capture",
        "--interface",
        "test0",
        "--hdmi-device",
        "0",
    ])
    .unwrap();
    let child = std::process::Command::new("/bin/sh")
        .args(["-c", "exit 7"])
        .spawn()
        .unwrap();
    let child = Arc::new(Mutex::new(child));
    let mut recorder = in_game_capture::Recorder::start(
        in_game_capture::Config {
            root: dir.path().to_owned(),
            video: hdmi::VideoConfig {
                device: "0".into(),
                framerate: 60,
                video_size: "1920x1080".into(),
                ffmpeg: "ffmpeg".into(),
            },
        },
        child.clone(),
    )
    .unwrap();
    let mut decoder = Decoder::new(
        None,
        Some((Ipv4Addr::new(192, 0, 2, 11), PiaKey([7; 16]))),
        None,
    );
    process_inner(
        Cursor::new(stream),
        &cli,
        &mut decoder,
        None,
        None,
        None,
        CaptureOutputs {
            salmon: None,
            game: Some(&mut recorder),
        },
    )
    .unwrap();
    assert!(!child.lock().unwrap().wait().unwrap().success());
    drop(recorder);
    let session = std::fs::read_dir(dir.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(session.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(
        manifest["incomplete"], true,
        "backend failure must not be persisted as a complete recording"
    );
}
