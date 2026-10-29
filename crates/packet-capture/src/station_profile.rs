//! Bounded StationInfo profiles and internal station roster identity fields.
//!
//! Slots after UserSetting and final inner-frame consumption are unproved.
//! Keep the resulting row classified as type_fields, even with many fields.
//! Roster identity adapters are internal transport data, not public player IDs.

use crate::decode::Field;

const USER_SETTING_BITS: usize = 1695;
const MAX_PROFILES: usize = 10;
const USER_NAME_BYTES: usize = 51;
const MAX_SESSION_NAME_BYTES: usize = 256;

fn bits(payload: &[u8], start: usize, width: usize, end: usize) -> Option<u64> {
    if width > 64 || start.checked_add(width)? > end.min(payload.len().checked_mul(8)?) {
        return None;
    }
    Some((0..width).fold(0, |value, offset| {
        value | (u64::from((payload[(start + offset) / 8] >> ((start + offset) % 8)) & 1) << offset)
    }))
}

fn zero_bits(payload: &[u8], start: usize, end: usize) -> bool {
    start <= end
        && end <= payload.len().saturating_mul(8)
        && (start..end).all(|bit| payload[bit / 8] & (1 << (bit % 8)) == 0)
}

struct Reader<'a> {
    payload: &'a [u8],
    position: usize,
    end: usize,
}

impl Reader<'_> {
    fn read(&mut self, width: usize) -> Option<u64> {
        let value = bits(self.payload, self.position, width, self.end)?;
        self.position += width;
        Some(value)
    }

    fn skip(&mut self, width: usize) -> Option<()> {
        self.position = self.position.checked_add(width)?;
        (self.position <= self.end).then_some(())
    }
}

struct Snapshot {
    station_index: u8,
    handler_start: usize,
    origin: usize,
}

fn display_name(payload: &[u8], origin: usize, end: usize) -> Option<String> {
    let mut bytes = [0; USER_NAME_BYTES];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = bits(payload, origin.checked_add(256 + index * 8)?, 8, end)? as u8;
    }
    let length = bytes.iter().position(|byte| *byte == 0)?;
    if length == 0 || bytes[length..].iter().any(|byte| *byte != 0) {
        return None;
    }
    let name = std::str::from_utf8(&bytes[..length]).ok()?;
    // The audited native UI converter consumes one-, two-, or three-byte
    // characters. Preserve unsupported names as absent, without replacement.
    if name
        .chars()
        .any(|ch| ch.is_control() || u32::from(ch) > 0xffff)
    {
        return None;
    }
    Some(name.to_owned())
}

// Deliberately no Debug or Serialize: private station identities are compared
// internally and enter WireRow only through the explicit adapters below.
#[derive(Clone, Copy, PartialEq, Eq)]
struct StationIdentity {
    constant: u64,
    variable: u16,
}

#[derive(Clone, Copy)]
struct StationRosterEntry {
    identity: StationIdentity,
    active: bool,
}

struct StationRoster {
    entries: [StationRosterEntry; MAX_PROFILES],
}

fn station_roster(payload: &[u8]) -> Option<StationRoster> {
    let end = 130 * 8;
    if payload.len() != 130
        || payload[0] != 7
        || payload[7] != 0
        || usize::from(u16::from_be_bytes([payload[1], payload[2]])) + 8 != payload.len()
        || bits(payload, 64, 1, end)? != 0
        || bits(payload, 65, 32, end)? != 1
        || !zero_bits(payload, 97, 101)
        || bits(payload, 101, 1, end)? != 1
        || bits(payload, 102, 4, end)? != 10
        || !zero_bits(payload, 1033, end)
    {
        return None;
    }
    let mut entries = [StationRosterEntry {
        identity: StationIdentity {
            constant: 0,
            variable: 0,
        },
        active: false,
    }; MAX_PROFILES];
    for index in 0..MAX_PROFILES {
        let start = 106 + 81 * index;
        let identity = StationIdentity {
            constant: bits(payload, start, 64, end)?,
            variable: bits(payload, start + 64, 16, end)? as u16,
        };
        let active = bits(payload, start + 80, 1, end)? != 0;
        if identity.constant == 0 {
            if identity.variable != 0 || active {
                return None;
            }
        } else if entries[..index]
            .iter()
            .any(|entry| entry.identity == identity)
        {
            return None;
        }
        entries[index] = StationRosterEntry { identity, active };
    }
    Some(StationRoster { entries })
}

/// Internal WireRow adapter. The server must strip private station identity
/// fields from public DTOs and checkpoints after its bounded identity join.
pub(crate) fn roster_fields(payload: &[u8]) -> Option<Vec<Field>> {
    let roster = station_roster(payload)?;
    let field = |name: String, value: String, bits: Option<[usize; 2]>| Field { name, value, bits };
    let mut fields = vec![
        field(
            "StationRosterScope".into(),
            "VersusSettingStationIdArrayStandalone".into(),
            None,
        ),
        field("StationRosterCount".into(), MAX_PROFILES.to_string(), None),
        field(
            "StationRosterActiveCount".into(),
            roster
                .entries
                .iter()
                .filter(|entry| entry.active)
                .count()
                .to_string(),
            None,
        ),
    ];
    for (index, entry) in roster.entries.iter().enumerate() {
        let start = 106 + 81 * index;
        fields.push(field(
            format!("StationRoster[{index}].Active"),
            entry.active.to_string(),
            Some([start + 80, start + 81]),
        ));
        fields.push(field(
            format!("StationRoster[{index}].StationConstant"),
            entry.identity.constant.to_string(),
            Some([start, start + 64]),
        ));
        fields.push(field(
            format!("StationRoster[{index}].StationVariable"),
            entry.identity.variable.to_string(),
            Some([start + 64, start + 80]),
        ));
    }
    Some(fields)
}

struct SessionRosterEntry {
    identity: StationIdentity,
    origin: usize,
}

fn session_roster(payload: &[u8]) -> Option<Vec<SessionRosterEntry>> {
    if !(27..=27 + MAX_PROFILES * (78 + MAX_SESSION_NAME_BYTES)).contains(&payload.len())
        || payload[0] != 5
        || payload[3..7] != [1, 0, 0, 3]
        || !(1..=MAX_PROFILES).contains(&usize::from(payload[17]))
        || payload[18] != 0
    {
        return None;
    }
    let mut entries: Vec<SessionRosterEntry> = Vec::new();
    let mut origin = 27usize;
    for _ in 0..payload[17] {
        if origin.checked_add(78)? > payload.len() || payload[origin + 53] != 1 {
            return None;
        }
        let identity = StationIdentity {
            constant: u64::from_be_bytes(payload[origin..origin + 8].try_into().ok()?),
            variable: u16::from_be_bytes(payload[origin + 8..origin + 10].try_into().ok()?),
        };
        if identity.constant == 0
            || identity.variable == 0
            || entries.iter().any(|entry| entry.identity == identity)
        {
            return None;
        }
        // One opaque inner item only. Its UID and bounded name are skipped;
        // UserSetting, not the Session name, supplies the display name.
        let length =
            u32::from_be_bytes(payload[origin + 73..origin + 77].try_into().ok()?) as usize;
        if length > MAX_SESSION_NAME_BYTES || payload[origin + 77] != 1 {
            return None;
        }
        let next = origin.checked_add(78)?.checked_add(length)?;
        if next > payload.len() {
            return None;
        }
        entries.push(SessionRosterEntry { identity, origin });
        origin = next;
    }
    (origin == payload.len()).then_some(entries)
}

/// Internal identity adapter for the audited single-fragment, one-inner-item
/// Session5 subset. It exports neither Session names nor opaque inner UIDs.
pub(crate) fn session_roster_fields(payload: &[u8]) -> Option<Vec<Field>> {
    let roster = session_roster(payload)?;
    let field = |name: String, value: String, bits: Option<[usize; 2]>| Field { name, value, bits };
    let mut fields = vec![
        field(
            "SessionRosterScope".into(),
            "Session5SingleFragmentStations".into(),
            None,
        ),
        field("SessionRosterCount".into(), roster.len().to_string(), None),
    ];
    for (index, entry) in roster.iter().enumerate() {
        fields.push(field(
            format!("SessionRoster[{index}].StationConstant"),
            entry.identity.constant.to_string(),
            Some([entry.origin * 8, (entry.origin + 8) * 8]),
        ));
        fields.push(field(
            format!("SessionRoster[{index}].StationVariable"),
            entry.identity.variable.to_string(),
            Some([(entry.origin + 8) * 8, (entry.origin + 10) * 8]),
        ));
    }
    Some(fields)
}

fn frame(
    payload: &[u8],
    start: usize,
    end: usize,
    snapshots: &mut Vec<Snapshot>,
    profile_bodies: &mut usize,
) -> Option<bool> {
    let mut reader = Reader {
        payload,
        position: start,
        end,
    };
    let handler = reader.read(32)?;
    if !(2..12).contains(&handler) {
        return Some(false);
    }
    if reader.read(1)? != 0 {
        reader.skip(3)?;
    } // native StationInfo sequence enum
    for width in [1, 1, 16, 20, 696] {
        if reader.read(1)? != 0 {
            reader.skip(width)?;
        }
    }
    if reader.read(1)? == 0 {
        return Some(true);
    }
    *profile_bodies += 1;
    if *profile_bodies > MAX_PROFILES {
        return None;
    }
    let origin = reader.position;
    reader.skip(USER_SETTING_BITS)?;
    // Each normal gear block reserves all three five-bit skill slots.
    for base in [921, 1007, 1093] {
        let count = bits(payload, origin + base + 69, 2, end)? as usize;
        if !zero_bits(payload, origin + base + 71 + count * 5, origin + base + 86) {
            return None;
        }
    }
    let station_index = (handler - 2) as u8;
    if let Some(previous) = snapshots
        .iter()
        .find(|snapshot| snapshot.station_index == station_index)
    {
        // Compare the complete body privately. A changed unexported member
        // must not silently merge two conflicting snapshots of one station.
        if !(0..USER_SETTING_BITS).all(|offset| {
            bits(payload, previous.origin + offset, 1, end)
                == bits(payload, origin + offset, 1, end)
        }) {
            return None;
        }
    } else {
        snapshots.push(Snapshot {
            station_index,
            handler_start: start,
            origin,
        });
    }
    Some(true)
}

/// Atomic, bounded full-profile extraction. A supported display name is
/// optional; no NplnUserId, Identifier, or private station ID is exported.
/// None means unsupported or malformed framing, never partial profile bodies.
pub(crate) fn fields(payload: &[u8]) -> Option<Vec<Field>> {
    if payload.len() < 13
        || payload[0] != 7
        || payload[7] >= 24
        || usize::from(u16::from_be_bytes([payload[1], payload[2]])) + 8 != payload.len()
    {
        return None;
    }
    let header = (((usize::from(payload[7]) + 31) >> 3) & 0x3c) + 8;
    let end = payload.len().checked_mul(8)?;
    let mut snapshots = Vec::new();
    let mut profile_bodies = 0;
    let mut saw_station = false;
    let mut position = header.checked_mul(8)?;
    let mode = bits(payload, position, 1, end)?;
    position += 1;
    if mode == 0 {
        saw_station |= frame(payload, position, end, &mut snapshots, &mut profile_bodies)?;
    } else {
        while position < end {
            let remaining = end - position;
            if remaining <= 7 {
                if !zero_bits(payload, position, end) {
                    return None;
                }
                break;
            }
            if remaining < 44 {
                return None;
            }
            let length = bits(payload, position, 12, end)? as usize;
            position += 12;
            let frame_end = position.checked_add(length)?;
            if !(32..=4088).contains(&length) || frame_end > end {
                return None;
            }
            saw_station |= frame(
                payload,
                position,
                frame_end,
                &mut snapshots,
                &mut profile_bodies,
            )?;
            position = frame_end;
        }
    }
    if !saw_station {
        return None;
    }
    let field = |name: String, value: String, bits: Option<[usize; 2]>| Field { name, value, bits };
    let mut result = vec![
        field(
            "ProfileScope".into(),
            "StationInfoSupportedPrefix".into(),
            None,
        ),
        field("ProfileCount".into(), snapshots.len().to_string(), None),
    ];
    for (index, snapshot) in snapshots.iter().enumerate() {
        result.push(field(
            format!("Profile[{index}].StationIndex"),
            snapshot.station_index.to_string(),
            Some([snapshot.handler_start, snapshot.handler_start + 32]),
        ));
        if let Some(name) = display_name(payload, snapshot.origin, end) {
            result.push(field(
                format!("Profile[{index}].DisplayName"),
                name,
                Some([snapshot.origin + 256, snapshot.origin + 664]),
            ));
        }
        for (name, offset, width, bias) in [
            ("ModelType", 812, 3, 0),
            ("BynameAdjective", 1233, 17, -1),
            ("BynameSubject", 1250, 17, -1),
            ("BynameGender", 1267, 2, 0),
            ("Badge1", 1269, 32, 0),
            ("Badge2", 1301, 32, 0),
            ("Badge3", 1333, 32, 0),
            ("Background", 1365, 17, 0),
        ] {
            let start = snapshot.origin + offset;
            let value = bits(payload, start, width, end)? as i64 + bias;
            result.push(field(
                format!("Profile[{index}].{name}"),
                value.to_string(),
                Some([start, start + width]),
            ));
        }
    }
    Some(result)
}
