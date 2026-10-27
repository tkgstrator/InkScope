//! Optional loopback control for a running capture. Commands are applied by
//! the decode thread at a packet boundary; this thread never owns the decoder.
use hmac::{Hmac, Mac};
use inkscope::Result;
use inkscope::crypto::{self, GameKey};
use inkscope::decode::Decoder;
use serde::Deserialize;
use serde_json::json;
use sha2::Sha256;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::visualizer::ConnectionState;

const MAX_HEADERS: usize = 4096;
const MAX_BODY: usize = 512;
const REQUEST_DEADLINE: Duration = Duration::from_secs(2);
const IO_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_CLIENTS: usize = 8;

#[derive(Clone, Copy)]
struct State {
    running: bool,
    key_epoch: u64,
    applied_epoch: u64,
    key_present: bool,
    key_locked: bool,
    key_source: &'static str,
}

impl State {
    fn key_state(self) -> &'static str {
        if self.key_epoch != self.applied_epoch {
            "pending"
        } else if self.key_present {
            "applied"
        } else {
            "absent"
        }
    }
}

fn lock_state(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|poison| poison.into_inner())
}

enum Command {
    Connect(GameKey, u64),
    Disconnect(u64),
}

impl Command {
    fn epoch(&self) -> u64 {
        match self {
            Self::Connect(_, epoch) | Self::Disconnect(epoch) => *epoch,
        }
    }
}

pub(crate) struct Control {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    receiver: Receiver<Command>,
}

impl Control {
    pub(crate) fn start(
        port: u16,
        initial_key_source: Option<&'static str>,
        publisher: Option<ConnectionState>,
    ) -> Result<Self> {
        let token = std::env::var("INKSCOPE_CONTROL_TOKEN")
            .map_err(|_| "INKSCOPE_CONTROL_TOKEN is required with --control-port")?;
        Self::start_with_token(port, initial_key_source, publisher, token)
    }

    fn start_with_token(
        port: u16,
        initial_key_source: Option<&'static str>,
        publisher: Option<ConnectionState>,
        token: String,
    ) -> Result<Self> {
        let token = Zeroizing::new(token);
        if port == 0 {
            return Err("control port must be nonzero".into());
        }
        if token.len() < 32 || !token.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
            return Err("INKSCOPE_CONTROL_TOKEN must be at least 32 printable ASCII characters without spaces".into());
        }
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
            .map_err(|error| format!("control listener unavailable: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("control listener setup failed: {error}"))?;
        let instance_id = Uuid::new_v4().to_string();
        let state = Arc::new(Mutex::new(State {
            running: false,
            key_epoch: 0,
            applied_epoch: 0,
            key_present: initial_key_source.is_some(),
            key_locked: initial_key_source.is_some(),
            key_source: initial_key_source.unwrap_or("none"),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::sync_channel(8);
        let worker_state = state.clone();
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            let token = Arc::new(token);
            let instance_id = Arc::new(instance_id);
            let mut clients: Vec<JoinHandle<()>> = Vec::new();
            let mut accept_failed = false;
            while !worker_stop.load(Ordering::Relaxed) {
                reap_clients(&mut clients);
                match listener.accept() {
                    Ok((stream, _)) => {
                        accept_failed = false;
                        if clients.len() >= MAX_CLIENTS {
                            drop(stream);
                            continue;
                        }
                        let token = token.clone();
                        let instance_id = instance_id.clone();
                        let sender = sender.clone();
                        let state = worker_state.clone();
                        let publisher = publisher.clone();
                        let stop = worker_stop.clone();
                        clients.push(thread::spawn(move || {
                            let context = HttpContext {
                                port,
                                token: &token,
                                instance_id: &instance_id,
                                sender: &sender,
                                state: &state,
                                publisher: publisher.as_ref(),
                            };
                            if let Ok(Some(stream)) = serve_one(stream, &context, true)
                                && !stop.load(Ordering::Relaxed)
                            {
                                let _ = serve_one(stream, &context, false);
                            }
                        }));
                    },
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    },
                    Err(error) => {
                        if !accept_failed {
                            eprintln!("control listener accept failed: {error}; retrying");
                            accept_failed = true;
                        }
                        thread::sleep(Duration::from_millis(100));
                    },
                }
            }
            for client in clients {
                let _ = client.join();
            }
        });
        Ok(Self {
            state,
            stop,
            worker: Some(worker),
            receiver,
        })
    }

    pub(crate) fn set_running(&self, running: bool) {
        lock_state(&self.state).running = running;
    }

    pub(crate) fn apply_pending(&self, decoder: &mut Decoder) {
        let mut latest = None;
        while let Ok(command) = self.receiver.try_recv() {
            latest = Some(command);
        }
        if let Some(command) = latest {
            let epoch = command.epoch();
            let key = match command {
                Command::Connect(key, _) => Some(key),
                Command::Disconnect(_) => None,
            };
            let key_present = key.is_some();
            decoder.replace_game_key(key);
            let mut state = lock_state(&self.state);
            state.applied_epoch = epoch;
            state.key_present = key_present;
            state.key_source = if key_present { "control" } else { "none" };
        }
    }

    pub(crate) fn finish(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn reap_clients(clients: &mut Vec<JoinHandle<()>>) {
    let mut index = 0;
    while index < clients.len() {
        if clients[index].is_finished() {
            let client = clients.swap_remove(index);
            let _ = client.join();
        } else {
            index += 1;
        }
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectBody {
    game_key: String,
}

impl Drop for ConnectBody {
    fn drop(&mut self) {
        self.game_key.zeroize();
    }
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut mismatch = left.len() ^ right.len();
    for (left, right) in left.iter().zip(right) {
        mismatch |= usize::from(left ^ right);
    }
    mismatch == 0
}

fn proof(token: &str, nonce: &str, instance_id: &str, port: u16) -> String {
    let message = format!("inkscope-control/v1\n{nonce}\n{instance_id}\n127.0.0.1:{port}");
    let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes()).expect("HMAC accepts any key");
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn response(stream: &mut TcpStream, code: u16, reason: &str, body: serde_json::Value) {
    response_with_connection(stream, code, reason, body, false);
}

fn response_with_connection(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    body: serde_json::Value,
    keep_alive: bool,
) {
    let text = body.to_string();
    let connection = if keep_alive { "keep-alive" } else { "close" };
    let header = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: {connection}\r\n\r\n",
        text.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(text.as_bytes());
}

#[derive(Clone, Copy)]
struct HttpContext<'a> {
    port: u16,
    token: &'a str,
    instance_id: &'a str,
    sender: &'a SyncSender<Command>,
    state: &'a Mutex<State>,
    publisher: Option<&'a ConnectionState>,
}

fn serve_one(
    mut stream: TcpStream,
    context: &HttpContext<'_>,
    allow_keep_alive: bool,
) -> std::io::Result<Option<TcpStream>> {
    let HttpContext {
        port,
        token,
        instance_id,
        sender,
        state,
        publisher,
    } = *context;
    // Some hosts propagate a nonblocking listener's mode to accepted sockets.
    // The per-request deadline and read timeout require blocking reads here.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let deadline = Instant::now() + REQUEST_DEADLINE;
    let mut wire = Zeroizing::new(vec![0u8; MAX_HEADERS + MAX_BODY]);
    let mut size = 0;
    let header_end = loop {
        if Instant::now() >= deadline || size >= MAX_HEADERS {
            response(
                &mut stream,
                400,
                "Bad Request",
                json!({"error":"invalid_request"}),
            );
            return Ok(None);
        }
        match stream.read(&mut wire[size..MAX_HEADERS]) {
            Ok(0) => return Ok(None),
            Ok(count) => size += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Ok(None),
        }
        if let Some(index) = wire[..size]
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
        {
            break index + 4;
        }
    };
    let head = match std::str::from_utf8(&wire[..header_end]) {
        Ok(head) => head,
        Err(_) => {
            response(
                &mut stream,
                400,
                "Bad Request",
                json!({"error":"invalid_request"}),
            );
            return Ok(None);
        },
    };
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split(' ');
    let (method, path, version) = (parts.next(), parts.next(), parts.next());
    if version != Some("HTTP/1.1") || parts.next().is_some() {
        response(
            &mut stream,
            400,
            "Bad Request",
            json!({"error":"invalid_request"}),
        );
        return Ok(None);
    }
    let mut host = None;
    let mut authorization = None;
    let mut length = None;
    let mut connection = None;
    let mut forbidden_header = false;
    for line in lines.filter(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            forbidden_header = true;
            break;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("host") {
            if host.replace(value).is_some() {
                forbidden_header = true;
            }
        } else if name.eq_ignore_ascii_case("authorization") {
            if authorization.replace(value).is_some() {
                forbidden_header = true;
            }
        } else if name.eq_ignore_ascii_case("content-length") {
            if length.replace(value).is_some() {
                forbidden_header = true;
            }
        } else if name.eq_ignore_ascii_case("connection") {
            if connection.replace(value).is_some() {
                forbidden_header = true;
            }
        } else if name.eq_ignore_ascii_case("origin")
            || name.eq_ignore_ascii_case("transfer-encoding")
        {
            forbidden_header = true;
        }
    }
    if forbidden_header || host != Some(format!("127.0.0.1:{port}").as_str()) {
        response(&mut stream, 403, "Forbidden", json!({"error":"forbidden"}));
        return Ok(None);
    }
    let content_length = match length.unwrap_or("0").parse::<usize>() {
        Ok(value) if value <= MAX_BODY => value,
        _ => {
            response(
                &mut stream,
                413,
                "Payload Too Large",
                json!({"error":"invalid_body"}),
            );
            return Ok(None);
        },
    };
    if size > header_end + content_length {
        response(
            &mut stream,
            400,
            "Bad Request",
            json!({"error":"invalid_body"}),
        );
        return Ok(None);
    }
    let identity = method == Some("GET")
        && path.is_some_and(|path| path.starts_with("/control/v1/identity?nonce="));
    if !identity
        && !authorization.is_some_and(|value| {
            value
                .strip_prefix("Bearer ")
                .is_some_and(|bearer| constant_time_equal(bearer.as_bytes(), token.as_bytes()))
        })
    {
        response(
            &mut stream,
            401,
            "Unauthorized",
            json!({"error":"unauthorized"}),
        );
        return Ok(None);
    }
    if identity {
        let nonce = path
            .unwrap()
            .strip_prefix("/control/v1/identity?nonce=")
            .unwrap();
        if nonce.len() != 32
            || !nonce
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || content_length != 0
        {
            response(
                &mut stream,
                400,
                "Bad Request",
                json!({"error":"invalid_nonce"}),
            );
        } else {
            let keep_alive = allow_keep_alive
                && connection.is_some_and(|value| value.eq_ignore_ascii_case("keep-alive"));
            response_with_connection(
                &mut stream,
                200,
                "OK",
                json!({
                    "version":1,"service":"inkscope-control","instance_id":instance_id,
                    "host":"127.0.0.1","port":port,
                    "nonce":nonce,"proof":proof(token,nonce,instance_id,port)
                }),
                keep_alive,
            );
            if keep_alive {
                return Ok(Some(stream));
            }
        }
        return Ok(None);
    }
    let method = method.unwrap_or("").to_owned();
    let path = path.unwrap_or("").to_owned();
    while size < header_end + content_length {
        if Instant::now() >= deadline {
            response(
                &mut stream,
                400,
                "Bad Request",
                json!({"error":"invalid_body"}),
            );
            return Ok(None);
        }
        match stream.read(&mut wire[size..header_end + content_length]) {
            Ok(0) => return Ok(None),
            Ok(count) => size += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Ok(None),
        }
    }
    if method == "GET" && path == "/control/v1/status" && content_length == 0 {
        let state = *lock_state(state);
        response(
            &mut stream,
            200,
            "OK",
            json!({
                "version":1,"service":"inkscope-control",
                "capture":if state.running { "running" } else { "starting" },
                "key_state":state.key_state(),"key_epoch":state.key_epoch,
                "key_locked":state.key_locked,"key_source":state.key_source,
                "publisher":{"enabled":publisher.is_some(),"connection":publisher.map_or("disabled", ConnectionState::label)}
            }),
        );
    } else if method == "POST" && path == "/control/v1/connect" {
        let body: ConnectBody =
            match serde_json::from_slice(&wire[header_end..header_end + content_length]) {
                Ok(value) => value,
                Err(_) => {
                    response(
                        &mut stream,
                        400,
                        "Bad Request",
                        json!({"error":"invalid_body"}),
                    );
                    return Ok(None);
                },
            };
        let game_key = match crypto::read_game_key_hex(&body.game_key) {
            Ok(value) => value,
            Err(_) => {
                response(
                    &mut stream,
                    400,
                    "Bad Request",
                    json!({"error":"invalid_game_key"}),
                );
                return Ok(None);
            },
        };
        enqueue(&mut stream, sender, state, Some(game_key));
    } else if method == "POST" && path == "/control/v1/disconnect" && content_length == 0 {
        enqueue(&mut stream, sender, state, None);
    } else {
        response(&mut stream, 404, "Not Found", json!({"error":"not_found"}));
    }
    Ok(None)
}

fn enqueue(
    stream: &mut TcpStream,
    sender: &SyncSender<Command>,
    state: &Mutex<State>,
    key: Option<GameKey>,
) {
    enum Outcome {
        Accepted(u64),
        NotRunning,
        Locked,
        Busy,
    }
    let outcome = {
        let mut current = lock_state(state);
        if !current.running {
            Outcome::NotRunning
        } else if current.key_locked {
            Outcome::Locked
        } else {
            let epoch = current.key_epoch.saturating_add(1);
            let command = match key {
                Some(key) => Command::Connect(key, epoch),
                None => Command::Disconnect(epoch),
            };
            match sender.try_send(command) {
                Ok(()) => {
                    current.key_epoch = epoch;
                    Outcome::Accepted(epoch)
                },
                Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => Outcome::Busy,
            }
        }
    };
    match outcome {
        Outcome::Accepted(epoch) => {
            response(
                stream,
                202,
                "Accepted",
                json!({"version":1,"service":"inkscope-control","key_state":"pending","key_epoch":epoch}),
            );
        },
        Outcome::NotRunning => response(
            stream,
            409,
            "Conflict",
            json!({"error":"capture_not_running"}),
        ),
        Outcome::Locked => response(stream, 409, "Conflict", json!({"error":"key_locked"})),
        Outcome::Busy => {
            response(
                stream,
                503,
                "Service Unavailable",
                json!({"error":"control_busy"}),
            );
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkscope::pcap::UdpPacket;
    use std::net::TcpListener;

    fn free_port() -> u16 {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn request(
        port: u16,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: &str,
        extras: &str,
    ) -> (u16, serde_json::Value, String) {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let auth = token.map_or(String::new(), |token| {
            format!("Authorization: Bearer {token}\r\n")
        });
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}{extras}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let code = response.split_whitespace().nth(1).unwrap().parse().unwrap();
        let json = serde_json::from_str(response.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        (code, json, response)
    }

    fn read_framed_response(stream: &mut TcpStream) -> (String, serde_json::Value) {
        let mut header = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            stream.read_exact(&mut byte).unwrap_or_else(|error| {
                panic!(
                    "response header ended after {} bytes: {error}",
                    header.len()
                )
            });
            header.push(byte[0]);
            assert!(header.len() <= MAX_HEADERS);
            if header.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let header = String::from_utf8(header).unwrap();
        let length: usize = header
            .split("\r\n")
            .find_map(|line| {
                line.strip_prefix("Content-Length: ")
                    .and_then(|value| value.parse().ok())
            })
            .unwrap();
        let mut body = vec![0u8; length];
        stream.read_exact(&mut body).unwrap();
        (header, serde_json::from_slice(&body).unwrap())
    }

    fn browse_reply() -> UdpPacket {
        let mut payload = vec![0u8; 620];
        payload[0] = 1;
        payload[544..548].copy_from_slice(&[192, 0, 2, 1]);
        payload[560..562].copy_from_slice(&49152u16.to_be_bytes());
        UdpPacket {
            record: 1,
            timestamp_ns: 0,
            source: Ipv4Addr::new(192, 0, 2, 1),
            destination: Ipv4Addr::new(192, 0, 2, 2),
            source_port: 35000,
            destination_port: 35000,
            source_mac: [1; 6],
            destination_mac: [2; 6],
            payload,
        }
    }

    #[test]
    fn identity_proof_has_stable_port_bound_wire_format() {
        assert_eq!(
            proof(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "0123456789abcdef0123456789abcdef",
                "00000000-0000-4000-8000-000000000000",
                10253,
            ),
            "b8286fe2b21cb7572e5ad97e839639f59f863ae2ef9e1dd509aeb0c59936fab1"
        );
    }

    #[test]
    fn control_token_requires_32_printable_ascii_characters() {
        for token in [
            "short".to_owned(),
            format!("{} ", "a".repeat(32)),
            format!("{}\n", "a".repeat(32)),
            format!("{}é", "a".repeat(32)),
        ] {
            assert!(Control::start_with_token(10253, None, None, token).is_err());
        }
    }

    #[test]
    fn signed_identity_authentication_and_key_epochs() {
        let port = free_port();
        let control = Control::start_with_token(
            port,
            None,
            None,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        )
        .unwrap();
        control.set_running(true);
        let nonce = "0123456789abcdef0123456789abcdef";
        let (code, identity, _) = request(
            port,
            "GET",
            &format!("/control/v1/identity?nonce={nonce}"),
            None,
            "",
            "",
        );
        assert_eq!(code, 200);
        assert_eq!(identity["service"], "inkscope-control");
        assert_eq!(identity["nonce"], nonce);
        assert_eq!(identity["host"], "127.0.0.1");
        assert_eq!(identity["port"], port);
        let id = identity["instance_id"].as_str().unwrap();
        assert!(Uuid::parse_str(id).is_ok());
        assert_eq!(
            identity["proof"],
            proof(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                nonce,
                id,
                port
            )
        );
        assert_ne!(
            identity["proof"],
            proof(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                nonce,
                id,
                port.wrapping_add(1)
            )
        );
        assert_eq!(
            request(port, "GET", "/control/v1/status", None, "", "").0,
            401
        );
        assert_eq!(
            request(port, "GET", "/control/v1/status", Some("wrong"), "", "").0,
            401
        );
        assert_eq!(
            request(
                port,
                "GET",
                "/control/v1/status",
                Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
                "",
                "Origin: http://localhost:10252\r\n"
            )
            .0,
            403
        );
        let (code, status, _) = request(
            port,
            "GET",
            "/control/v1/status",
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
            "",
            "",
        );
        assert_eq!(code, 200);
        assert_eq!(status["key_state"], "absent");
        assert_eq!(status["key_locked"], false);
        assert_eq!(status["key_source"], "none");
        assert_eq!(status["publisher"]["connection"], "disabled");

        let key = "00112233445566778899aabbccddeeff";
        let invalid = json!({"game_key":"invalid"}).to_string();
        assert_eq!(
            request(
                port,
                "POST",
                "/control/v1/connect",
                Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
                &invalid,
                ""
            )
            .0,
            400
        );
        let body = json!({"game_key":key}).to_string();
        let (code, queued, raw) = request(
            port,
            "POST",
            "/control/v1/connect",
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
            &body,
            "",
        );
        assert_eq!(code, 202);
        assert_eq!(queued["key_state"], "pending");
        assert_eq!(queued["key_epoch"], 1);
        assert!(!raw.contains(key));
        assert_eq!(
            request(
                port,
                "GET",
                "/control/v1/status",
                Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
                "",
                ""
            )
            .1["key_state"],
            "pending"
        );
        let mut decoder = Decoder::new(None, None, None);
        assert_eq!(
            decoder.process(&browse_reply(), false)[0].status,
            "game_key_missing"
        );
        control.apply_pending(&mut decoder);
        assert_ne!(
            decoder.process(&browse_reply(), false)[0].status,
            "game_key_missing"
        );
        let status = request(
            port,
            "GET",
            "/control/v1/status",
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
            "",
            "",
        )
        .1;
        assert_eq!(status["key_state"], "applied");
        assert_eq!(status["key_source"], "control");
        assert_eq!(status["key_epoch"], 1);
        let (code, queued, _) = request(
            port,
            "POST",
            "/control/v1/disconnect",
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
            "",
            "",
        );
        assert_eq!(code, 202);
        assert_eq!(queued["key_epoch"], 2);
        control.apply_pending(&mut decoder);
        assert_eq!(
            decoder.process(&browse_reply(), false)[0].status,
            "game_key_missing"
        );
        let status = request(
            port,
            "GET",
            "/control/v1/status",
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
            "",
            "",
        )
        .1;
        assert_eq!(status["key_state"], "absent");
        assert_eq!(status["key_source"], "none");
        assert_eq!(status["key_epoch"], 2);
        control.finish();
    }

    #[test]
    fn identity_and_key_mutation_share_one_bounded_tcp_connection() {
        let port = free_port();
        let token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let control = Control::start_with_token(port, None, None, token.into()).unwrap();
        control.set_running(true);
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let local_address = stream.local_addr().unwrap();
        let nonce = "0123456789abcdef0123456789abcdef";
        write!(stream, "GET /control/v1/identity?nonce={nonce} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: keep-alive\r\n\r\n").unwrap();
        let (header, identity) = read_framed_response(&mut stream);
        assert!(header.starts_with("HTTP/1.1 200"));
        assert!(header.contains("Connection: keep-alive\r\n"));
        assert_eq!(identity["port"], port);
        assert_eq!(
            identity["proof"],
            proof(
                token,
                nonce,
                identity["instance_id"].as_str().unwrap(),
                port
            )
        );
        assert_eq!(stream.local_addr().unwrap(), local_address);

        let body = json!({"game_key":"00112233445566778899aabbccddeeff"}).to_string();
        write!(stream, "POST /control/v1/connect HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}", body.len()).unwrap();
        let (header, accepted) = read_framed_response(&mut stream);
        assert!(header.starts_with("HTTP/1.1 202"));
        assert!(header.contains("Connection: close\r\n"));
        assert_eq!(accepted["key_state"], "pending");
        let mut eof = [0u8; 1];
        assert_eq!(stream.read(&mut eof).unwrap(), 0);
        let mut decoder = Decoder::new(None, None, None);
        control.apply_pending(&mut decoder);
        assert_ne!(
            decoder.process(&browse_reply(), false)[0].status,
            "game_key_missing"
        );
        control.finish();
    }

    #[test]
    fn stalled_local_client_does_not_block_other_control_requests_or_shutdown() {
        let port = free_port();
        let token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let control = Control::start_with_token(port, None, None, token.into()).unwrap();
        control.set_running(true);
        let mut stalled = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stalled.write_all(b"G").unwrap();
        let mut drip_stream = stalled.try_clone().unwrap();
        let drip = thread::spawn(move || {
            for byte in b"ET /" {
                thread::sleep(Duration::from_millis(400));
                if drip_stream.write_all(&[*byte]).is_err() {
                    break;
                }
            }
        });
        thread::sleep(Duration::from_millis(50));

        let start = Instant::now();
        let (code, identity, _) = request(
            port,
            "GET",
            "/control/v1/identity?nonce=0123456789abcdef0123456789abcdef",
            None,
            "",
            "",
        );
        assert_eq!(code, 200);
        assert_eq!(identity["service"], "inkscope-control");
        assert_eq!(
            request(port, "GET", "/control/v1/status", Some(token), "", "").0,
            200
        );
        assert!(start.elapsed() < Duration::from_millis(1500));

        drip.join().unwrap();
        drop(stalled);
        let mut idle_at_shutdown = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        idle_at_shutdown.write_all(b"G").unwrap();
        thread::sleep(Duration::from_millis(50));

        let shutdown_start = Instant::now();
        control.finish();
        assert!(shutdown_start.elapsed() < Duration::from_secs(2));
        drop(idle_at_shutdown);
    }

    #[test]
    fn refuses_oversize_and_foreign_host_without_queuing_key() {
        let port = free_port();
        let control = Control::start_with_token(
            port,
            None,
            None,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        )
        .unwrap();
        control.set_running(true);
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(stream, "POST /control/v1/connect HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\r\nContent-Length: 513\r\n\r\n").unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        assert!(raw.starts_with("HTTP/1.1 413"));
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(stream, "GET /control/v1/status HTTP/1.1\r\nHost: attacker.test\r\nAuthorization: Bearer 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\r\n\r\n").unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        assert!(raw.starts_with("HTTP/1.1 403"));
        assert_eq!(
            request(
                port,
                "GET",
                "/control/v1/status",
                Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
                "",
                ""
            )
            .1["key_epoch"],
            0
        );
        control.finish();
    }

    #[test]
    fn startup_key_is_reported_without_secret_and_cannot_be_replaced() {
        let port = free_port();
        let control = Control::start_with_token(
            port,
            Some("file"),
            None,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        )
        .unwrap();
        control.set_running(true);
        let (_, status, raw) = request(
            port,
            "GET",
            "/control/v1/status",
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
            "",
            "",
        );
        assert_eq!(status["key_locked"], true);
        assert_eq!(status["key_source"], "file");
        assert_eq!(status["key_state"], "applied");
        assert!(!raw.contains("/run/secrets"));
        let body = json!({"game_key":"00112233445566778899aabbccddeeff"}).to_string();
        let (code, error, raw) = request(
            port,
            "POST",
            "/control/v1/connect",
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
            &body,
            "",
        );
        assert_eq!(code, 409);
        assert_eq!(error["error"], "key_locked");
        assert!(!raw.contains("00112233445566778899aabbccddeeff"));
        assert_eq!(
            request(
                port,
                "POST",
                "/control/v1/disconnect",
                Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
                "",
                ""
            )
            .0,
            409
        );
        let status = request(
            port,
            "GET",
            "/control/v1/status",
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
            "",
            "",
        )
        .1;
        assert_eq!(status["key_epoch"], 0);
        assert_eq!(status["key_state"], "applied");
        control.finish();
    }
}
