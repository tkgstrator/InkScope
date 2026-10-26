//! Stateful LAN discovery, authenticated Pia decoding, and conservative field labels.
use crate::Result;
use crate::crypto::{self, GameKey, PiaKey};
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
        }
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
            Ok(chunks) => chunks
                .into_iter()
                .enumerate()
                .map(|(index, chunk)| {
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
                        if semantic.len() > 2 {
                            if chunk.protocol == 8
                                && event_type(&chunk.payload).is_some_and(|id| {
                                    event_body_bits(id, chunk.payload.len()).is_none()
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
                    output
                })
                .collect(),
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
        3 => "RttProtocol",
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
        "Cop_Shakedent" => "Gone Fission Hydroplant",
        _ => "Unknown",
    }
}

// The captured Protocol 8 layouts locate the trailing frame fields. Type ID
// and fixed leading fields are interpreted independently of the total length.
fn event_body_bits(id: u32, length: usize) -> Option<usize> {
    match (id, length) {
        (0x9a1e036d, 211) => Some(1459),
        (0x0caefc01, 89) => Some(481),
        (0x6e7d7386, 60) => Some(252),
        (0x5b64bbf1, 38) => Some(73),
        (0xedbcbb02, 37) => Some(69),
        (0xf010cc79, 53) => Some(195),
        (0x3d8d2c59, 29) => Some(4),
        _ => None,
    }
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
                } else {
                    value_field(&mut fields, p, "ArrayACount", 72, 3);
                    value_field(&mut fields, p, "ArrayBCount", 123, 3);
                }
            }
        },
        8 if p.len() >= 25 && p[0] == 7 => {
            if let Some(id) = event_type(p) {
                fields.push(field("TypeId", format!("0x{id:08x}"), Some([168, 200])));
                let confirmed = event_body_bits(id, p.len());
                match id {
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
            }
        },
        11 if p.len() == 161 && chunk.port == Some(100) => {
            // Observed standalone q candidate: not a complete serializer decoder.
            let q = bits(p, 102, 10).unwrap_or(0);
            if bits(p, 96, 6) == Some(32) && q > 0 && (q - 1).is_multiple_of(5) && q <= 1001 {
                fields.push(field(
                    "HazardCandidatePercent",
                    format!("{:.1}", (q as f64 - 1.0) / 5.0),
                    Some([102, 112]),
                ));
            }
        },
        _ => {},
    }
    fields
}

#[cfg(test)]
#[path = "../test_support/unit/decode.rs"]
mod tests;
