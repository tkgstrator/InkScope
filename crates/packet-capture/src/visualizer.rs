use inkscope::decode::{Field, Row};
use inkscope::pcap::UdpPacket;
use inkscope::preview::WavePreview;
use serde::Serialize;
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::{Message, WebSocket, client};
use uuid::Uuid;

const MAX_WIRE_BYTES: usize = 512 * 1024;
const QUEUE_CAPACITY: usize = 256;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(350);
const RECONNECT_INTERVAL: Duration = Duration::from_secs(1);
const FINISH_GRACE: Duration = Duration::from_secs(2);
const ACK_TIMEOUT: Duration = Duration::from_millis(1500);

struct Outbound {
    json: String,
    record: String,
    session_id: String,
}

#[derive(Default)]
struct SenderReport {
    dropped: u64,
    uncertain: u64,
}

enum AckFailure {
    Rejected,
    Uncertain,
}

pub(crate) struct Publisher {
    session_id: String,
    sender: SyncSender<Outbound>,
    worker: JoinHandle<SenderReport>,
    queue_drops: u64,
    connection: ConnectionState,
}

#[derive(Clone)]
pub(crate) struct ConnectionState(Arc<AtomicU8>);

impl ConnectionState {
    pub(crate) fn label(&self) -> &'static str {
        match self.0.load(Ordering::Relaxed) {
            1 => "connected",
            2 => "degraded",
            3 => "connecting",
            _ => "idle",
        }
    }
}

impl Publisher {
    pub(crate) fn start(cli_url: Option<&str>) -> Result<Option<Self>, String> {
        let configured_url = cli_url
            .map(str::to_owned)
            .or_else(|| std::env::var("INKSCOPE_VISUALIZER_URL").ok());
        let Some(configured_url) = configured_url else {
            return Ok(None);
        };
        let (url, address) = validate_url(&configured_url)?;
        let token = std::env::var("INKSCOPE_VISUALIZER_TOKEN").map_err(|_| {
            "INKSCOPE_VISUALIZER_TOKEN is required when Visualizer export is enabled".to_owned()
        })?;
        if token.is_empty()
            || token
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err(
                "INKSCOPE_VISUALIZER_TOKEN must contain non-whitespace header characters".into(),
            );
        }
        let (sender, receiver) = mpsc::sync_channel::<Outbound>(QUEUE_CAPACITY);
        let connection = ConnectionState(Arc::new(AtomicU8::new(0)));
        let worker_connection = connection.clone();
        let worker =
            thread::spawn(move || sender_loop(receiver, url, address, token, worker_connection));
        Ok(Some(Self {
            session_id: Uuid::new_v4().to_string(),
            sender,
            worker,
            queue_drops: 0,
            connection,
        }))
    }

    pub(crate) fn connection_state(&self) -> ConnectionState {
        self.connection.clone()
    }

    pub(crate) fn emit(&mut self, packet: &UdpPacket, rows: &[Row]) {
        let json = encode_packet(&self.session_id, packet, rows);
        if json.len() > MAX_WIRE_BYTES {
            self.queue_drops += 1;
            return;
        }
        let outbound = Outbound {
            json,
            record: packet.record.to_string(),
            session_id: self.session_id.clone(),
        };
        if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) =
            self.sender.try_send(outbound)
        {
            self.queue_drops += 1;
        }
    }

    pub(crate) fn finish(self) {
        drop(self.sender);
        let deadline = Instant::now() + FINISH_GRACE;
        while !self.worker.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if !self.worker.is_finished() {
            eprintln!("Visualizer export sender is still busy; pending UI copies may be lost");
            return;
        }
        let report = match self.worker.join() {
            Ok(report) => report,
            Err(_) => {
                eprintln!("Visualizer export sender failed; pending UI copies may be lost");
                return;
            },
        };
        let total_drops = self.queue_drops + report.dropped;
        if total_drops > 0 {
            eprintln!(
                "Visualizer export dropped {total_drops} UI copies; capture and terminal output were unaffected"
            );
        }
        if report.uncertain > 0 {
            eprintln!(
                "Visualizer export has {} transmissions without acknowledgement; acceptance is unknown",
                report.uncertain
            );
        }
    }
}

fn sender_loop(
    receiver: Receiver<Outbound>,
    url: String,
    address: SocketAddr,
    token: String,
    connection: ConnectionState,
) -> SenderReport {
    let mut socket: Option<WebSocket<TcpStream>> = None;
    let mut next_connect = Instant::now();
    let mut report = SenderReport::default();
    while let Ok(outbound) = receiver.recv() {
        if socket.is_none() && Instant::now() >= next_connect {
            connection.0.store(3, Ordering::Relaxed);
            match connect_local(&url, address, &token) {
                Ok(connected) => {
                    connection.0.store(1, Ordering::Relaxed);
                    socket = Some(connected);
                },
                Err(_) => {
                    connection.0.store(2, Ordering::Relaxed);
                    next_connect = Instant::now() + RECONNECT_INTERVAL;
                },
            }
        }
        if let Some(connected) = socket.as_mut() {
            if connected.send(Message::Text(outbound.json.into())).is_err() {
                report.uncertain += 1;
                socket = None;
                connection.0.store(2, Ordering::Relaxed);
                next_connect = Instant::now() + RECONNECT_INTERVAL;
            } else {
                match read_ack(connected, &outbound.session_id, &outbound.record) {
                    Ok(true) => continue,
                    Ok(false) | Err(AckFailure::Rejected) => report.dropped += 1,
                    Err(AckFailure::Uncertain) => report.uncertain += 1,
                }
                socket = None;
                connection.0.store(2, Ordering::Relaxed);
                next_connect = Instant::now() + RECONNECT_INTERVAL;
            }
        } else {
            report.dropped += 1;
        }
    }
    report
}

fn validate_url(url: &str) -> Result<(String, SocketAddr), String> {
    let remainder = url
        .strip_prefix("ws://127.0.0.1:")
        .ok_or("Visualizer URL must use ws://127.0.0.1:<port>/api/ingest (loopback only)")?;
    let (port, path) = remainder
        .split_once('/')
        .ok_or("Visualizer URL must use ws://127.0.0.1:<port>/api/ingest (loopback only)")?;
    let port: u16 = port
        .parse()
        .map_err(|_| "Visualizer loopback port is invalid")?;
    if port == 0 || path != "api/ingest" {
        return Err(
            "Visualizer URL must use ws://127.0.0.1:<port>/api/ingest (loopback only)".into(),
        );
    }
    Ok((url.to_owned(), SocketAddr::from(([127, 0, 0, 1], port))))
}

fn connect_local(
    url: &str,
    address: SocketAddr,
    token: &str,
) -> Result<WebSocket<TcpStream>, String> {
    let stream =
        TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(CONNECT_TIMEOUT))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(CONNECT_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let mut request = url
        .into_client_request()
        .map_err(|error| error.to_string())?;
    let bearer = HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|_| "Visualizer token is not a valid HTTP header")?;
    request.headers_mut().insert("Authorization", bearer);
    let (mut socket, _) = client(request, stream).map_err(|error| error.to_string())?;
    socket
        .get_mut()
        .set_read_timeout(Some(ACK_TIMEOUT))
        .map_err(|error| error.to_string())?;
    Ok(socket)
}

fn read_ack(
    socket: &mut WebSocket<TcpStream>,
    session_id: &str,
    record: &str,
) -> Result<bool, AckFailure> {
    let frame = socket.read().map_err(|_| AckFailure::Uncertain)?;
    let text = frame.to_text().map_err(|_| AckFailure::Uncertain)?;
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| AckFailure::Uncertain)?;
    if value["version"] != 1 {
        return Err(AckFailure::Uncertain);
    }
    if value["type"] == "error" {
        return Err(AckFailure::Rejected);
    }
    if value["type"] != "ack" || value["session_id"] != session_id || value["record"] != record {
        return Err(AckFailure::Uncertain);
    }
    match value["result"].as_str() {
        Some("accepted" | "duplicate") => Ok(true),
        Some("stale") => Ok(false),
        _ => Err(AckFailure::Uncertain),
    }
}

#[derive(Serialize)]
struct WirePacket {
    source_ip: String,
    source_port: u16,
    source_mac: String,
    destination_ip: String,
    destination_port: u16,
    destination_mac: String,
    payload_hex: String,
}

#[derive(Serialize)]
struct WireRow<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    direction: Option<&'a str>,
    kind: &'a str,
    status: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    layout: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chunk: Option<usize>,
    fields: Vec<&'a Field>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wave_preview: Option<&'a WavePreview>,
}

#[derive(Serialize)]
struct Envelope<'a> {
    version: u8,
    #[serde(rename = "type")]
    event_type: &'static str,
    session_id: &'a str,
    record: String,
    timestamp_ns: String,
    packet: WirePacket,
    rows: Vec<WireRow<'a>>,
}

fn mac(bytes: &[u8; 6]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn safe_field(field: &Field) -> bool {
    let name = field.name.to_ascii_lowercase();
    !["gamekey", "piakey", "randomtable", "secret"]
        .iter()
        .any(|sensitive| name.contains(sensitive))
}

pub(crate) fn encode_packet(session_id: &str, packet: &UdpPacket, rows: &[Row]) -> String {
    let envelope = Envelope {
        version: 1,
        event_type: "packet",
        session_id,
        record: packet.record.to_string(),
        timestamp_ns: packet.timestamp_ns.to_string(),
        packet: WirePacket {
            source_ip: packet.source.to_string(),
            source_port: packet.source_port,
            source_mac: mac(&packet.source_mac),
            destination_ip: packet.destination.to_string(),
            destination_port: packet.destination_port,
            destination_mac: mac(&packet.destination_mac),
            payload_hex: hex::encode(&packet.payload),
        },
        rows: rows
            .iter()
            .map(|row| WireRow {
                direction: row.direction.as_deref(),
                kind: &row.kind,
                status: &row.status,
                layout: row.layout.as_deref(),
                protocol: row.protocol,
                port: row.port,
                chunk: row.chunk,
                fields: row
                    .fields
                    .iter()
                    .filter(|field| safe_field(field))
                    .collect(),
                wave_preview: row.wave_preview.as_ref(),
            })
            .collect(),
    };
    serde_json::to_string(&envelope).expect("packet envelope contains JSON-compatible values")
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkscope::decode::Field;
    use serde_json::Value;
    use std::net::Ipv4Addr;

    #[test]
    fn packet_envelope_matches_backend_fixture_and_excludes_key_material() {
        let expected: Value =
            serde_json::from_str(include_str!("../test_support/fixtures/packet-v1.json")).unwrap();
        let packet = UdpPacket {
            record: 1,
            timestamp_ns: 1_793_059_200_123_456_789,
            source: Ipv4Addr::new(192, 0, 2, 10),
            destination: Ipv4Addr::new(192, 0, 2, 11),
            source_port: 49152,
            destination_port: 49153,
            source_mac: [2, 0, 0, 0, 0, 16],
            destination_mac: [2, 0, 0, 0, 0, 17],
            payload: vec![0, 0x32, 0xab, 0xcd],
        };
        let row = Row {
            record: packet.record,
            timestamp_ns: packet.timestamp_ns,
            source: "192.0.2.10:49152".into(),
            destination: "192.0.2.11:49153".into(),
            source_mac: "02:00:00:00:00:10".into(),
            destination_mac: "02:00:00:00:00:11".into(),
            direction: None,
            kind: "RTTProtocol".into(),
            status: "authenticated".into(),
            layout: None,
            protocol: Some(3),
            port: None,
            chunk: None,
            fields: vec![
                Field {
                    name: "Type".into(),
                    value: "1".into(),
                    bits: Some([0, 8]),
                },
                Field {
                    name: "CaptureResponseDelayMs".into(),
                    value: "42.6".into(),
                    bits: Some([8, 40]),
                },
                Field {
                    name: "PiaKey".into(),
                    value: "must-not-send".into(),
                    bits: None,
                },
            ],
            authenticated_payload_hex: Some("deadbeef".into()),
            wave_preview: None,
        };
        let actual: Value = serde_json::from_str(&encode_packet(
            "d61c1c49-4dae-4c6f-aa41-4f7f2a44411e",
            &packet,
            &[row],
        ))
        .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn publisher_shutdown_does_not_wait_for_a_stalled_sender() {
        let (sender, _receiver) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(3));
            SenderReport::default()
        });
        let publisher = Publisher {
            session_id: "unused".into(),
            sender,
            worker,
            queue_drops: 0,
            connection: ConnectionState(Arc::new(AtomicU8::new(0))),
        };
        let start = std::time::Instant::now();
        publisher.finish();
        assert!(start.elapsed() < std::time::Duration::from_millis(2500));
    }

    #[test]
    fn sender_reconnects_for_later_packet_after_visualizer_returns() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let url = format!("ws://127.0.0.1:{}/api/ingest", address.port());
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        let worker = std::thread::spawn(move || {
            sender_loop(
                receiver,
                url,
                address,
                "test-token".into(),
                ConnectionState(Arc::new(AtomicU8::new(0))),
            )
        });
        let session_id = "d61c1c49-4dae-4c6f-aa41-4f7f2a44411e";
        let mut event: Value =
            serde_json::from_str(include_str!("../test_support/fixtures/packet-v1.json")).unwrap();
        sender
            .send(Outbound {
                json: event.to_string(),
                record: "1".into(),
                session_id: session_id.into(),
            })
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let listener = std::net::TcpListener::bind(address).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server =
            std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
                let stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && std::time::Instant::now() < deadline =>
                        {
                            std::thread::sleep(std::time::Duration::from_millis(10))
                        },
                        Err(error) => panic!("reconnect missing: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut websocket = tungstenite::accept(stream).unwrap();
                let message = websocket.read().unwrap();
                let received: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                websocket.send(Message::Text(serde_json::json!({
                "version": 1, "type": "ack", "session_id": received["session_id"],
                "record": received["record"], "result": "accepted", "persistence": "memory"
            }).to_string().into())).unwrap();
                received["record"].clone()
            });
        std::thread::sleep(std::time::Duration::from_millis(1100));
        event["record"] = Value::String("2".into());
        sender
            .send(Outbound {
                json: event.to_string(),
                record: "2".into(),
                session_id: session_id.into(),
            })
            .unwrap();
        drop(sender);
        let report = worker.join().unwrap();
        assert_eq!(report.dropped, 1);
        assert_eq!(report.uncertain, 0);
        assert_eq!(server.join().unwrap(), "2");
    }
}
