//! Stateful LAN discovery, authenticated Pia decoding, and conservative field labels.
use crate::Result;
use crate::crypto::{self, GameKey, PiaKey};
use crate::equipment::EquipmentSetting;
use crate::pcap::UdpPacket;
use crate::preview::WavePreview;
use serde::Serialize;
use std::collections::HashMap;
use std::net::Ipv4Addr;

#[derive(Clone, Serialize)]
pub struct Field {
    pub name: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bits: Option<[usize; 2]>,
}

#[derive(Serialize)]
pub struct Row {
    pub record: u64,
    pub timestamp_ns: u64,
    pub source: String,
    pub destination: String,
    pub source_mac: String,
    pub destination_mac: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    pub kind: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk: Option<usize>,
    pub fields: Vec<Field>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authenticated_payload_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wave_preview: Option<WavePreview>,
}

fn field(name: &str, value: impl ToString, bits: Option<[usize; 2]>) -> Field {
    Field {
        name: name.into(),
        value: value.to_string(),
        bits,
    }
}

fn mac(bytes: &[u8; 6]) -> String {
    bytes
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn row(packet: &UdpPacket, kind: &str, status: &str) -> Row {
    Row {
        record: packet.record,
        timestamp_ns: packet.timestamp_ns,
        source: format!("{}:{}", packet.source, packet.source_port),
        destination: format!("{}:{}", packet.destination, packet.destination_port),
        source_mac: mac(&packet.source_mac),
        destination_mac: mac(&packet.destination_mac),
        direction: None,
        kind: kind.into(),
        status: status.into(),
        layout: None,
        protocol: None,
        port: None,
        chunk: None,
        fields: vec![],
        authenticated_payload_hex: None,
        wave_preview: None,
    }
}

struct Request {
    client: Ipv4Addr,
    broadcast: Ipv4Addr,
    wire: Vec<u8>,
    plaintext: Vec<u8>,
}
struct Candidate {
    key: PiaKey,
    reply_record: u64,
    verified: bool,
}

const MAX_CANDIDATE_HOSTS: usize = 16;
const RTT_PENDING_LIMIT: usize = 256;
const RTT_PENDING_NS: u64 = 2_000_000_000;

fn push_candidate(candidates: &mut Vec<Candidate>, candidate: Candidate) {
    if let Some(existing) = candidates
        .iter_mut()
        .find(|value| value.key.0 == candidate.key.0)
    {
        existing.reply_record = candidate.reply_record;
        existing.verified |= candidate.verified;
        return;
    }
    let verified = candidate.verified;
    candidates.push(candidate);
    // An unauthenticated reply cannot evict proof-verified keys. Keep a
    // bounded fallback tier for captures that begin after BrowseRequest.
    if candidates
        .iter()
        .filter(|value| value.verified == verified)
        .count()
        > 8
        && let Some(oldest) = candidates
            .iter()
            .position(|value| value.verified == verified)
    {
        candidates.remove(oldest);
    }
}

pub struct Decoder {
    game_key: Option<GameKey>,
    manual_pia: Option<(Ipv4Addr, PiaKey)>,
    broadcast: Option<Ipv4Addr>,
    requests: Vec<Request>,
    candidates: HashMap<Ipv4Addr, Vec<Candidate>>,
    rtt_pending: HashMap<(Ipv4Addr, Ipv4Addr, u64), u64>,
}

impl Decoder {
    pub fn new(
        game_key: Option<GameKey>,
        manual_pia: Option<(Ipv4Addr, PiaKey)>,
        broadcast: Option<Ipv4Addr>,
    ) -> Self {
        Self {
            game_key,
            manual_pia,
            broadcast,
            requests: vec![],
            candidates: HashMap::new(),
            rtt_pending: HashMap::new(),
        }
    }

    /// A control-plane key change invalidates proof and derived keys from the
    /// previous GameKey. An explicitly supplied PiaKey remains independent.
    pub fn replace_game_key(&mut self, game_key: Option<GameKey>) {
        self.game_key = game_key;
        self.requests.clear();
        self.candidates.clear();
        self.rtt_pending.clear();
    }

    /// Check only the explicitly supplied PiaKey for packets involving its
    /// requested peer. `None` excludes unrelated or malformed packet layouts;
    /// it must not be counted as evidence that a cached key is stale.
    pub fn manual_pia_authentication(&self, packet: &UdpPacket) -> Option<bool> {
        let (peer, key) = self.manual_pia.as_ref()?;
        if packet.source_port != 49152 && packet.destination_port != 49152 {
            return None;
        }
        if packet.source != *peer && packet.destination != *peer {
            return None;
        }
        crypto::validate_pia_header(&packet.payload).ok()?;
        match crypto::decrypt_pia(key, &packet.payload, packet.source) {
            Ok(_) => Some(true),
            Err(error) if error == "Pia authentication failed" => Some(false),
            Err(_) => None,
        }
    }

    pub fn discard_manual_pia(&mut self) {
        self.manual_pia = None;
    }

    pub fn replace_manual_pia(&mut self, peer: Ipv4Addr, key: PiaKey) {
        self.manual_pia = Some((peer, key));
    }

    pub fn manual_pia_key(&self, peer: Ipv4Addr) -> Option<&PiaKey> {
        let (configured, key) = self.manual_pia.as_ref()?;
        (*configured == peer).then_some(key)
    }

    /// The latest key tied to a proof-verified BrowseReply for this host.
    pub fn verified_pia_key(&self, peer: Ipv4Addr) -> Option<&PiaKey> {
        self.candidates
            .get(&peer)?
            .iter()
            .rev()
            .find(|candidate| candidate.verified)
            .map(|candidate| &candidate.key)
    }

    /// An automatically derived candidate that authenticates this exact Pia
    /// packet. A BrowseReply without a verified request can still yield a
    /// usable key once GCM authentication succeeds.
    pub fn authenticated_candidate_key(
        &self,
        packet: &UdpPacket,
        peer: Ipv4Addr,
    ) -> Option<[u8; 16]> {
        if packet.source != peer && packet.destination != peer {
            return None;
        }
        crypto::validate_pia_header(&packet.payload).ok()?;
        let manual = self.manual_pia_key(peer);
        for key in self.keys_for(packet) {
            if manual.is_some_and(|manual| std::ptr::eq(manual, key)) {
                continue;
            }
            if crypto::decrypt_pia(key, &packet.payload, packet.source).is_ok() {
                return Some(key.0);
            }
        }
        None
    }

    /// Capture-time request-to-response interval, including peer processing and capture effects.
    /// This is separate from the sender's ElapsedMicroseconds and is not network-only RTT.
    fn rtt_capture_delay(&mut self, packet: &UdpPacket, chunk: &Chunk) -> Option<String> {
        let p = &chunk.payload;
        if chunk.protocol != 3 || p.len() != 21 || !matches!(p[0], 0 | 1) {
            return None;
        }
        self.rtt_pending.retain(|_, timestamp| {
            packet.timestamp_ns.saturating_sub(*timestamp) <= RTT_PENDING_NS
        });
        let origin = u64::from_be_bytes(p[1..9].try_into().ok()?);
        if p[0] == 0 {
            self.rtt_pending.insert(
                (packet.source, packet.destination, origin),
                packet.timestamp_ns,
            );
            if self.rtt_pending.len() > RTT_PENDING_LIMIT
                && let Some(oldest) = self
                    .rtt_pending
                    .iter()
                    .min_by_key(|(_, timestamp)| *timestamp)
                    .map(|(key, _)| *key)
            {
                self.rtt_pending.remove(&oldest);
            }
            return None;
        }
        let key = (packet.destination, packet.source, origin);
        let timestamp = *self.rtt_pending.get(&key)?;
        let elapsed = packet.timestamp_ns.checked_sub(timestamp)?;
        if elapsed == 0 || elapsed > RTT_PENDING_NS {
            return None;
        }
        self.rtt_pending.remove(&key);
        Some(format!("{:.3}", elapsed as f64 / 1_000_000.0))
    }

    pub fn process(&mut self, packet: &UdpPacket, details: bool) -> Vec<Row> {
        if packet.source_port == 35000 || packet.destination_port == 35000 {
            return vec![self.browse(packet)];
        }
        if packet.source_port == 49152 || packet.destination_port == 49152 {
            return self.pia(packet, details);
        }
        vec![]
    }

    fn add_candidate(&mut self, host: Ipv4Addr, candidate: Candidate) {
        if !self.candidates.contains_key(&host) && self.candidates.len() >= MAX_CANDIDATE_HOSTS {
            let oldest = self
                .candidates
                .iter()
                .filter(|(_, values)| values.iter().all(|value| !value.verified))
                .min_by_key(|(_, values)| values.last().map_or(0, |value| value.reply_record))
                .or_else(|| {
                    self.candidates.iter().min_by_key(|(_, values)| {
                        values.last().map_or(0, |value| value.reply_record)
                    })
                })
                .map(|(peer, _)| *peer);
            if let Some(oldest) = oldest {
                self.candidates.remove(&oldest);
            }
        }
        push_candidate(self.candidates.entry(host).or_default(), candidate);
    }

    fn keys_for(&self, packet: &UdpPacket) -> Vec<&PiaKey> {
        let peers = if packet.source == packet.destination {
            vec![packet.source]
        } else {
            vec![packet.source, packet.destination]
        };
        let mut others = self
            .candidates
            .keys()
            .filter(|host| !peers.contains(host))
            .copied()
            .collect::<Vec<_>>();
        others.sort();
        let mut keys = vec![];
        if let Some((peer, key)) = &self.manual_pia
            && peers.contains(peer)
        {
            keys.push(key);
        }
        for verified in [true, false] {
            for host in peers.iter().chain(others.iter()) {
                if let Some(candidates) = self.candidates.get(host) {
                    keys.extend(
                        candidates
                            .iter()
                            .rev()
                            .filter(|c| c.verified == verified && c.reply_record < packet.record)
                            .map(|c| &c.key),
                    );
                }
            }
        }
        if let Some((peer, key)) = &self.manual_pia
            && !peers.contains(peer)
        {
            keys.push(key);
        }
        keys
    }

    fn browse(&mut self, packet: &UdpPacket) -> Row {
        let wire = &packet.payload;
        if wire.is_empty() {
            return row(packet, "Browse", "malformed");
        }
        match wire[0] {
            0 if wire.len() == 325 => {
                let mut output = row(packet, "BrowseRequest", "observed");
                output.fields.push(field(
                    "HeaderLength",
                    u32::from_be_bytes(wire[1..5].try_into().unwrap()),
                    Some([8, 40]),
                ));
                output.fields.push(field(
                    "TitleId",
                    format!("0x{}", hex::encode(&wire[19..27])),
                    Some([152, 216]),
                ));
                let broadcast = self.broadcast.or_else(|| {
                    if packet.destination.is_broadcast() || packet.destination.octets()[3] == 255 {
                        Some(packet.destination)
                    } else {
                        None
                    }
                });
                if let (Some(game), Some(ip)) = (&self.game_key, broadcast) {
                    match crypto::decrypt_browse_request(game, wire, ip) {
                        Ok(plaintext) => {
                            output.status = "authenticated".into();
                            self.requests.push(Request {
                                client: packet.source,
                                broadcast: ip,
                                wire: wire.clone(),
                                plaintext,
                            });
                            if self.requests.len() > 32 {
                                self.requests.remove(0);
                            }
                        },
                        Err(_) => output.status = "authentication_failed".into(),
                    }
                } else {
                    output.status = "key_or_broadcast_missing".into();
                }
                output
            },
            1 if wire.len() == 620 => {
                let mut output = row(packet, "BrowseReply", "observed_unverified");
                let host = Ipv4Addr::new(wire[544], wire[545], wire[546], wire[547]);
                let port = u16::from_be_bytes([wire[560], wire[561]]);
                output
                    .fields
                    .push(field("HostIPv4", host, Some([544 * 8, 548 * 8])));
                output
                    .fields
                    .push(field("PiaPort", port, Some([560 * 8, 562 * 8])));
                if host != packet.source || port != 49152 {
                    output.status = "endpoint_mismatch".into();
                    return output;
                }
                if let Some(game) = &self.game_key {
                    let matching_requests = self
                        .requests
                        .iter()
                        .rev()
                        .filter(|r| r.client == packet.destination);
                    let mut attempted = false;
                    for request in matching_requests {
                        attempted = true;
                        if crypto::verify_browse_reply(
                            game,
                            &request.wire,
                            &request.plaintext,
                            wire,
                            request.broadcast,
                        ) == Ok(true)
                        {
                            output.status = "proof_verified".into();
                            break;
                        }
                    }
                    if attempted && output.status != "proof_verified" {
                        output.status = "proof_failed".into();
                    }
                    if output.status != "proof_failed"
                        && let Ok(key) = crypto::derive_pia_key(game, &wire[105..137])
                    {
                        self.add_candidate(
                            host,
                            Candidate {
                                key,
                                reply_record: packet.record,
                                verified: output.status == "proof_verified",
                            },
                        );
                    }
                } else {
                    output.status = "game_key_missing".into();
                }
                output
            },
            _ => row(packet, "Browse", "unsupported_layout"),
        }
    }

    fn pia(&mut self, packet: &UdpPacket, details: bool) -> Vec<Row> {
        let wire = &packet.payload;
        if crypto::validate_pia_header(wire).is_err() {
            return vec![row(packet, "Pia", "unsupported_layout")];
        }
        let keys = self.keys_for(packet);
        if keys.is_empty() {
            return vec![row(packet, "Pia", "key_missing")];
        }
        let mut authenticated = None;
        let mut specific_error = None;
        for key in keys {
            match crypto::decrypt_pia(key, wire, packet.source) {
                Ok(value) => {
                    authenticated = Some(value);
                    break;
                },
                Err(error) if error != "Pia authentication failed" => specific_error = Some(error),
                Err(_) => {},
            }
        }
        let Some(pia) = authenticated else {
            let mut output = row(
                packet,
                "Pia",
                if specific_error.is_some() {
                    "invalid_authenticated_frame"
                } else {
                    "authentication_failed"
                },
            );
            if let Some(error) = specific_error {
                output.fields.push(field("Error", error, None));
            }
            return vec![output];
        };
        match chunks(&pia.body) {
            Ok(chunks) => {
                let mut rows = Vec::with_capacity(chunks.len());
                for (index, chunk) in chunks.into_iter().enumerate() {
                    let mut output = row(packet, &describe_chunk(&chunk), "authenticated");
                    output.protocol = Some(chunk.protocol);
                    output.port = chunk.port;
                    output.chunk = Some(index);
                    output
                        .fields
                        .push(field("HeaderPacketId", pia.packet_id, None));
                    output
                        .fields
                        .push(field("HeaderCompressedFlag", pia.flags & 1 != 0, None));
                    let semantic = semantic_fields(&chunk);
                    output.layout = Some(
                        if semantic.iter().any(|field| {
                            matches!(
                                (chunk.protocol, field.name.as_str()),
                                (11, "ProfileScope" | "StationRosterScope")
                                    | (13, "SessionRosterScope")
                            )
                        }) {
                            "type_fields"
                        } else if semantic.len() > 2 {
                            if chunk.protocol == 8
                                && event_type(&chunk.payload).is_some_and(|id| {
                                    event_body_bits(id, &chunk.payload, chunk.port).is_none()
                                })
                            {
                                "type_fields"
                            } else {
                                "confirmed_fields"
                            }
                        } else if chunk.protocol == 0 {
                            "empty"
                        } else {
                            "type_only"
                        }
                        .into(),
                    );
                    output.fields.extend(semantic);
                    if let Some(delay) = self.rtt_capture_delay(packet, &chunk) {
                        output
                            .fields
                            .push(field("CaptureResponseDelayMs", delay, None));
                    }
                    if details {
                        output
                            .fields
                            .push(field("HeaderDestinationId", pia.destination_id, None));
                        output
                            .fields
                            .push(field("HeaderSourceId", pia.source_id, None));
                    }
                    if details {
                        output.authenticated_payload_hex = Some(hex::encode(&chunk.payload));
                    }
                    rows.push(output);
                }
                rows
            },
            Err(error) => {
                let mut output = row(packet, "Pia", "authenticated_malformed_chunks");
                output.fields.push(field("Error", error, None));
                vec![output]
            },
        }
    }
}

#[derive(Debug)]
pub struct Chunk {
    pub protocol: u8,
    pub port: Option<u8>,
    pub payload: Vec<u8>,
}

pub fn chunks(body: &[u8]) -> Result<Vec<Chunk>> {
    if body.is_empty() || body.len() > 0x2263 {
        return Err("Pia plaintext bounds invalid".into());
    }
    let mut cursor = 0;
    let mut length = None;
    let mut protocol = None;
    let mut port = None;
    let mut out = vec![];
    while cursor < body.len() {
        if out.len() == 32 {
            return Err("Pia chunk count exceeded".into());
        }
        let flags = body[cursor];
        cursor += 1;
        if flags & !0x1f != 0 {
            return Err("Pia chunk header flags unsupported".into());
        }
        let mut take = |count: usize| -> Result<&[u8]> {
            let value = body
                .get(cursor..cursor + count)
                .ok_or("Pia chunk header truncated")?;
            cursor += count;
            Ok(value)
        };
        if flags & 1 != 0 {
            take(1)?;
        }
        if flags & 2 != 0 {
            let bytes = take(2)?;
            length = Some(u16::from_be_bytes(bytes.try_into().unwrap()) as usize);
        }
        if flags & 4 != 0 {
            protocol = Some(take(1)?[0]);
        }
        if flags & 8 != 0 {
            port = Some(take(1)?[0]);
        }
        if flags & 16 != 0 {
            take(1)?;
        }
        let id = protocol.ok_or("Pia inherited protocol unavailable")?;
        // Captured Protocol 0 keepalive is exactly [0x04, 0x00]: its omitted
        // first length denotes an empty body. Other first chunks need a length.
        let len = match length {
            Some(len) => len,
            None if id == 0 => 0,
            None => return Err("Pia inherited chunk length unavailable".into()),
        };
        if len > 0x5a3 {
            return Err("Pia chunk length exceeded".into());
        }
        let payload = body
            .get(cursor..cursor + len)
            .ok_or("Pia chunk body truncated")?
            .to_vec();
        cursor += len;
        out.push(Chunk {
            protocol: id,
            port,
            payload,
        });
    }
    Ok(out)
}

fn protocol_name(id: u8) -> &'static str {
    match id {
        0 => "Empty",
        1 => "NetProtocol",
        2 => "TurnProtocol",
        3 => "RTTProtocol",
        4 => "SyncProtocol",
        5 => "UnreliableProtocol",
        6 => "CloneProtocol",
        7 => "EventProtocol",
        8 => "BroadcastEventProtocol",
        9 => "ClockProtocol",
        10 => "ReliableProtocol",
        11 => "BroadcastReliableProtocol",
        12 => "StreamBroadcastReliableProtocol",
        13 => "SessionProtocol",
        14 => "NatTraversalResultProtocol",
        15 => "MonitoringDataProtocol",
        16 => "WanNatTraversalProtocol",
        17 => "Reckoning1DProtocol",
        18 => "Reckoning3DProtocol",
        _ => "UnknownProtocol",
    }
}

fn event_type(payload: &[u8]) -> Option<u32> {
    // The native wrapper's entry count n is exactly the byte at offset 7.
    // Only n=0 has the fixed header and event offsets decoded below.
    if payload.len() < 25 || payload[0] != 7 || payload[7] != 0 {
        return None;
    }
    // Validate the Protocol 8 envelope before reading its type ID. Control
    // frames can satisfy these length relations too, so an unknown value at
    // this offset is not enough to call the frame a typed event.
    let message_length = u16::from_be_bytes([payload[1], payload[2]]) as usize;
    let event_length = u16::from_be_bytes([payload[13], payload[14]]) as usize;
    if message_length + 8 != payload.len() || event_length + 17 != payload.len() {
        return None;
    }
    let id = u32::from_le_bytes(payload[21..25].try_into().unwrap());
    (event_name(id) != "Unknown").then_some(id)
}

fn event_name(id: u32) -> &'static str {
    match id {
        0xf429516a => "AttackEvent",
        0xaef8a8b7 => "AttackEventMulti8",
        0x6c199c2e => "BulletNetEvent::SpawnWallDrop",
        0x8d0e3437 => "CoopEnemySakeCarrierSpawnSakeCopterEvent",
        0x838fb319 => "CoopEnemySakeDolphinTargetPosEvent",
        0x83af1b69 => "CoopEnemySakeFlyBagmanEndPickupEvent",
        0x4ab76ee0 => "CoopEnemySakeFlyBagmanTargetPosEvent",
        0x7db12439 => "CoopEnemySakeSaucerFallEvent",
        0x15f34197 => "CoopEnemySakeSaucerTargetPosEvent",
        0x0caefc01 => "CoopEnemySpawnNetEvent",
        0x6e7d7386 => "CoopItemSpawnEvent",
        0x5b64bbf1 => "CoopItemResponseGetByPlayerEvent",
        0xedbcbb02 => "CoopItemResponseGetByEnemyEvent",
        0xf010cc79 => "CoopItemGoldenIkuraShotEvent",
        0x3d8d2c59 => "CoopItemGoldenIkuraDirectBankEvent",
        0x9a1e036d => "OnlineCoopSetting",
        0x4c1ecd54 => "CoopEnemyStateEvent",
        0xc7cd3faf => "CoopSeqRoundFinishEvent",
        0x8aa4d7d5 => "CoopItemRequestGetByPlayerEvent",
        0x1a894818 => "CoopItemShootSleepEvent",
        0xb28e7c61 => "GeneratorBind",
        0xa73057e1 => "GeneratorDestroyRequest",
        0x39b6e487 => "GeneratorWaveSync",
        0x85b99621 => "HitEffectNetEvent",
        0xba206257 => "PlayerNetEvent::Attack",
        0xd63f3522 => "PlayerNetEvent::BulletClam",
        0xbc4e7627 => "PlayerNetEvent::BulletShooter",
        0x89c53d2e => "PlayerNetEvent::BulletSpShockSonar",
        0x079c2378 => "PlayerNetEvent::BulletSpinner",
        0xcf9be194 => "PlayerNetEvent::CoopRescued",
        0x4c4d2b61 => "PlayerNetEvent::CoopRoundSetup",
        0xf4d6f09e => "PlayerNetEvent::DirectBankClam",
        0x13c90357 => "PlayerNetEvent::DokanWarp_End",
        0x9f9b5f27 => "PlayerNetEvent::DokanWarp_Start",
        0x25bee8f2 => "PlayerNetEvent::PerformSpecial",
        0x74400e72 => "PlayerNetEvent::Revival",
        0x89e91844 => "PlayerNetEvent::TroubleDie",
        0x4939c1ac => "ColPaintEvent",
        0xe3f0eb9a => "ObjPaintEvent",
        _ => "Unknown",
    }
}

fn describe_chunk(chunk: &Chunk) -> String {
    let base = protocol_name(chunk.protocol);
    match chunk.protocol {
        1 if chunk.payload.len() >= 2 && chunk.payload[0] == 1 => {
            format!("Net 0x{:02x}", chunk.payload[1])
        },
        13 if !chunk.payload.is_empty() => format!("Session {}", chunk.payload[0]),
        5 => match coop_state_group(&chunk.payload) {
            Some(1104) => "CoopSequenceNetState".into(),
            Some(1107 | 1108) => "CoopSeqPlayerNetState".into(),
            _ => base.into(),
        },
        8 => event_type(&chunk.payload).map_or(base, event_name).into(),
        11 if private_setting(chunk).is_some() => "PrivateSetting".into(),
        _ => base.into(),
    }
}

fn bits(bytes: &[u8], start: usize, width: usize) -> Option<u64> {
    if width > 64 || start.checked_add(width)? > bytes.len() * 8 {
        return None;
    }
    let mut value = 0;
    for bit in 0..width {
        value |= (((bytes[(start + bit) / 8] >> ((start + bit) % 8)) & 1) as u64) << bit;
    }
    Some(value)
}

struct PrivateSetting {
    stage_id: Option<String>,
    difficulty_wire: Option<(u64, usize)>,
}

fn private_stage_id(payload: &[u8]) -> Option<String> {
    let bytes = (0..64)
        .map(|index| bits(payload, 100 + index * 8, 8).map(|value| value as u8))
        .collect::<Option<Vec<_>>>()?;
    let end = bytes.iter().position(|byte| *byte == 0)?;
    if !bytes[..end].iter().all(u8::is_ascii_graphic) || !bytes[end..].iter().all(|byte| *byte == 0)
    {
        return None;
    }
    String::from_utf8(bytes[..end].to_vec()).ok()
}

fn private_setting(chunk: &Chunk) -> Option<PrivateSetting> {
    let p = &chunk.payload;
    if chunk.protocol != 11
        || chunk.port != Some(100)
        || !matches!(p.len(), 78 | 161 | 226)
        || p[0] != 7
        || usize::from(u16::from_be_bytes([p[1], p[2]])) != p.len() - 8
    {
        return None;
    }
    match p.len() {
        78 if bits(p, 97, 3) == Some(4)
            && bits(p, 612, 2) == Some(1)
            && bits(p, 614, 1) == Some(0)
            && bits(p, 615, 3) == Some(0)
            && bits(p, 618, 6) == Some(0) =>
        {
            Some(PrivateSetting {
                stage_id: Some(private_stage_id(p)?),
                difficulty_wire: None,
            })
        },
        161 if bits(p, 97, 5) == Some(16) && bits(p, 1287, 1) == Some(0) => Some(PrivateSetting {
            stage_id: None,
            difficulty_wire: Some((bits(p, 102, 11)?, 102)),
        }),
        // NXCTF docs/security/2026-10-25-s3-salmon-settings-and-gameplay-static-evidence.md:
        // Common spans bits 100..615; presence bits 615/616 precede Coop at
        // 617. Coop's first 11 bits encode the internal value plus one.
        226 if bits(p, 97, 3) == Some(4)
            && bits(p, 612, 2) == Some(1)
            && bits(p, 614, 1) == Some(0)
            && bits(p, 615, 2) == Some(2)
            && bits(p, 1802, 1) == Some(0)
            && bits(p, 1803, 5) == Some(0) =>
        {
            Some(PrivateSetting {
                stage_id: Some(private_stage_id(p)?),
                difficulty_wire: Some((bits(p, 617, 11)?, 617)),
            })
        },
        _ => None,
    }
}

fn coop_state_group(payload: &[u8]) -> Option<u64> {
    if payload.len().checked_mul(8)? < 76 || bits(payload, 0, 1) != Some(1) {
        return None;
    }
    let group = bits(payload, 13, 32)?;
    matches!(group, 1104 | 1107 | 1108).then_some(group)
}

fn value_field(fields: &mut Vec<Field>, bytes: &[u8], name: &str, start: usize, width: usize) {
    if let Some(value) = bits(bytes, start, width) {
        fields.push(field(name, value, Some([start, start + width])));
    }
}

fn axis(bytes: &[u8], start: usize) -> Option<f64> {
    let raw = bits(bytes, start, 17)? as u32;
    let magnitude = (raw & 0xffff) as f64 * 256.0 / 65535.0;
    Some(if raw & 0x10000 != 0 {
        -magnitude
    } else {
        magnitude
    })
}

fn shot_trig_sample(index: u32) -> (f64, f64) {
    // Regenerates the native table at 0x50c9fe0 without copying asset data.
    // Exact cardinal zeros also preserve the native axis-aligned vectors.
    match index % 256 {
        0 => (0.0, 1.0),
        64 => (1.0, 0.0),
        128 => (0.0, -1.0),
        192 => (-1.0, 0.0),
        _ => (f64::from(index) * std::f64::consts::TAU / 256.0).sin_cos(),
    }
}

fn shot_interpolated_trig(angle: f32, fused: bool) -> (f32, f32) {
    // 0x31cbd94..0x31cbe20 converts a signed angle to a wrapping 8-bit
    // table index and 24-bit fractional part. Pitch uses separate operations;
    // yaw uses native FMADD. Direct sin/cos(angle) is measurably different.
    let fixed = (angle * f32::from_bits(0x4e22_f983)) as i64;
    let index = ((fixed as u64 >> 24) & 255) as u32;
    let fraction = (fixed as u64 & 0x00ff_ffff) as f32;
    let (sin, cos) = shot_trig_sample(index);
    let (next_sin, next_cos) = shot_trig_sample(index + 1);
    let interpolate = |start: f64, end: f64| {
        let delta = (end - start) as f32;
        if fused {
            delta.mul_add(fraction * f32::from_bits(0x3380_0000), start as f32)
        } else {
            start as f32 + (delta * fraction) * f32::from_bits(0x3380_0000)
        }
    };
    (interpolate(sin, next_sin), interpolate(cos, next_cos))
}

fn shot_velocity(pitch_raw: u32, yaw_raw: u32, length_raw: u32) -> [f32; 3] {
    // Native direction decoder 0x31cbcb8: signed pitch/2047 * pi/2,
    // signed yaw/4095 * pi, table interpolation, then normalize once more.
    let signed = |raw: u32, sign: u32, maximum: f32, angle: f32| {
        let magnitude = (raw & (sign - 1)) as f32 / maximum;
        (if raw & sign == 0 {
            magnitude
        } else {
            -magnitude
        }) * angle
    };
    let pitch = signed(pitch_raw, 0x800, 2047.0, std::f32::consts::FRAC_PI_2);
    let yaw = signed(yaw_raw, 0x1000, 4095.0, std::f32::consts::PI);
    let y = shot_interpolated_trig(pitch, false).0;
    let (sin_yaw, cos_yaw) = shot_interpolated_trig(yaw, true);
    let radius = (1.0 - y * y).sqrt();
    let x = radius * sin_yaw;
    let z = radius * cos_yaw;
    let inverse_norm = 1.0 / z.mul_add(z, x.mul_add(x, y * y)).sqrt();
    // Preserve 0x379a6cc..0x379a710's f32 evaluation order. The output is
    // the launch velocity per native frame, not a destination coordinate.
    let inverse_sixty = f32::from_bits(0x3c88_8889);
    let magnitude = (length_raw as f32 / 65535.0 * 2048.0 * inverse_sixty) * 60.0;
    [x, y, z].map(|axis| ((axis * inverse_norm) * magnitude) * inverse_sixty)
}

fn player_state_axes(fields: &mut Vec<Field>, payload: &[u8], start: usize) {
    // These three axes match the owned 11.3.0 PlayerNetState serializer's
    // first fields. Their screen slot and current-world-coordinate meaning
    // are not established by the packet shape alone.
    fields.push(field("PlayerStateCoordinateMeaning", "Unknown", None));
    for (name, offset) in [
        ("PlayerStateX", 0),
        ("PlayerStateY", 17),
        ("PlayerStateZ", 34),
    ] {
        if let Some(value) = axis(payload, start + offset) {
            fields.push(field(
                name,
                format!("{value:.3}"),
                Some([start + offset, start + offset + 17]),
            ));
        }
    }
    // Owned 11.3.0 PlayerNetState +0x68 is an unsigned affine 12-bit
    // normalized camera-pitch control, not a sign/magnitude angle. Encoder
    // 0x27fcb74..0x27fcbdc and decoder 0x27fd7c0..0x27fd804 use 4095;
    // the decoder evaluates in f64, then stores f32. Sender 0x287e4fc/e504
    // copies actor-state +0x584 here; Camera 0x28ee4a4/e4a8 copies the same
    // property to +0x16c, consumed by geometry 0x28f065c -> 0x28eb664.
    if let Some(raw) = bits(payload, start + 172, 12) {
        let control = (2.0 * (raw as f64 / 4095.0) - 1.0) as f32;
        let span = Some([start + 172, start + 184]);
        fields.push(field("PlayerStateCameraPitchRaw", raw, span));
        fields.push(field(
            "PlayerStateCameraPitchControl",
            format!("{control:.9}"),
            span,
        ));
    }
    // +0xf8 is the nested-state selector: decoder 0x27fdb00..0x27fdbd8
    // selects Squid vtable 0x5d2bef8 for 0, Human 0x5d2bdd8 for 1.
    // These names label nested animation-state codecs. CoopZombie animation
    // state 0x94 and entry/warp transitions also select Squid, so this is not
    // an independent current-screen-form flag. Camera +0x17ac is separate.
    if let Some(form) = bits(payload, start + 318, 1) {
        fields.push(field(
            "PlayerStateForm",
            if form == 0 { "Squid" } else { "Human" },
            Some([start + 318, start + 319]),
        ));
    }
    // Owned 11.3.0 PlayerNetState encoder 0x27fcb70 calls 0x31cc060 for
    // object +0x5c: body bits 159..172 carry a horizontal-vector angle.
    // Decoder 0x27fd700..0x27fd7bc reconstructs [sin(angle), 0, cos(angle)].
    // Body facing versus aim is still Unknown; do not substitute movement.
    if let Some(raw) = bits(payload, start + 159, 13) {
        let magnitude = (raw & 0xfff) as f64 / 4095.0;
        let sign = if raw & 0x1000 == 0 { 1.0 } else { -1.0 };
        let radians = sign * magnitude * std::f64::consts::PI;
        fields.push(field("PlayerStateDirectionMeaning", "Unknown", None));
        fields.push(field(
            "PlayerStateHorizontalAngleRaw",
            raw,
            Some([start + 159, start + 172]),
        ));
        fields.push(field(
            "PlayerStateHorizontalAngleDeg",
            format!("{:.6}", radians.to_degrees()),
            Some([start + 159, start + 172]),
        ));
        fields.push(field(
            "PlayerStateDirectionX",
            format!("{:.6}", radians.sin()),
            None,
        ));
        fields.push(field(
            "PlayerStateDirectionZ",
            format!("{:.6}", radians.cos()),
            None,
        ));
    }
    // The guarded explicit PlayerNetState layouts carry the concrete actor
    // resource group immediately before presence and body. This is distinct
    // from the leading CoopSeqPlayerNetState stats group 1107/1108.
    value_field(fields, payload, "PlayerStateGroupId", start - 33, 32);
    // Owned 11.3.0 encoder 0x27fce70..0x27fce90 writes state +0xb4
    // plus one in five bits; decoder 0x27fda6c..0x27fdaa0 subtracts one.
    // Keep the -1 sentinel and the neutral matching-value name.
    if let Some(raw) = bits(payload, start + 304, 5) {
        fields.push(field(
            "PlayerStateMatchingValue",
            raw as i64 - 1,
            Some([start + 304, start + 309]),
        ));
    }
}

fn standalone_player_state_axes_start(payload: &[u8], group: u64) -> Option<usize> {
    // The existing shifted checks below are equivalent to the appended
    // frame's length 619 at bit 235, group 200000/210000 at 247, and present
    // flag at 279. Its PlayerNetState candidate body starts at bit 280.
    let marker = match group {
        1107 => 0x800186a0,
        1108 => 0x80019a28,
        _ => return None,
    };
    (payload.len() == 109
        && bits(payload, 0, 1) == Some(1)
        && bits(payload, 1, 12) == Some(222)
        && bits(payload, 45, 1) == Some(1)
        && bits(payload, 72, 3) == Some(4)
        && bits(payload, 123, 3) == Some(4)
        && bits(payload, 235, 1) == Some(1)
        && bits(payload, 236, 12) == Some(309)
        && bits(payload, 248, 32) == Some(marker)
        && bits(payload, 866, 6) == Some(0))
    .then_some(280)
}

fn nested_host_player_state_axes_start(payload: &[u8]) -> Option<usize> {
    // The host's 1104 state contains a 1107 frame at bit 75. The shortest
    // observed composite is 118 bytes and ends after the 586-bit body.
    (payload.len() >= 118
        && bits(payload, 0, 1) == Some(1)
        && bits(payload, 1, 12) == Some(63)
        && bits(payload, 45, 1) == Some(1)
        && bits(payload, 75, 1) == Some(0)
        && bits(payload, 76, 12) == Some(222)
        && bits(payload, 88, 32) == Some(1107)
        && bits(payload, 120, 1) == Some(1)
        && bits(payload, 147, 3) == Some(4)
        && bits(payload, 198, 3) == Some(4)
        && bits(payload, 310, 1) == Some(1)
        && bits(payload, 311, 12) == Some(309)
        && bits(payload, 323, 32) == Some(0x800186a0)
        && (payload.len() != 118 || bits(payload, 941, 3) == Some(0)))
    .then_some(355)
}

fn nested_host_stats_fields(payload: &[u8], fields: &mut Vec<Field>) {
    // The global mode bit belongs to the whole P5 payload. The outer 1104
    // body ends at bit75 (native0x382cf24..0x382cf44); the next frame's
    // length begins at76. Bit75 is not a second mode and may be either value.
    if payload.len().saturating_mul(8) < 310
        || bits(payload, 0, 1) != Some(1)
        || bits(payload, 1, 12) != Some(63)
        || bits(payload, 13, 32) != Some(1104)
        || bits(payload, 45, 1) != Some(1)
        || bits(payload, 76, 12) != Some(222)
        || bits(payload, 88, 32) != Some(1107)
        || bits(payload, 120, 1) != Some(1)
        || bits(payload, 147, 3) != Some(4)
        || bits(payload, 198, 3) != Some(4)
    {
        return;
    }
    for (name, start, width) in [
        ("NestedStatsFrameLength", 76, 12),
        ("NestedStatsGroupId", 88, 32),
        ("NestedStatsPresent", 120, 1),
        ("NestedStatsIkuraNum", 121, 14),
        ("NestedStatsHeldGoldenIkuraNum", 135, 12),
        ("NestedStatsArrayACount", 147, 3),
        ("NestedStatsArrayBCount", 198, 3),
    ] {
        value_field(fields, payload, name, start, width);
    }
    for index in 0..4 {
        value_field(
            fields,
            payload,
            &format!("NestedStatsArrayA[{index}]"),
            150 + index * 12,
            12,
        );
        value_field(
            fields,
            payload,
            &format!("NestedStatsArrayB[{index}]"),
            201 + index * 12,
            12,
        );
    }
}

fn salmon_type(id: u64) -> &'static str {
    match id {
        0 => "SakelienStandard",
        1 => "SakelienLarge",
        2 => "SakelienSmall",
        3 => "SakeFlyBagman",
        4 => "SakelienBomber",
        5 => "SakelienCupTwins",
        6 => "SakelienShield",
        7 => "SakelienSnake",
        8 => "SakelienTower",
        9 => "Sakediver",
        10 => "Sakerocket",
        11 => "SakePillar",
        12 => "SakeDolphin",
        13 => "SakeArtillery",
        14 => "SakeSaucer",
        15 => "SakelienGolden",
        16 => "SakelienGeyser",
        17 => "Sakedozer",
        18 => "SakeCarrier",
        19 => "SakeCopter",
        20 => "SakeBigMouth",
        21 => "SakeBigMouthGold",
        22 => "SakeShip",
        23 => "SakelienGiant",
        24 => "SakeRope",
        25 => "SakeJaw",
        26 => "SakeBigBalloon",
        27 => "SakeGhost",
        28 => "SakeBalloon",
        29 => "SakeWheel",
        30 => "Triple",
        31 => "Random",
        _ => "Unknown",
    }
}

fn stage_name(resource: &str) -> &'static str {
    match resource {
        "Cop_Shakeup" => "Spawning Grounds",
        "Cop_Shakespiral" => "Sockeye Station",
        "Cop_Shakelift" => "Salmonid Smokeyard",
        "Cop_Shakeship" => "Marooner's Bay",
        "Cop_Shakedent" => "Gone Fission Hydroplant",
        "Cop_Shakehighway" => "Jammin' Salmon Junction",
        "Cop_Shakerail" => "Bonerattle Arena",
        _ => "Unknown",
    }
}

// The captured Protocol 8 layouts locate the trailing frame fields. Type ID
// and fixed leading fields are interpreted independently of the total length.
fn event_body_bits(id: u32, payload: &[u8], port: Option<u8>) -> Option<usize> {
    if matches!(id, 0xf010cc79 | 0x1a894818) && port != Some(100) {
        return None;
    }
    let body_bits = match (id, payload.len()) {
        (0x9a1e036d, 211) if EquipmentSetting::from_payload(payload).is_some() => 1459,
        (0x0caefc01, 89) => 481,
        (0x6e7d7386, 60) => 252,
        (0x5b64bbf1, 38) => 73,
        (0xedbcbb02, 37) => 69,
        (0xf010cc79, 53) => 195,
        (0x1a894818, 33) => 36,
        (0x3d8d2c59, 29) => 4,
        (0x4939c1ac, 48) => 154,
        (0xe3f0eb9a, 48) => 155,
        // Owned 11.3.0 exact Coop lifecycle / round-setup codecs.
        (0xcf9be194, 29) => 4,
        (0x74400e72, 43) => 110,
        (0x89e91844, 39) => 78,
        (0x4c4d2b61, 29) => 2,
        _ => return None,
    };
    if matches!(id, 0xcf9be194 | 0x74400e72 | 0x89e91844 | 0x4c4d2b61) {
        let padding_start = 200 + body_bits + 27;
        if bits(payload, padding_start, payload.len() * 8 - padding_start) != Some(0) {
            return None;
        }
    }
    Some(body_bits)
}

fn semantic_fields(chunk: &Chunk) -> Vec<Field> {
    let p = &chunk.payload;
    let mut fields = vec![field("PayloadBytes", p.len(), None)];
    match chunk.protocol {
        1 if p.len() >= 8 && p[0] == 1 => {
            if matches!(p[1], 0x11 | 0x12 | 0x50 | 0x51) {
                fields.push(field(
                    if p[1] == 0x50 || p[1] == 0x51 {
                        "Generation"
                    } else {
                        "Sequence"
                    },
                    u32::from_be_bytes(p[4..8].try_into().unwrap()),
                    Some([32, 64]),
                ));
            } else if p[1] == 0x33 && p.len() >= 41 {
                fields.push(field("Reason", p[38], Some([304, 312])));
                fields.push(field(
                    "Detail",
                    u16::from_be_bytes([p[39], p[40]]),
                    Some([312, 328]),
                ));
            }
        },
        3 if p.len() == 21 => {
            // ElapsedMicroseconds belongs to this sender; it is not the
            // capture-time interval computed after a matched Type 1 response.
            fields.push(field("Type", p[0], Some([0, 8])));
            fields.push(field(
                "OriginTick",
                u64::from_be_bytes(p[1..9].try_into().unwrap()),
                Some([8, 72]),
            ));
            fields.push(field(
                "ElapsedMicroseconds",
                u64::from_be_bytes(p[9..17].try_into().unwrap()),
                Some([72, 136]),
            ));
            fields.push(field(
                "DestinationVariableId",
                u16::from_be_bytes(p[17..19].try_into().unwrap()),
                Some([136, 152]),
            ));
            fields.push(field(
                "ProtocolStationVariableId",
                u16::from_be_bytes(p[19..21].try_into().unwrap()),
                Some([152, 168]),
            ));
        },
        5 => {
            if let Some(group) = coop_state_group(p) {
                value_field(&mut fields, p, "FrameMode", 0, 1);
                value_field(&mut fields, p, "FrameLength", 1, 12);
                fields.push(field("GroupId", group, Some([13, 45])));
                value_field(&mut fields, p, "Present", 45, 1);
                if group == 1104 {
                    value_field(&mut fields, p, "Sequence", 46, 3);
                    value_field(&mut fields, p, "Quota", 49, 6);
                    value_field(&mut fields, p, "BaselineA", 55, 10);
                    value_field(&mut fields, p, "BaselineB", 65, 10);
                    if chunk.port == Some(101) {
                        nested_host_stats_fields(p, &mut fields);
                    }
                    if chunk.port == Some(101)
                        && let Some(start) = nested_host_player_state_axes_start(p)
                    {
                        player_state_axes(&mut fields, p, start);
                    }
                } else {
                    value_field(&mut fields, p, "ArrayACount", 72, 3);
                    value_field(&mut fields, p, "ArrayBCount", 123, 3);
                    // Preserve the existing generic array field names. Named
                    // participant scalars require this complete count-four shape.
                    // The 222-bit frame begins after the 13-bit mode/length header;
                    // the authenticated chunk may contain more data after that frame.
                    if bits(p, 1, 12) == Some(222)
                        && p.len() * 8 >= 13 + 222
                        && bits(p, 45, 1) == Some(1)
                        && bits(p, 72, 3) == Some(4)
                        && bits(p, 123, 3) == Some(4)
                    {
                        value_field(&mut fields, p, "IkuraNum", 46, 14);
                        value_field(&mut fields, p, "HeldGoldenIkuraNum", 60, 12);
                        for index in 0..4 {
                            value_field(
                                &mut fields,
                                p,
                                &format!("ArrayA[{index}]"),
                                75 + index * 12,
                                12,
                            );
                            value_field(
                                &mut fields,
                                p,
                                &format!("ArrayB[{index}]"),
                                126 + index * 12,
                                12,
                            );
                        }
                    }
                    if chunk.port == Some(101)
                        && let Some(start) = standalone_player_state_axes_start(p, group)
                    {
                        player_state_axes(&mut fields, p, start);
                    }
                }
            }
        },
        8 if p.len() >= 25 && p[0] == 7 => {
            if let Some(id) = event_type(p) {
                fields.push(field("TypeId", format!("0x{id:08x}"), Some([168, 200])));
                let confirmed = event_body_bits(id, p, chunk.port);
                match id {
                    0xcf9be194 if confirmed == Some(4) => {
                        let index = bits(p, 200, 4).expect("complete rescued body") as i64 - 1;
                        fields.push(field("RescuerIndex", index, Some([200, 204])));
                    },
                    0xf010cc79 if confirmed == Some(195) => {
                        // 0x379a090/0x379a590 encode/decode the complete shot.
                        // The receiver copies XYZ A into BulletGoldenIkura's
                        // creation position. XYZ B is a separate input, not a
                        // proven landing point; keep that triple neutral.
                        for (name, start) in [
                            ("ShotStartX", 200),
                            ("ShotStartY", 217),
                            ("ShotStartZ", 234),
                            ("ShotOtherX", 292),
                            ("ShotOtherY", 309),
                            ("ShotOtherZ", 326),
                        ] {
                            if let Some(value) = axis(p, start) {
                                fields.push(field(
                                    name,
                                    format!("{value:.3}"),
                                    Some([start, start + 17]),
                                ));
                            }
                        }
                        // Direction helper 0x31cbac0 writes 12 then 13 bits;
                        // 0x31cc354 follows it with a 16-bit magnitude.
                        value_field(&mut fields, p, "ShotVectorRawA", 251, 12);
                        value_field(&mut fields, p, "ShotVectorRawB", 263, 13);
                        value_field(&mut fields, p, "ShotVectorLengthRaw", 276, 16);
                        let velocity = shot_velocity(
                            bits(p, 251, 12).expect("confirmed shot direction") as u32,
                            bits(p, 263, 13).expect("confirmed shot direction") as u32,
                            bits(p, 276, 16).expect("confirmed shot magnitude") as u32,
                        );
                        for (name, value) in ["ShotVelocityX", "ShotVelocityY", "ShotVelocityZ"]
                            .into_iter()
                            .zip(velocity)
                        {
                            fields.push(field(name, format!("{value:.6}"), Some([251, 292])));
                        }
                        value_field(&mut fields, p, "ShotValueRaw", 343, 16);
                        value_field(&mut fields, p, "ShotValuePlusOne", 359, 4);
                        value_field(&mut fields, p, "ShotItemId", 363, 32);
                    },
                    0x1a894818 if confirmed == Some(36) => {
                        // 0x3799740/0x3799808: identity32 + plus-one4.
                        // This event contains no position or landing field.
                        value_field(&mut fields, p, "SleepItemId", 200, 32);
                        value_field(&mut fields, p, "SleepValuePlusOne", 232, 4);
                    },
                    0x4939c1ac | 0xe3f0eb9a
                        if confirmed
                            .is_some_and(|body_bits| p.len() * 8 >= 200 + body_bits + 27) =>
                    {
                        // Both paint-event encoders begin with three 17-bit axes.
                        // Keep the next two 12-bit values raw: the ELF stores
                        // each as a float after scaling by 64.0 / 4095.0,
                        // while their gameplay meanings remain unknown.
                        for (name, start) in [("PaintX", 200), ("PaintY", 217), ("PaintZ", 234)] {
                            if let Some(value) = axis(p, start) {
                                fields.push(field(
                                    name,
                                    format!("{value:.3}"),
                                    Some([start, start + 17]),
                                ));
                            }
                        }
                        value_field(&mut fields, p, "PaintScalarRawA", 251, 12);
                        value_field(&mut fields, p, "PaintScalarRawB", 263, 12);
                    },
                    0x0caefc01 if p.len() * 8 >= 333 => {
                        for (name, start) in [("SpawnX", 200), ("SpawnY", 217), ("SpawnZ", 234)] {
                            if let Some(value) = axis(p, start) {
                                fields.push(field(
                                    name,
                                    format!("{value:.3}"),
                                    Some([start, start + 17]),
                                ));
                            }
                        }
                        if let Some(salmon_id) = bits(p, 328, 5) {
                            fields.push(field("SalmonId", salmon_id, Some([328, 333])));
                            fields.push(field("SalmonType", salmon_type(salmon_id), None));
                        }
                    },
                    0x6e7d7386 if p.len() * 8 >= 452 => {
                        for (name, start) in [
                            ("SpawnX", 200),
                            ("SpawnY", 217),
                            ("SpawnZ", 234),
                            ("OtherX", 401),
                            ("OtherY", 418),
                            ("OtherZ", 435),
                        ] {
                            if let Some(value) = axis(p, start) {
                                fields.push(field(
                                    name,
                                    format!("{value:.3}"),
                                    Some([start, start + 17]),
                                ));
                            }
                        }
                        value_field(&mut fields, p, "TransformValue", 336, 16);
                        value_field(&mut fields, p, "LinkByte", 385, 8);
                        value_field(&mut fields, p, "LinkNibble", 393, 4);
                    },
                    0x9a1e036d if p.len() * 8 >= 1203 => {
                        if let Some(equipment) = EquipmentSetting::from_payload(p) {
                            fields.extend(equipment.fields());
                        }
                        if let Some(stage) = p.get(25..89) {
                            let end = stage.iter().position(|v| *v == 0).unwrap_or(stage.len());
                            if end > 0 && stage[..end].iter().all(|v| v.is_ascii_graphic()) {
                                let resource = String::from_utf8_lossy(&stage[..end]);
                                fields.push(field("StageResource", &resource, Some([200, 712])));
                                fields.push(field("StageName", stage_name(&resource), None));
                            }
                        }
                        value_field(&mut fields, p, "JobType", 712, 3);
                        if let Some(wire) = bits(p, 1064, 11) {
                            fields.push(field("DifficultyWire", wire, Some([1064, 1075])));
                            if wire > 0 {
                                fields.push(field("Difficulty", wire - 1, None));
                            }
                        }
                        for index in 0..4 {
                            value_field(
                                &mut fields,
                                p,
                                &format!("GameSeed[{index}]"),
                                1075 + 32 * index,
                                32,
                            );
                        }
                    },
                    0x5b64bbf1 | 0xedbcbb02 if p.len() * 8 >= 232 => {
                        value_field(&mut fields, p, "LinkValue", 200, 32);
                    },
                    0x3d8d2c59 if p.len() * 8 >= 204 => {
                        value_field(&mut fields, p, "ValuePlusOne", 200, 4);
                    },
                    _ => {},
                }
                if let Some(body_bits) = confirmed {
                    value_field(&mut fields, p, "GameFrame", 200 + body_bits, 23);
                    value_field(&mut fields, p, "LifeNumber", 223 + body_bits, 4);
                }
                value_field(&mut fields, p, "EventResourceGroupId", 136, 32);
            }
        },
        11 => {
            if let Some(profiles) = crate::station_profile::fields(p) {
                fields.extend(profiles);
            }
            if let Some(roster) = crate::station_profile::roster_fields(p) {
                fields.extend(roster);
            }
            if let Some(setting) = private_setting(chunk) {
                if let Some(stage_id) = setting.stage_id {
                    fields.push(field("StageId", stage_id, Some([100, 612])));
                }
                if let Some((wire, start)) = setting.difficulty_wire {
                    fields.push(field("DifficultyWire", wire, Some([start, start + 11])));
                    if wire > 0 {
                        fields.push(field("Difficulty", wire - 1, None));
                    }
                }
            }
        },
        13 => {
            if let Some(roster) = crate::station_profile::session_roster_fields(p) {
                fields.extend(roster);
            }
        },
        _ => {},
    }
    fields
}

#[cfg(test)]
#[path = "../test_support/unit/decode.rs"]
mod tests;
