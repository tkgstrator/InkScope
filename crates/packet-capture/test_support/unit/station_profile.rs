use super::*;

fn write_bits(out: &mut Vec<bool>, value: u64, width: usize) {
    out.extend((0..width).map(|bit| value & (1 << bit) != 0));
}

fn profile_body(model: u64) -> Vec<bool> {
    let mut bytes = vec![0; 212];
    for (start, width, value) in [
        (812, 3, model),
        (1233, 17, 0),
        (1250, 17, 131071),
        (1267, 2, 3),
        (1269, 32, u32::MAX as u64),
        (1301, 32, 0),
        (1333, 32, 123),
        (1365, 17, 131071),
    ] {
        set_bits(&mut bytes, start, width, value);
    }
    (0..1695)
        .map(|bit| bytes[bit / 8] & (1 << (bit % 8)) != 0)
        .collect()
}

fn profile_with_name(name: &[u8]) -> Vec<bool> {
    assert!(name.len() <= 51);
    let mut body = profile_body(2);
    for (index, byte) in name.iter().enumerate() {
        for bit in 0..8 {
            body[256 + index * 8 + bit] = byte & (1 << bit) != 0;
        }
    }
    body
}

#[test]
fn station_profile_display_name_uses_the_dynamic_full_profile_origin() {
    for present in [[false; 6], [true; 6]] {
        for mode_one in [false, true] {
            let payload = station_payload(
                &[station_frame_with_slots(
                    3,
                    Some(profile_with_name("サンプル A".as_bytes())),
                    present,
                )],
                mode_one,
            );
            let row = station_row(&payload);
            assert_eq!(
                field_value(&row, "Profile[0].DisplayName"),
                Some("サンプル A")
            );
            let origin = if present[0] { 841 } else { 104 } + if mode_one { 12 } else { 0 };
            let name = row
                .fields
                .iter()
                .find(|f| f.name == "Profile[0].DisplayName")
                .unwrap();
            assert_eq!(name.bits, Some([origin + 256, origin + 664]));
            assert_eq!(row.layout.as_deref(), Some("type_fields"));
        }
    }
}

#[test]
fn station_profile_unsupported_names_do_not_fabricate_a_display_name() {
    let mut nonzero_tail = b"name\0x".to_vec();
    nonzero_tail.resize(51, 0);
    for name in [
        vec![],
        vec![b'x'; 51],
        nonzero_tail,
        vec![0xff],
        b"line\nbreak".to_vec(),
        "supplementary 🦑".as_bytes().to_vec(),
    ] {
        let row = station_row(&station_payload(
            &[station_frame(2, Some(profile_with_name(&name)))],
            false,
        ));
        assert_eq!(field_value(&row, "ProfileCount"), Some("1"));
        assert_eq!(field_value(&row, "Profile[0].ModelType"), Some("2"));
        assert_eq!(field_value(&row, "Profile[0].DisplayName"), None);
    }
}

fn roster_payload() -> Vec<u8> {
    let mut payload = vec![0; 130];
    payload[0] = 7;
    payload[1..3].copy_from_slice(&122u16.to_be_bytes());
    for (start, width, value) in [
        (65, 32, 1),
        (101, 1, 1),
        (102, 4, 10),
        (106, 64, 0x0123_4567_89ab_cdef),
        (170, 16, 17),
        (186, 1, 1),
        (349, 64, 0xfedc_ba98_7654_3210),
        (413, 16, 513),
        (429, 1, 1),
        // Nonempty but not active is native status2 and stays excluded.
        (430, 64, 0x1122_3344_5566_7788),
        (494, 16, 23),
        (916, 20, 2),
    ] {
        set_bits(&mut payload, start, width, value);
    }
    payload
}

#[test]
fn station_roster_extracts_only_the_exact_standalone_ten_entry_shape() {
    let fields = crate::station_profile::roster_fields(&roster_payload()).unwrap();
    let get = |name: &str| {
        fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.value.as_str())
    };
    assert_eq!(
        get("StationRosterScope"),
        Some("VersusSettingStationIdArrayStandalone")
    );
    assert_eq!(get("StationRosterCount"), Some("10"));
    assert_eq!(get("StationRosterActiveCount"), Some("2"));
    assert_eq!(
        get("StationRoster[0].StationConstant"),
        Some("81985529216486895")
    );
    assert_eq!(
        get("StationRoster[3].StationConstant"),
        Some("18364758544493064720")
    );
    assert_eq!(get("StationRoster[3].StationVariable"), Some("513"));
    assert_eq!(get("StationRoster[0].Active"), Some("true"));
    assert_eq!(get("StationRoster[3].Active"), Some("true"));
    assert_eq!(get("StationRoster[4].Active"), Some("false"));
    assert_eq!(get("StationRoster[9].StationConstant"), Some("0"));
    let name = fields
        .iter()
        .find(|f| f.name == "StationRoster[3].StationConstant")
        .unwrap();
    assert_eq!(name.bits, Some([349, 413]));
    assert!(fields.iter().all(|f| !["Npln", "Identifier", "Key", "Name"]
        .iter()
        .any(|s| f.name.contains(s))));
}

#[test]
fn station_roster_rejects_wrapper_count_presence_padding_and_identity_conflicts() {
    let good = roster_payload();
    for (start, width, value) in [
        (0, 8, 6),
        (16, 8, 0),
        (56, 8, 1),
        (64, 1, 1),
        (65, 32, 2),
        (97, 1, 1),
        (100, 1, 1),
        (101, 1, 0),
        (102, 4, 9),
        (102, 4, 11),
        (1033, 1, 1),
        (1039, 1, 1),
        // An otherwise empty entry may not carry a variable ID or active flag.
        (251, 16, 3),
        (267, 1, 1),
    ] {
        let mut bad = good.clone();
        set_bits(&mut bad, start, width, value);
        assert!(
            crate::station_profile::roster_fields(&bad).is_none(),
            "bit {start}"
        );
    }
    let mut duplicate = good.clone();
    set_bits(&mut duplicate, 349, 64, 0x0123_4567_89ab_cdef);
    set_bits(&mut duplicate, 413, 16, 17);
    assert!(crate::station_profile::roster_fields(&duplicate).is_none());
    for length in [0, 12, 129, 131] {
        let mut bad = good.clone();
        bad.resize(length, 0);
        assert!(crate::station_profile::roster_fields(&bad).is_none());
    }
}

fn session_roster_payload() -> Vec<u8> {
    let pairs = [
        (0x0123_4567_89ab_cdefu64, 17u16),
        (0xfedc_ba98_7654_3210, 513),
    ];
    let mut payload = vec![0; 27];
    payload[0] = 5;
    payload[3..7].copy_from_slice(&[1, 0, 0, 3]);
    payload[7..15].copy_from_slice(&pairs[0].0.to_be_bytes());
    payload[15..17].copy_from_slice(&pairs[0].1.to_be_bytes());
    payload[17] = 2;
    for ((constant, variable), name) in pairs.into_iter().zip([b"first".as_slice(), b"second"]) {
        let start = payload.len();
        payload.resize(start + 78, 0);
        payload[start..start + 8].copy_from_slice(&constant.to_be_bytes());
        payload[start + 8..start + 10].copy_from_slice(&variable.to_be_bytes());
        payload[start + 53] = 1;
        payload[start + 57..start + 73].fill(0xab); // Unexported opaque identity.
        payload[start + 73..start + 77].copy_from_slice(&(name.len() as u32).to_be_bytes());
        payload[start + 77] = 1;
        payload.extend(name);
    }
    payload
}

#[test]
fn session_roster_normalizes_private_pairs_and_never_exports_session_names_or_uids() {
    let fields = crate::station_profile::session_roster_fields(&session_roster_payload()).unwrap();
    let get = |name: &str| {
        fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.value.as_str())
    };
    assert_eq!(
        get("SessionRosterScope"),
        Some("Session5SingleFragmentStations")
    );
    assert_eq!(get("SessionRosterCount"), Some("2"));
    assert_eq!(
        get("SessionRoster[0].StationConstant"),
        Some("81985529216486895")
    );
    assert_eq!(
        get("SessionRoster[1].StationConstant"),
        Some("18364758544493064720")
    );
    assert_eq!(get("SessionRoster[1].StationVariable"), Some("513"));
    assert_eq!(fields.len(), 6);
    assert!(fields
        .iter()
        .all(|f| !f.name.contains("Name") && !f.name.contains("Uid")));
    let field = fields
        .iter()
        .find(|f| f.name == "SessionRoster[1].StationConstant")
        .unwrap();
    assert_eq!(field.bits, Some([110 * 8, 118 * 8]));
}

#[test]
fn session_roster_rejects_truncation_general_shapes_duplicate_pairs_and_trailing_data() {
    let good = session_roster_payload();
    for (offset, value) in [
        (0, 6),
        (3, 2),
        (6, 2),
        (17, 0),
        (17, 11),
        (18, 1),
        (27 + 53, 2),
        (27 + 77, 0),
    ] {
        let mut bad = good.clone();
        bad[offset] = value;
        assert!(
            crate::station_profile::session_roster_fields(&bad).is_none(),
            "offset {offset}"
        );
    }
    let mut oversized_name = good.clone();
    oversized_name[100..104].copy_from_slice(&257u32.to_be_bytes());
    let mut duplicate = good.clone();
    duplicate[110..120].copy_from_slice(&good[27..37]);
    let mut trailing = good.clone();
    trailing.push(0);
    let mut zero_identity = good.clone();
    zero_identity[27..37].fill(0);
    let mut truncated = good.clone();
    truncated.pop();
    for bad in [
        oversized_name,
        duplicate,
        trailing,
        zero_identity,
        truncated,
    ] {
        assert!(crate::station_profile::session_roster_fields(&bad).is_none());
    }
}

#[test]
fn session_roster_limits_ten_stations_and_two_hundred_fifty_six_name_bytes() {
    for length in [0usize, 256] {
        let mut payload = vec![0; 27];
        payload[0] = 5;
        payload[3..7].copy_from_slice(&[1, 0, 0, 3]);
        payload[7..15].copy_from_slice(&1u64.to_be_bytes());
        payload[15..17].copy_from_slice(&1u16.to_be_bytes());
        payload[17] = 10;
        for index in 0..10u16 {
            let start = payload.len();
            payload.resize(start + 78 + length, 0);
            payload[start..start + 8].copy_from_slice(&u64::from(index + 1).to_be_bytes());
            payload[start + 8..start + 10].copy_from_slice(&(index + 1).to_be_bytes());
            payload[start + 53] = 1;
            payload[start + 73..start + 77].copy_from_slice(&(length as u32).to_be_bytes());
            payload[start + 77] = 1;
            payload[start + 78..].fill(b'x');
        }
        let fields = crate::station_profile::session_roster_fields(&payload).unwrap();
        assert_eq!(
            fields
                .iter()
                .find(|f| f.name == "SessionRosterCount")
                .unwrap()
                .value,
            "10"
        );
        assert_eq!(fields.len(), 22);
    }
}

#[test]
fn station_roster_authenticated_rows_keep_private_pairs_in_raw_fields_and_type_classification() {
    let source = Ipv4Addr::new(192, 0, 2, 10);
    let peer = Ipv4Addr::new(192, 0, 2, 11);
    for (protocol, payload, scope, identity) in [
        (
            11,
            roster_payload(),
            "StationRosterScope",
            "StationRoster[3].StationConstant",
        ),
        (
            13,
            session_roster_payload(),
            "SessionRosterScope",
            "SessionRoster[1].StationConstant",
        ),
    ] {
        let packet = authenticated_test_packet(source, peer, 1, 1, protocol, Some(100), &payload);
        let row = Decoder::new(None, Some((peer, PiaKey([7; 16]))), None)
            .process(&packet, false)
            .remove(0);
        assert_eq!(row.status, "authenticated");
        assert_eq!(row.protocol, Some(protocol));
        assert_eq!(row.layout.as_deref(), Some("type_fields"));
        assert!(field_value(&row, scope).is_some());
        assert_eq!(field_value(&row, identity), Some("18364758544493064720"));
        assert!(!row
            .fields
            .iter()
            .any(|f| f.name.contains("Npln") || f.name.contains("Uid")));
    }
}

fn station_frame(handler: u64, body: Option<Vec<bool>>) -> Vec<bool> {
    station_frame_with_slots(handler, body, [false; 6])
}

fn station_frame_with_slots(
    handler: u64,
    body: Option<Vec<bool>>,
    present: [bool; 6],
) -> Vec<bool> {
    let mut frame = vec![];
    write_bits(&mut frame, handler, 32);
    for ((slot, width), present) in [3, 1, 1, 16, 20, 696].into_iter().enumerate().zip(present) {
        frame.push(present);
        if present {
            if slot == 5 {
                // Opaque StationNetState body with a nonzero final bit.
                frame.extend((0..696).map(|bit| bit % 11 == 0 || bit == 695));
            } else {
                write_bits(&mut frame, [5, 1, 0, 0x1234, 0xabcde][slot], width);
            }
        }
    }
    frame.push(body.is_some());
    if let Some(body) = body {
        frame.extend(body);
    }
    frame
}

#[test]
fn station_profile_dynamic_origin_handles_all_and_mixed_present_prefix_slots() {
    // Body origins are hand-counted from the native slot widths, independent
    // of the fixture helper:65wrapper+32handler+7presence+present body widths.
    for (present, origin) in [
        ([true; 6], 841),
        ([true, false, false, true, false, true], 819),
        ([false, true, true, false, true, false], 126),
        ([true, false, true, false, true, true], 824),
    ] {
        for mode_one in [false, true] {
            let row = station_row(&station_payload(
                &[station_frame_with_slots(4, Some(profile_body(5)), present)],
                mode_one,
            ));
            assert_eq!(field_value(&row, "ProfileCount"), Some("1"));
            assert_eq!(field_value(&row, "Profile[0].StationIndex"), Some("2"));
            assert_eq!(row.layout.as_deref(), Some("type_fields"));
            let origin = origin + if mode_one { 12 } else { 0 };
            for (name, value, offset, width) in [
                ("ModelType", "5", 812, 3),
                ("BynameAdjective", "-1", 1233, 17),
                ("BynameSubject", "131070", 1250, 17),
                ("Badge3", "123", 1333, 32),
                ("Background", "131071", 1365, 17),
            ] {
                let field = row
                    .fields
                    .iter()
                    .find(|field| field.name == format!("Profile[0].{name}"))
                    .unwrap();
                assert_eq!(field.value, value);
                assert_eq!(field.bits, Some([origin + offset, origin + offset + width]));
            }
        }
    }
}

#[test]
fn station_profile_dynamic_prefix_truncation_or_shift_rejects_the_chunk_atomically() {
    // In an all-present frame StationNetState starts at bit79, occupies696
    // bits, then the UserSetting presence flag is at bit775.
    let complete = station_frame_with_slots(3, Some(profile_body(5)), [true; 6]);
    let mut short_station_state = complete.clone();
    short_station_state.truncate(79 + 695);
    let mut missing_profile_presence = complete.clone();
    missing_profile_presence.truncate(775);
    let mut short_profile = complete.clone();
    short_profile.pop();
    let mut marked_body = profile_body(5);
    marked_body[0] = true;
    let mut shifted_last_slot = station_frame_with_slots(3, Some(marked_body), [true; 6]);
    shifted_last_slot.remove(79 + 695);
    for malformed in [
        short_station_state,
        missing_profile_presence,
        short_profile,
        shifted_last_slot,
    ] {
        let row = station_row(&station_payload(
            &[station_frame(2, Some(profile_body(0))), malformed],
            true,
        ));
        assert_eq!(field_value(&row, "ProfileScope"), None);
        assert_eq!(field_value(&row, "ProfileCount"), None);
        assert!(!row
            .fields
            .iter()
            .any(|field| field.name.starts_with("Profile[")));
        assert_ne!(row.layout.as_deref(), Some("confirmed_fields"));
    }
}

fn station_payload(frames: &[Vec<bool>], mode_one: bool) -> Vec<u8> {
    let mut wire = vec![false; 64];
    wire.push(mode_one);
    for frame in frames {
        if mode_one {
            write_bits(&mut wire, frame.len() as u64, 12);
        }
        wire.extend(frame);
    }
    let mut payload = vec![0; wire.len().div_ceil(8)];
    for (bit, value) in wire.into_iter().enumerate() {
        if value {
            payload[bit / 8] |= 1 << (bit % 8);
        }
    }
    payload[0] = 7;
    let length = (payload.len() - 8) as u16;
    payload[1..3].copy_from_slice(&length.to_be_bytes());
    payload
}

fn station_row(payload: &[u8]) -> Row {
    let source = Ipv4Addr::new(192, 0, 2, 10);
    let peer = Ipv4Addr::new(192, 0, 2, 11);
    Decoder::new(None, Some((peer, PiaKey([7; 16]))), None)
        .process(
            &authenticated_test_packet(source, peer, 1, 1, 11, Some(100), payload),
            false,
        )
        .remove(0)
}

#[test]
fn station_profile_exports_only_safe_fields_and_keeps_prefix_classification() {
    let row = station_row(&station_payload(
        &[station_frame(4, Some(profile_body(7)))],
        false,
    ));
    assert_eq!(row.layout.as_deref(), Some("type_fields"));
    assert_eq!(
        field_value(&row, "ProfileScope"),
        Some("StationInfoSupportedPrefix")
    );
    assert_eq!(field_value(&row, "ProfileCount"), Some("1"));
    for (name, expected) in [
        ("StationIndex", "2"),
        ("ModelType", "7"),
        ("BynameAdjective", "-1"),
        ("BynameSubject", "131070"),
        ("BynameGender", "3"),
        ("Badge1", "4294967295"),
        ("Badge2", "0"),
        ("Badge3", "123"),
        ("Background", "131071"),
    ] {
        assert_eq!(
            field_value(&row, &format!("Profile[0].{name}")),
            Some(expected)
        );
    }
    assert_eq!(
        row.fields
            .iter()
            .filter(|field| field.name.starts_with("Profile[0]."))
            .count(),
        9
    );
    assert!(!row.fields.iter().any(|field| {
        ["Name", "Identifier", "Npln", "StationId"]
            .iter()
            .any(|private| field.name.contains(private))
    }));
}

#[test]
fn station_profile_chunk_is_atomic_for_malformed_or_truncated_supported_frames() {
    let good = station_frame(2, Some(profile_body(0)));
    let mut truncated = profile_body(1);
    truncated.pop();
    let mut bad_padding = profile_body(1);
    bad_padding[1002] = true;
    let mut conflict = profile_body(0);
    conflict[1269] = false;
    let mut private_conflict = profile_body(0);
    private_conflict[0] = true;
    let mut incomplete_prefix = vec![];
    write_bits(&mut incomplete_prefix, 3, 32);
    write_bits(&mut incomplete_prefix, 0, 6);
    let frames = [
        vec![good.clone(), station_frame(3, Some(truncated))],
        vec![good.clone(), station_frame(3, Some(bad_padding))],
        vec![good.clone(), station_frame(2, Some(conflict))],
        vec![good.clone(), station_frame(2, Some(private_conflict))],
        vec![good.clone(), incomplete_prefix], // truncated StationInfo slot-presence prefix
    ];
    for frames in frames {
        let row = station_row(&station_payload(&frames, true));
        assert_eq!(field_value(&row, "ProfileScope"), None);
        assert_eq!(field_value(&row, "ProfileCount"), None);
        assert!(!row
            .fields
            .iter()
            .any(|field| field.name.starts_with("Profile[")));
        assert_ne!(row.layout.as_deref(), Some("confirmed_fields"));
    }
    let mut payload = station_payload(&[good], true);
    let last = payload.len() - 1;
    payload[last] |= 0x80;
    assert_eq!(field_value(&station_row(&payload), "ProfileScope"), None);
    payload[1] ^= 1;
    assert_eq!(field_value(&station_row(&payload), "ProfileScope"), None);
}

#[test]
fn station_profile_rejects_each_bad_wrapper_and_truncation_independently() {
    let good = station_payload(&[station_frame(2, Some(profile_body(0)))], false);
    for change in 0..4 {
        let mut payload = good.clone();
        match change {
            0 => payload[0] = 6,
            1 => payload[7] = 24,
            2 => payload[1] ^= 1,
            _ => {
                payload.pop();
                let length = (payload.len() - 8) as u16;
                payload[1..3].copy_from_slice(&length.to_be_bytes());
            },
        }
        assert_eq!(field_value(&station_row(&payload), "ProfileScope"), None);
    }
    let mut payload = station_payload(
        &[
            station_frame(2, Some(profile_body(0))),
            station_frame(3, None),
        ],
        true,
    );
    set_bits(&mut payload, 65 + 12 + 1734, 12, 4088);
    assert_eq!(field_value(&station_row(&payload), "ProfileScope"), None);
}

#[test]
fn station_profile_multiple_snapshots_are_bounded_and_identical_duplicates_coalesce() {
    let frames: Vec<_> = (2..12)
        .map(|handler| station_frame(handler, Some(profile_body(handler % 8))))
        .collect();
    let payload = station_payload(&frames, true);
    // Ten full bodies exceed the existing Pia datagram cap. Test the
    // extraction bound directly without weakening that separate guard.
    let fields = crate::station_profile::fields(&payload).expect("ten-profile extraction bound");
    for (name, want) in [("ProfileCount", "10"), ("Profile[9].StationIndex", "9")] {
        assert_eq!(
            fields
                .iter()
                .find(|field| field.name == name)
                .map(|field| field.value.as_str()),
            Some(want)
        );
    }
    let mut too_many = frames.clone();
    too_many.push(frames[0].clone());
    assert!(crate::station_profile::fields(&station_payload(&too_many, true)).is_none());
    let repeated = station_payload(&[frames[0].clone(), frames[0].clone()], true);
    assert_eq!(
        field_value(&station_row(&repeated), "ProfileCount"),
        Some("1")
    );
}

#[test]
fn station_profile_preserves_absence_and_unrelated_protocol11_classification() {
    let absent = station_row(&station_payload(&[station_frame(3, None)], false));
    assert_eq!(field_value(&absent, "ProfileCount"), Some("0"));
    assert_eq!(absent.layout.as_deref(), Some("type_fields"));
    let unrelated = station_row(&station_payload(&[station_frame(30, None)], false));
    assert_eq!(field_value(&unrelated, "ProfileScope"), None);
    assert_eq!(unrelated.layout.as_deref(), Some("type_only"));
}

#[test]
fn observed_ikura_scalars_require_the_complete_two_count_four_state_shape() {
    let mut payload = vec![0; 30];
    for (start, width, value) in [
        (0, 1, 1),
        (1, 12, 222),
        (13, 32, 1108),
        (45, 1, 1),
        (46, 14, 16383),
        (60, 12, 4095),
        (72, 3, 4),
        (123, 3, 4),
    ] {
        set_bits(&mut payload, start, width, value);
    }
    let fields = semantic_fields(&Chunk {
        protocol: 5,
        port: Some(101),
        payload: payload.clone(),
    });
    for (name, value) in [("IkuraNum", "16383"), ("HeldGoldenIkuraNum", "4095")] {
        assert_eq!(
            fields
                .iter()
                .find(|field| field.name == name)
                .map(|field| field.value.as_str()),
            Some(value)
        );
    }
    for (start, width, value) in [(1, 12, 221), (45, 1, 0), (72, 3, 3), (123, 3, 3)] {
        let mut malformed = payload.clone();
        set_bits(&mut malformed, start, width, value);
        assert!(!semantic_fields(&Chunk {
            protocol: 5,
            port: Some(101),
            payload: malformed
        })
        .iter()
        .any(|field| matches!(field.name.as_str(), "IkuraNum" | "HeldGoldenIkuraNum")));
    }
    payload.truncate(29);
    assert!(!semantic_fields(&Chunk {
        protocol: 5,
        port: Some(101),
        payload
    })
    .iter()
    .any(|field| field.name == "IkuraNum"));
}

#[test]
fn rescued_index_is_signed_and_requires_the_exact_known_footer() {
    let mut payload = vec![0; 29];
    payload[0] = 7;
    payload[1..3].copy_from_slice(&21u16.to_be_bytes());
    payload[13..15].copy_from_slice(&12u16.to_be_bytes());
    payload[21..25].copy_from_slice(&0xcf9be194u32.to_le_bytes());
    for (wire, want) in [(0, "-1"), (1, "0"), (4, "3"), (15, "14")] {
        set_bits(&mut payload, 200, 4, wire);
        let fields = semantic_fields(&Chunk {
            protocol: 8,
            port: Some(100),
            payload: payload.clone(),
        });
        assert_eq!(
            fields
                .iter()
                .find(|field| field.name == "RescuerIndex")
                .map(|field| field.value.as_str()),
            Some(want)
        );
    }
    for (start, width, value) in [(231, 1, 1), (56, 8, 1), (8, 16, 0)] {
        let mut malformed = payload.clone();
        set_bits(&mut malformed, start, width, value);
        assert!(!semantic_fields(&Chunk {
            protocol: 8,
            port: Some(100),
            payload: malformed
        })
        .iter()
        .any(|field| field.name == "RescuerIndex"));
    }
}
