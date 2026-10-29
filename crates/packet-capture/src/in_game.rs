//! Conservative recording gates from authenticated Salmon Run messages.
//!
//! Settings arm a session; an explicit sequence state starts video. A round
//! finish pauses video until a later sequence. Neither Session 5/6 nor the
//! number of observed finish events is interpreted as a clear/failure result.
use inkscope::decode::Row;
use inkscope::pcap::UdpPacket;
use std::collections::VecDeque;
use std::net::Ipv4Addr;

const MAX_IDENTITIES: usize = 256;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VideoAction {
    #[default]
    None,
    Start,
    Stop,
}

/// Apply close_session before start_session, then apply the video action.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Decision {
    pub start_session: bool,
    pub save_packet: bool,
    pub video: VideoAction,
    pub close_session: Option<&'static str>,
}

#[derive(Clone, Copy)]
struct Room {
    host: Ipv4Addr,
    client: Ipv4Addr,
}

impl Room {
    fn contains(self, packet: &UdpPacket) -> bool {
        self.is_from_host(packet) || packet.source == self.client && packet.destination == self.host
    }

    fn is_from_host(self, packet: &UdpPacket) -> bool {
        packet.source == self.host && packet.destination == self.client
    }
}

#[derive(Default)]
pub struct Gate {
    room: Option<Room>,
    settings: VecDeque<Vec<u8>>,
    finishes: VecDeque<Vec<u8>>,
    sequence: Option<u8>,
    video_active: bool,
    finished_without_sequence: bool,
}

impl Gate {
    /// Forget the room and deduplication state after an explicit close or timeout.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Inspect one original UDP record and the rows decoded from that record.
    ///
    /// Raw authenticated chunk bytes are required for event deduplication. The
    /// caller must request decoder details internally, even if not displaying
    /// details. No room key is held or emitted by this state machine.
    pub fn observe(&mut self, packet: &UdpPacket, rows: &[Row]) -> Decision {
        if packet.source_port != 49152
            || packet.destination_port != 49152
            || self.room.is_some_and(|room| !room.contains(packet))
        {
            return Decision::default();
        }

        let mut decision = Decision {
            save_packet: self.video_active,
            ..Decision::default()
        };
        let from_host = self.room.is_none_or(|room| room.is_from_host(packet));
        let setting = from_host
            .then(|| rows.iter().find_map(|row| setting_identity(packet, row)))
            .flatten();

        if let Some(identity) = setting.as_ref()
            && !self.settings.contains(identity)
        {
            if self.room.is_some() {
                decision.close_session = Some("new_setting");
            }
            if self.video_active {
                decision.video = VideoAction::Stop;
            }
            self.room = Some(Room {
                host: packet.source,
                client: packet.destination,
            });
            remember(&mut self.settings, identity.clone());
            self.sequence = None;
            self.video_active = false;
            self.finished_without_sequence = false;
            decision.start_session = true;
            decision.save_packet = true;
        }

        let Some(room) = self.room else {
            return decision;
        };
        if !room.is_from_host(packet) {
            return decision;
        }

        // A start setting in this same UDP record wins over an older lobby
        // update coalesced alongside it. An ACK alone never closes a session.
        if setting.is_none() && rows.iter().any(|row| lobby_setting(packet, row)) {
            let was_active = self.video_active;
            self.reset();
            return Decision {
                video: if was_active {
                    VideoAction::Stop
                } else {
                    VideoAction::None
                },
                close_session: Some("lobby_setting"),
                ..Decision::default()
            };
        }

        for row in rows {
            if let Some(sequence) = state_sequence(packet, row) {
                // With no observed sequence before the finish, a later state
                // cannot be distinguished from a delayed old-wave update.
                if self.finished_without_sequence {
                    continue;
                }
                // After a finish, repeated state rows for that same wave must
                // not reopen video. Older reordered sequence states are ignored.
                if self.sequence.is_none_or(|previous| sequence > previous) {
                    self.sequence = Some(sequence);
                    if !self.video_active {
                        self.video_active = true;
                        decision.video = VideoAction::Start;
                    }
                    decision.save_packet = true;
                }
            } else if let Some(identity) = finish_identity(packet, row)
                && !self.finishes.contains(&identity)
            {
                remember(&mut self.finishes, identity);
                self.finished_without_sequence = self.sequence.is_none();
                decision.save_packet = true;
                if self.video_active {
                    self.video_active = false;
                    decision.video = VideoAction::Stop;
                }
            }
        }
        decision
    }
}

fn value<'a>(row: &'a Row, name: &str) -> Option<&'a str> {
    row.fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.value.as_str())
}

fn number(row: &Row, name: &str) -> Option<u64> {
    value(row, name)?.parse().ok()
}

fn authenticated(packet: &UdpPacket, row: &Row, protocol: u8, port: u8) -> bool {
    row.status == "authenticated"
        && row.protocol == Some(protocol)
        && row.port == Some(port)
        && row.record == packet.record
        && row.source == format!("{}:{}", packet.source, packet.source_port)
        && row.destination == format!("{}:{}", packet.destination, packet.destination_port)
}

fn event_identity(row: &Row, length: usize, type_id: u32) -> Option<Vec<u8>> {
    if number(row, "PayloadBytes") != Some(length as u64)
        || value(row, "TypeId")? != format!("0x{type_id:08x}")
    {
        return None;
    }
    let payload = hex::decode(row.authenticated_payload_hex.as_ref()?).ok()?;
    if payload.len() != length
        || payload[0] != 7
        || payload[7] != 0
        || usize::from(u16::from_be_bytes([payload[1], payload[2]])) + 8 != length
        || usize::from(u16::from_be_bytes([payload[13], payload[14]])) + 17 != length
        || u32::from_le_bytes(payload[21..25].try_into().ok()?) != type_id
    {
        return None;
    }
    // The outer packet ID and event envelope can change on retransmission.
    // Keep the serialized event body, including its own frame/life metadata.
    Some(payload[25..].to_vec())
}

fn setting_identity(packet: &UdpPacket, row: &Row) -> Option<Vec<u8>> {
    if !authenticated(packet, row, 8, 100)
        || row.kind != "OnlineCoopSetting"
        || row.layout.as_deref() != Some("confirmed_fields")
    {
        return None;
    }
    let stage = value(row, "StageResource")?;
    if !stage.starts_with("Cop_")
        || stage.len() <= 4
        || stage.len() > 63
        || !stage
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || number(row, "JobType")? > 7
        || number(row, "Difficulty")? > 2046
        || number(row, "GameFrame")? > 0x7f_ffff
        || number(row, "LifeNumber")? > 15
    {
        return None;
    }
    for index in 0..4 {
        if number(row, &format!("GameSeed[{index}]"))? > u64::from(u32::MAX) {
            return None;
        }
    }
    event_identity(row, 211, 0x9a1e036d)
}

fn state_sequence(packet: &UdpPacket, row: &Row) -> Option<u8> {
    if !authenticated(packet, row, 5, 101)
        || row.kind != "CoopSequenceNetState"
        || row.layout.as_deref() != Some("confirmed_fields")
        || number(row, "FrameMode") != Some(1)
        || number(row, "FrameLength") != Some(63)
        || number(row, "GroupId") != Some(1104)
        || number(row, "Present") != Some(1)
        || !(10..=65_535).contains(&number(row, "PayloadBytes")?)
    {
        return None;
    }
    // Only the three ordinary WAVE sequence values have observation evidence.
    let sequence = number(row, "Sequence")?;
    (sequence <= 2).then_some(sequence as u8)
}

fn finish_identity(packet: &UdpPacket, row: &Row) -> Option<Vec<u8>> {
    if !authenticated(packet, row, 8, 100)
        || row.kind != "CoopSeqRoundFinishEvent"
        || !matches!(
            row.layout.as_deref(),
            Some("type_only" | "type_fields" | "confirmed_fields")
        )
    {
        return None;
    }
    event_identity(row, 55, 0xc7cd3faf)
}

fn lobby_setting(packet: &UdpPacket, row: &Row) -> bool {
    authenticated(packet, row, 11, 100)
        && row.kind == "PrivateSetting"
        && row.layout.as_deref() == Some("confirmed_fields")
        && matches!(number(row, "PayloadBytes"), Some(78 | 161 | 226))
        && (value(row, "StageId").is_some() || value(row, "Difficulty").is_some())
}

fn remember(identities: &mut VecDeque<Vec<u8>>, identity: Vec<u8>) {
    if identities.len() == MAX_IDENTITIES {
        identities.pop_front();
    }
    identities.push_back(identity);
}

#[cfg(test)]
#[path = "../test_support/unit/in_game.rs"]
mod tests;
