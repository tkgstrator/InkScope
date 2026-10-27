use serde_json::Value;
use std::fs;
use std::net::TcpListener;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::{Message, accept, accept_hdr};

fn one_packet_pcap() -> Vec<u8> {
    let mut frame = vec![0u8; 14 + 20 + 8 + 3];
    frame[..6].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
    frame[6..12].copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
    frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    frame[14] = 0x45;
    frame[16..18].copy_from_slice(&31u16.to_be_bytes());
    frame[23] = 17;
    frame[26..30].copy_from_slice(&[192, 0, 2, 1]);
    frame[30..34].copy_from_slice(&[192, 0, 2, 2]);
    frame[34..36].copy_from_slice(&49152u16.to_be_bytes());
    frame[36..38].copy_from_slice(&49152u16.to_be_bytes());
    frame[38..40].copy_from_slice(&11u16.to_be_bytes());
    frame[42..].copy_from_slice(&[1, 2, 3]);

    let mut pcap = vec![];
    pcap.extend_from_slice(&[0xd4, 0xc3, 0xb2, 0xa1]);
    pcap.extend_from_slice(&[2, 0, 4, 0]);
    pcap.extend_from_slice(&[0; 8]);
    pcap.extend_from_slice(&65535u32.to_le_bytes());
    pcap.extend_from_slice(&1u32.to_le_bytes());
    for word in [
        1_792_937_154u32,
        105_000,
        frame.len() as u32,
        frame.len() as u32,
    ] {
        pcap.extend_from_slice(&word.to_le_bytes());
    }
    pcap.extend_from_slice(&frame);
    pcap
}

fn run_inspect(
    pcap: &std::path::Path,
    url: Option<&str>,
    token: Option<&str>,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_inkscope"));
    command
        .arg("inspect")
        .arg(pcap)
        .args(["--game-key", "00000000000000000000000000000000"])
        .env_remove("INKSCOPE_VISUALIZER_URL")
        .env_remove("INKSCOPE_VISUALIZER_TOKEN");
    if let Some(url) = url {
        command.arg("--visualizer-url").arg(url);
    }
    if let Some(token) = token {
        command.env("INKSCOPE_VISUALIZER_TOKEN", token);
    }
    command.output().unwrap()
}

#[test]
#[allow(clippy::result_large_err)] // tungstenite's required handshake callback has a large error type.
fn opt_in_websocket_receives_raw_packet_without_changing_terminal_output() {
    let dir = tempfile::tempdir().unwrap();
    let pcap = dir.path().join("one.pcap");
    fs::write(&pcap, one_packet_pcap()).unwrap();
    let baseline = run_inspect(&pcap, None, None);
    assert!(baseline.status.success());

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10))
                },
                Err(error) => panic!("WebSocket connection missing: {error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut authorization = None;
        let mut websocket = accept_hdr(
            stream,
            |request: &tungstenite::handshake::server::Request,
             response: tungstenite::handshake::server::Response| {
                authorization = request
                    .headers()
                    .get("authorization")
                    .map(|value| value.to_str().unwrap().to_owned());
                Ok(response)
            },
        )
        .unwrap();
        let message = websocket.read().unwrap();
        let event: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
        websocket
            .send(Message::Text(
                serde_json::json!({
                    "version": 1,
                    "type": "ack",
                    "session_id": event["session_id"],
                    "record": event["record"],
                    "result": "accepted",
                    "persistence": "memory"
                })
                .to_string()
                .into(),
            ))
            .unwrap();
        (authorization, message.into_text().unwrap().to_string())
    });
    let url = format!("ws://127.0.0.1:{}/api/ingest", address.port());
    let output = run_inspect(&pcap, Some(&url), Some("test-token"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, baseline.stdout);
    assert!(!String::from_utf8_lossy(&output.stderr).contains("Visualizer export"));
    let (authorization, message) = server.join().unwrap();
    assert_eq!(authorization.as_deref(), Some("Bearer test-token"));
    let envelope: Value = serde_json::from_str(&message).unwrap();
    assert_eq!(envelope["version"], 1);
    assert_eq!(envelope["type"], "packet");
    assert_eq!(envelope["record"], "1");
    assert_eq!(envelope["timestamp_ns"], "1792937154105000000");
    assert_eq!(envelope["packet"]["payload_hex"], "010203");
    assert_eq!(envelope["packet"]["source_ip"], "192.0.2.1");
    assert!(
        envelope["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row.get("authenticated_payload_hex").is_none())
    );
    assert!(uuid::Uuid::parse_str(envelope["session_id"].as_str().unwrap()).is_ok());
}

#[test]
fn unavailable_visualizer_does_not_stop_offline_inspection() {
    let dir = tempfile::tempdir().unwrap();
    let pcap = dir.path().join("one.pcap");
    fs::write(&pcap, one_packet_pcap()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let start = Instant::now();
    let output = run_inspect(
        &pcap,
        Some(&format!("ws://127.0.0.1:{}/api/ingest", address.port())),
        Some("test-token"),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(String::from_utf8_lossy(&output.stdout).contains("Pia"));
}

#[test]
fn visualizer_export_requires_a_token_and_loopback_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let pcap = dir.path().join("one.pcap");
    fs::write(&pcap, one_packet_pcap()).unwrap();

    let no_token = run_inspect(&pcap, Some("ws://127.0.0.1:10252/api/ingest"), None);
    assert!(!no_token.status.success());
    assert!(String::from_utf8_lossy(&no_token.stderr).contains("INKSCOPE_VISUALIZER_TOKEN"));
    let remote = run_inspect(
        &pcap,
        Some("ws://192.0.2.10:10252/api/ingest"),
        Some("test-token"),
    );
    assert!(!remote.status.success());
    assert!(String::from_utf8_lossy(&remote.stderr).contains("loopback"));
}

#[test]
fn ingest_error_does_not_interrupt_inspection_and_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let pcap = dir.path().join("one.pcap");
    fs::write(&pcap, one_packet_pcap()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut websocket = accept(stream).unwrap();
        websocket.read().unwrap();
        let _ = websocket.send(Message::Text(
            r#"{"version":1,"type":"error","code":"invalid_packet"}"#.into(),
        ));
    });
    let output = run_inspect(
        &pcap,
        Some(&format!("ws://127.0.0.1:{port}/api/ingest")),
        Some("test-token"),
    );
    server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Pia"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("dropped 1 UI copies"));
}
