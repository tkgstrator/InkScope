use super::*;
use aes::Aes128;
use aes_gcm::aead::{
    AeadInOut, KeyInit,
    consts::{U8, U12},
};
use aes_gcm::{AesGcm, Nonce};

#[path = "station_profile.rs"]
mod station_profile;

#[path = "obs_nested_stats.rs"]
mod obs_nested_stats;

fn protocol_fixture(name: &str) -> Vec<u8> {
    let source: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/protocol-payloads-v1.json")).unwrap();
    let entry = source["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap();
    hex::decode(entry["payload_hex"].as_str().unwrap()).unwrap()
}

fn authenticated_test_packet(
    source: Ipv4Addr,
    destination: Ipv4Addr,
    record: u64,
    timestamp_ns: u64,
    protocol: u8,
    port: Option<u8>,
    payload: &[u8],
) -> UdpPacket {
    let mut body = vec![if port.is_some() { 0x0e } else { 0x06 }];
    body.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    body.push(protocol);
    if let Some(port) = port {
        body.push(port);
    }
    body.extend_from_slice(payload);
    let mut wire = vec![0u8; 29];
    wire[..5].copy_from_slice(b"\x32\xab\x98\x64\x8f");
    let padding = (16 - body.len() % 16) % 16;
    wire[5] = (padding as u8) << 4;
    wire[10..12].copy_from_slice(&(record as u16).to_be_bytes());
    wire[13..21].copy_from_slice(&[9; 8]);
    body.resize(body.len() + padding, 0xff);
    let mut nonce = [0u8; 12];
    nonce[..4].copy_from_slice(&source.octets());
    nonce[4..].copy_from_slice(&[9; 8]);
    let cipher = AesGcm::<Aes128, U12, U8>::new_from_slice(&[7; 16]).unwrap();
    let tag = cipher
        .encrypt_inout_detached(&Nonce::from(nonce), b"", body.as_mut_slice().into())
        .unwrap();
    wire[21..29].copy_from_slice(&tag);
    wire.extend(body);
    UdpPacket {
        record,
        timestamp_ns,
        source,
        destination,
        source_port: 49152,
        destination_port: 49152,
        source_mac: [0; 6],
        destination_mac: [0; 6],
        payload: wire,
    }
}

#[test]
fn authenticated_reverse_rtt_response_reports_capture_interval() {
    let client = Ipv4Addr::new(192, 0, 2, 10);
    let server = Ipv4Addr::new(192, 0, 2, 11);
    let mut decoder = Decoder::new(None, Some((server, PiaKey([7; 16]))), None);
    let request = authenticated_test_packet(
        client,
        server,
        8,
        1_000_000_000,
        3,
        None,
        &protocol_fixture("rttRequest"),
    );
    let response = authenticated_test_packet(
        server,
        client,
        9,
        1_042_600_000,
        3,
        None,
        &protocol_fixture("rtt"),
    );
    let request_rows = decoder.process(&request, false);
    assert_eq!(request_rows[0].status, "authenticated");
    assert_eq!(
        field_value(&request_rows[0], "ElapsedMicroseconds"),
        Some("3595950592")
    );
    assert_eq!(
        field_value(&request_rows[0], "DestinationVariableId"),
        Some("0")
    );
    assert_eq!(
        field_value(&request_rows[0], "ProtocolStationVariableId"),
        Some("29460")
    );
    let rows = decoder.process(&response, false);
    assert_eq!(rows[0].kind, "RTTProtocol");
    assert_eq!(
        field_value(&rows[0], "OriginTick"),
        field_value(&request_rows[0], "OriginTick")
    );
    assert_eq!(field_value(&rows[0], "ElapsedMicroseconds"), Some("80310"));
    assert_eq!(
        field_value(&rows[0], "DestinationVariableId"),
        Some("29460")
    );
    assert_eq!(
        field_value(&rows[0], "ProtocolStationVariableId"),
        Some("29460")
    );
    assert!(
        rows[0]
            .fields
            .iter()
            .any(|field| field.name == "CaptureResponseDelayMs" && field.value == "42.600")
    );
}

#[test]
fn authenticated_private_setting_shapes_expose_stage_and_internal_difficulty() {
    let host = Ipv4Addr::new(192, 0, 2, 11);
    let client = Ipv4Addr::new(192, 0, 2, 10);
    let mut decoder = Decoder::new(None, Some((host, PiaKey([7; 16]))), None);
    for (name, record, stage, difficulty) in [
        ("privateCommon", 295, Some("Cop_Shakeup"), None),
        ("privateCoop", 419, None, Some("25")),
        ("privateCombined", 479, Some("Cop_Shakeup"), Some("775")),
    ] {
        let packet = authenticated_test_packet(
            host,
            client,
            record,
            record * 1_000_000,
            11,
            Some(100),
            &protocol_fixture(name),
        );
        let rows = decoder.process(&packet, false);
        assert_eq!(rows[0].kind, "PrivateSetting", "{name}");
        assert_eq!(
            rows[0]
                .fields
                .iter()
                .find(|field| field.name == "StageId")
                .map(|field| field.value.as_str()),
            stage,
            "{name}"
        );
        assert_eq!(
            rows[0]
                .fields
                .iter()
                .find(|field| field.name == "Difficulty")
                .map(|field| field.value.as_str()),
            difficulty,
            "{name}"
        );
        if name == "privateCombined" {
            let wire = rows[0]
                .fields
                .iter()
                .find(|field| field.name == "DifficultyWire")
                .unwrap();
            assert_eq!(wire.value, "776");
            assert_eq!(wire.bits, Some([617, 628]));
        }
    }
}

fn field_value<'a>(row: &'a Row, name: &str) -> Option<&'a str> {
    row.fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.value.as_str())
}

fn set_bits(payload: &mut [u8], start: usize, width: usize, value: u64) {
    for offset in 0..width {
        let byte = &mut payload[(start + offset) / 8];
        let mask = 1 << ((start + offset) % 8);
        *byte = (*byte & !mask) | ((((value >> offset) & 1) as u8) * mask);
    }
}

fn equipment_setting_payload() -> Vec<u8> {
    let mut payload = vec![0; 211];
    payload[0] = 7;
    payload[1..3].copy_from_slice(&203u16.to_be_bytes());
    payload[13..15].copy_from_slice(&194u16.to_be_bytes());
    payload[21..25].copy_from_slice(&0x9a1e036du32.to_le_bytes());
    payload[25..38].copy_from_slice(b"Cop_Shakedent");
    set_bits(&mut payload, 712, 3, 1);
    set_bits(&mut payload, 725, 3, 4);
    for (index, wire) in [2, 22033, 0, 65535].into_iter().enumerate() {
        set_bits(&mut payload, 728 + index * 16, 16, wire);
    }
    set_bits(&mut payload, 1064, 11, 151);
    for index in 0..4 {
        set_bits(&mut payload, 1075 + index * 32, 32, index as u64 + 1);
    }
    set_bits(&mut payload, 1559, 32, 0x000b0300);
    set_bits(&mut payload, 1592, 3, 1);
    set_bits(&mut payload, 1595, 16, 2);
    payload
}

fn equipment_setting_row(payload: &[u8]) -> Row {
    let host = Ipv4Addr::new(192, 0, 2, 11);
    let client = Ipv4Addr::new(192, 0, 2, 10);
    Decoder::new(None, Some((host, PiaKey([7; 16]))), None)
        .process(
            &authenticated_test_packet(host, client, 1, 1, 8, Some(100), payload),
            false,
        )
        .remove(0)
}

#[test]
fn equipment_setting_exposes_signed_sentinels_and_version_atomically() {
    let row = equipment_setting_row(&equipment_setting_payload());
    assert_eq!(row.layout.as_deref(), Some("confirmed_fields"));
    for (name, expected) in [
        ("UnlockedSeason", "-1"),
        ("WeaponIdCount", "4"),
        ("WeaponId[0]", "-1"),
        ("WeaponId[1]", "22030"),
        ("WeaponId[2]", "-3"),
        ("WeaponId[3]", "65532"),
        ("ScenarioAppVersion", "721664"),
        ("ScenarioCompetitionType", "0"),
        ("ScenarioOriginalWeaponIdCount", "1"),
        ("ScenarioOriginalWeaponId[0]", "-1"),
    ] {
        assert_eq!(field_value(&row, name), Some(expected), "{name}");
    }
    assert_eq!(field_value(&row, "ScenarioOriginalWeaponId[1]"), None);
}

#[test]
fn equipment_setting_accepts_zero_through_four_logical_slots_with_zero_unused_padding() {
    for count in 0..=4 {
        let mut payload = equipment_setting_payload();
        set_bits(&mut payload, 715, 10, 1023);
        set_bits(&mut payload, 725, 3, count);
        set_bits(&mut payload, 1592, 3, count);
        for index in 0..4 {
            let wire = if index < count { 22033 } else { 0 };
            set_bits(&mut payload, 728 + index as usize * 16, 16, wire);
            set_bits(&mut payload, 1595 + index as usize * 16, 16, wire);
        }
        let row = equipment_setting_row(&payload);
        assert_eq!(row.layout.as_deref(), Some("confirmed_fields"));
        assert_eq!(field_value(&row, "UnlockedSeason"), Some("1022"));
        assert_eq!(
            field_value(&row, "WeaponIdCount"),
            Some(count.to_string().as_str())
        );
        for index in 0..4 {
            for prefix in ["WeaponId", "ScenarioOriginalWeaponId"] {
                assert_eq!(
                    field_value(&row, &format!("{prefix}[{index}]")),
                    (index < count).then_some("22030")
                );
            }
        }
    }
}

#[test]
fn malformed_equipment_shape_never_looks_complete_or_exposes_partial_metadata() {
    for (start, width, wire) in [
        (725, 3, 5),
        (1592, 3, 5),
        (792, 4, 1),
        (1395, 3, 1),
        (1428, 3, 1),
        (796, 1, 1),
        (1051, 1, 1),
        (1398, 1, 1),
        (1427, 1, 1),
        (1431, 1, 1),
        (1558, 1, 1),
        (1611, 1, 1),
        (1658, 1, 1),
        (1686, 1, 1),
        (1687, 1, 1),
        (725, 3, 3), // the fourth nonzero slot becomes invalid unused padding
    ] {
        let mut payload = equipment_setting_payload();
        set_bits(&mut payload, start, width, wire);
        let row = equipment_setting_row(&payload);
        assert_eq!(row.layout.as_deref(), Some("type_fields"), "bit {start}");
        for name in [
            "UnlockedSeason",
            "WeaponIdCount",
            "ScenarioAppVersion",
            "GameFrame",
        ] {
            assert_eq!(field_value(&row, name), None, "{name}, bit {start}");
        }
        assert!(field_value(&row, "GameSeed[0]").is_some());
    }
    let mut truncated = equipment_setting_payload();
    truncated.pop();
    let row = equipment_setting_row(&truncated);
    assert_ne!(row.layout.as_deref(), Some("confirmed_fields"));
    assert_eq!(field_value(&row, "WeaponIdCount"), None);
}

#[test]
fn rtt_match_is_directional_authenticated_and_capture_time_bounded() {
    let client = Ipv4Addr::new(192, 0, 2, 10);
    let server = Ipv4Addr::new(192, 0, 2, 11);
    let request = protocol_fixture("rttRequest");
    let response = protocol_fixture("rtt");
    let mut decoder = Decoder::new(None, Some((server, PiaKey([7; 16]))), None);
    decoder.process(
        &authenticated_test_packet(client, server, 1, 1_000_000_000, 3, None, &request),
        false,
    );
    let same_direction = decoder.process(
        &authenticated_test_packet(client, server, 2, 1_010_000_000, 3, None, &response),
        false,
    );
    assert_eq!(
        field_value(&same_direction[0], "CaptureResponseDelayMs"),
        None
    );
    let mut unauthenticated =
        authenticated_test_packet(server, client, 3, 1_020_000_000, 3, None, &response);
    unauthenticated.payload[21] ^= 0x80;
    assert_eq!(
        decoder.process(&unauthenticated, false)[0].status,
        "authentication_failed"
    );
    let matched = decoder.process(
        &authenticated_test_packet(server, client, 4, 1_042_600_000, 3, None, &response),
        false,
    );
    assert_eq!(
        field_value(&matched[0], "CaptureResponseDelayMs"),
        Some("42.600")
    );
    let duplicate_response = decoder.process(
        &authenticated_test_packet(server, client, 5, 1_050_000_000, 3, None, &response),
        false,
    );
    assert_eq!(
        field_value(&duplicate_response[0], "CaptureResponseDelayMs"),
        None
    );

    decoder.process(
        &authenticated_test_packet(client, server, 6, 3_000_000_000, 3, None, &request),
        false,
    );
    let backwards = decoder.process(
        &authenticated_test_packet(server, client, 7, 2_999_999_999, 3, None, &response),
        false,
    );
    assert_eq!(field_value(&backwards[0], "CaptureResponseDelayMs"), None);
    let stale = decoder.process(
        &authenticated_test_packet(server, client, 8, 5_000_000_001, 3, None, &response),
        false,
    );
    assert_eq!(field_value(&stale[0], "CaptureResponseDelayMs"), None);
}

#[test]
fn rtt_duplicate_request_replaces_time_and_pending_is_bounded_and_cleared() {
    let client = Ipv4Addr::new(192, 0, 2, 10);
    let server = Ipv4Addr::new(192, 0, 2, 11);
    let request = protocol_fixture("rttRequest");
    let response = protocol_fixture("rtt");
    let mut decoder = Decoder::new(None, Some((server, PiaKey([7; 16]))), None);
    decoder.process(
        &authenticated_test_packet(client, server, 1, 1_000_000_000, 3, None, &request),
        false,
    );
    decoder.process(
        &authenticated_test_packet(client, server, 2, 1_025_000_000, 3, None, &request),
        false,
    );
    let matched = decoder.process(
        &authenticated_test_packet(server, client, 3, 1_042_600_000, 3, None, &response),
        false,
    );
    assert_eq!(
        field_value(&matched[0], "CaptureResponseDelayMs"),
        Some("17.600")
    );

    for index in 0..=RTT_PENDING_LIMIT {
        let mut payload = request.clone();
        payload[1..9].copy_from_slice(&(index as u64 + 100).to_be_bytes());
        decoder.process(
            &authenticated_test_packet(
                client,
                server,
                10 + index as u64,
                1_100_000_000 + index as u64,
                3,
                None,
                &payload,
            ),
            false,
        );
    }
    assert_eq!(decoder.rtt_pending.len(), RTT_PENDING_LIMIT);
    assert!(!decoder.rtt_pending.contains_key(&(client, server, 100)));
    decoder.replace_game_key(None);
    assert!(decoder.rtt_pending.is_empty());
}

#[test]
fn private_setting_rejects_malformed_layouts_and_keeps_zero_wire_unavailable() {
    for name in ["privateCommon", "privateCoop", "privateCombined"] {
        let payload = protocol_fixture(name);
        let valid = Chunk {
            protocol: 11,
            port: Some(100),
            payload: payload.clone(),
        };
        assert_eq!(describe_chunk(&valid), "PrivateSetting", "{name}");
        let mut wrong_port = Chunk {
            protocol: 11,
            port: Some(99),
            payload: payload.clone(),
        };
        assert_eq!(
            describe_chunk(&wrong_port),
            "BroadcastReliableProtocol",
            "{name} port"
        );
        wrong_port.port = Some(100);
        wrong_port.payload[2] ^= 1;
        assert_eq!(
            describe_chunk(&wrong_port),
            "BroadcastReliableProtocol",
            "{name} length"
        );
        let mut wrong_presence = payload.clone();
        set_bits(&mut wrong_presence, 97, 1, 1);
        assert_eq!(
            describe_chunk(&Chunk {
                protocol: 11,
                port: Some(100),
                payload: wrong_presence
            }),
            "BroadcastReliableProtocol",
            "{name} presence"
        );
    }
    for (name, bit) in [
        ("privateCommon", 613),
        ("privateCommon", 616),
        ("privateCommon", 617),
        ("privateCommon", 618),
        ("privateCoop", 1287),
        ("privateCombined", 615),
        ("privateCombined", 616),
        ("privateCombined", 1802),
        ("privateCombined", 1807),
    ] {
        let mut payload = protocol_fixture(name);
        payload[bit / 8] ^= 1 << (bit % 8);
        assert_eq!(
            describe_chunk(&Chunk {
                protocol: 11,
                port: Some(100),
                payload
            }),
            "BroadcastReliableProtocol",
            "{name} bit {bit}"
        );
    }
    let mut coop = protocol_fixture("privateCoop");
    set_bits(&mut coop, 102, 11, 0);
    let zero = Chunk {
        protocol: 11,
        port: Some(100),
        payload: coop,
    };
    assert_eq!(describe_chunk(&zero), "PrivateSetting");
    assert_eq!(
        semantic_fields(&zero)
            .iter()
            .find(|field| field.name == "DifficultyWire")
            .map(|field| field.value.as_str()),
        Some("0")
    );
    assert!(
        !semantic_fields(&zero)
            .iter()
            .any(|field| field.name == "Difficulty")
    );
    let mut common = protocol_fixture("privateCommon");
    // Clear the 512-bit fixed string byte by byte.
    for index in 0..64 {
        set_bits(&mut common, 100 + index * 8, 8, 0);
    }
    let empty = Chunk {
        protocol: 11,
        port: Some(100),
        payload: common,
    };
    assert_eq!(describe_chunk(&empty), "PrivateSetting");
    assert_eq!(
        semantic_fields(&empty)
            .iter()
            .find(|field| field.name == "StageId")
            .map(|field| field.value.as_str()),
        Some("")
    );
}

#[test]
fn rtt_protocol_uses_pia_632_display_name() {
    let chunk = Chunk {
        protocol: 3,
        port: None,
        payload: vec![0; 21],
    };
    assert_eq!(describe_chunk(&chunk), "RTTProtocol");
}

#[test]
fn unreliable_coop_groups_use_documented_state_type_names() {
    for (group, name) in [
        (1104u32, "CoopSequenceNetState"),
        (1107, "CoopSeqPlayerNetState"),
        (1108, "CoopSeqPlayerNetState"),
    ] {
        let mut payload = vec![0u8; 16];
        let header = 1u64 | (u64::from(group) << 13) | (1u64 << 45);
        payload[..8].copy_from_slice(&header.to_le_bytes());
        let chunk = Chunk {
            protocol: 5,
            port: None,
            payload,
        };
        assert_eq!(describe_chunk(&chunk), name);
    }
    let chunk = Chunk {
        protocol: 5,
        port: None,
        payload: vec![0; 16],
    };
    assert_eq!(describe_chunk(&chunk), "UnreliableProtocol");
}

#[test]
fn authenticated_coop_player_state_exposes_two_unnamed_four_value_arrays() {
    let source = Ipv4Addr::new(192, 0, 2, 10);
    let peer = Ipv4Addr::new(192, 0, 2, 11);
    let mut decoder = Decoder::new(None, Some((peer, PiaKey([7; 16]))), None);
    for (group, payload_bytes) in [(1107, 30), (1108, 30), (1107, 109), (1108, 109)] {
        let mut payload = vec![0u8; payload_bytes];
        for (start, width, value) in [
            (0, 1, 1),
            (1, 12, 222),
            (13, 32, group),
            (45, 1, 1),
            (72, 3, 4),
            (123, 3, 4),
        ] {
            set_bits(&mut payload, start, width, value);
        }
        for (index, value) in [0, 1, 2048, 4095].into_iter().enumerate() {
            set_bits(&mut payload, 75 + index * 12, 12, value);
        }
        for (index, value) in [4095, 3072, 2, 0].into_iter().enumerate() {
            set_bits(&mut payload, 126 + index * 12, 12, value);
        }
        let packet = authenticated_test_packet(source, peer, group, group, 5, None, &payload);
        let rows = decoder.process(&packet, false);
        assert_eq!(rows[0].status, "authenticated");
        assert_eq!(rows[0].kind, "CoopSeqPlayerNetState");
        for (name, expected, span) in [
            ("ArrayA[0]", "0", [75, 87]),
            ("ArrayA[1]", "1", [87, 99]),
            ("ArrayA[2]", "2048", [99, 111]),
            ("ArrayA[3]", "4095", [111, 123]),
            ("ArrayB[0]", "4095", [126, 138]),
            ("ArrayB[1]", "3072", [138, 150]),
            ("ArrayB[2]", "2", [150, 162]),
            ("ArrayB[3]", "0", [162, 174]),
        ] {
            let field = rows[0]
                .fields
                .iter()
                .find(|field| field.name == name)
                .unwrap();
            assert_eq!(field.value, expected);
            assert_eq!(field.bits, Some(span));
        }
    }
}

#[test]
fn coop_player_arrays_require_the_confirmed_authenticated_frame_shape() {
    let source = Ipv4Addr::new(192, 0, 2, 10);
    let peer = Ipv4Addr::new(192, 0, 2, 11);
    let mut valid = vec![0u8; 109];
    for (start, width, value) in [
        (0, 1, 1),
        (1, 12, 222),
        (13, 32, 1108),
        (45, 1, 1),
        (72, 3, 4),
        (123, 3, 4),
        (87, 12, 9),
        (138, 12, 22),
    ] {
        set_bits(&mut valid, start, width, value);
    }
    for (start, width, value) in [(0, 1, 0), (1, 12, 221), (45, 1, 0), (72, 3, 3), (123, 3, 3)] {
        let mut payload = valid.clone();
        set_bits(&mut payload, start, width, value);
        let chunk = Chunk {
            protocol: 5,
            port: None,
            payload,
        };
        assert!(!semantic_fields(&chunk).iter().any(|field| field.name.starts_with("ArrayA[") || field.name.starts_with("ArrayB[")));
    }
    let chunk = Chunk {
        protocol: 5,
        port: None,
        payload: valid[..29].to_vec(),
    };
    assert!(
        !semantic_fields(&chunk)
            .iter()
            .any(|field| field.name.starts_with("ArrayA[") || field.name.starts_with("ArrayB["))
    );
    let mut decoder = Decoder::new(None, Some((peer, PiaKey([7; 16]))), None);
    let mut packet = authenticated_test_packet(source, peer, 1, 1, 5, None, &valid);
    packet.payload[21] ^= 0x80;
    let rows = decoder.process(&packet, false);
    assert_ne!(rows[0].status, "authenticated");
    assert!(
        !rows
            .iter()
            .flat_map(|row| &row.fields)
            .any(|field| field.name.starts_with("ArrayA[") || field.name.starts_with("ArrayB["))
    );
}

#[test]
fn player_state_candidate_heading_matches_signed_horizontal_wire_angle() {
    // PlayerNetState body begins at 280 after the 222-bit CoopSeq frame and
    // a 619-bit group 200000 frame header. Heading occupies body bits 159..172.
    for raw in [0u64, 2048, 4095, 4096, 6144, 8191] {
        let mut payload = vec![0u8; 109];
        for (start, width, value) in [
            (0, 1, 1),
            (1, 12, 222),
            (13, 32, 1107),
            (45, 1, 1),
            (72, 3, 4),
            (123, 3, 4),
            (235, 12, 619),
            (247, 32, 200000),
            (279, 1, 1),
            (439, 13, raw),
        ] {
            set_bits(&mut payload, start, width, value);
        }
        let fields = semantic_fields(&Chunk {
            protocol: 5,
            port: Some(101),
            payload,
        });
        let find = |name: &str| fields.iter().find(|entry| entry.name == name).unwrap();
        assert_eq!(find("PlayerStateHorizontalAngleRaw").value, raw.to_string());
        assert_eq!(find("PlayerStateHorizontalAngleRaw").bits, Some([439, 452]));
        let sign = if raw & 0x1000 == 0 { 1.0 } else { -1.0 };
        let radians = sign * (raw & 0xfff) as f64 / 4095.0 * std::f64::consts::PI;
        for (name, expected) in [
            ("PlayerStateHorizontalAngleDeg", radians.to_degrees()),
            ("PlayerStateDirectionX", radians.sin()),
            ("PlayerStateDirectionZ", radians.cos()),
        ] {
            let actual = find(name).value.parse::<f64>().unwrap();
            assert!((actual - expected).abs() < 0.000001, "raw={raw} {name}");
        }
        assert_eq!(find("PlayerStateDirectionMeaning").value, "Unknown");
    }
}

#[test]
fn player_state_camera_pitch_and_form_match_owned_wire_codec_boundaries() {
    // A wrong body offset, sign/magnitude decode, missing f32 rounding, or
    // swapped Human/Squid selector must change the consumer-visible fields.
    for (raw, expected) in [
        (0, "-1.000000000"),
        (1, "-0.999511600"),
        (14, "-0.993162394"),
        (1024, "-0.499877900"),
        (2047, "-0.000244200"),
        (2048, "0.000244200"),
        (3071, "0.499877900"),
        (4094, "0.999511600"),
        (4095, "1.000000000"),
    ] {
        for (group, appended_group) in [(1107, 200000), (1108, 210000)] {
            for (form_bit, form_name) in [(0, "Squid"), (1, "Human")] {
                let mut payload = vec![0u8; 109];
                for (start, width, value) in [
                    (0, 1, 1),
                    (1, 12, 222),
                    (13, 32, group),
                    (45, 1, 1),
                    (72, 3, 4),
                    (123, 3, 4),
                    (235, 12, 619),
                    (247, 32, appended_group),
                    (279, 1, 1),
                    (452, 12, raw),
                    (598, 1, form_bit),
                ] {
                    set_bits(&mut payload, start, width, value);
                }
                let fields = semantic_fields(&Chunk {
                    protocol: 5,
                    port: Some(101),
                    payload,
                });
                for (name, value, span) in [
                    ("PlayerStateCameraPitchRaw", raw.to_string(), [452, 464]),
                    (
                        "PlayerStateCameraPitchControl",
                        expected.to_string(),
                        [452, 464],
                    ),
                    ("PlayerStateForm", form_name.to_string(), [598, 599]),
                ] {
                    let actual = fields.iter().find(|field| field.name == name).unwrap();
                    assert_eq!(actual.value, value, "group={group} raw={raw} {name}");
                    assert_eq!(actual.bits, Some(span), "{name}");
                }
            }
        }
    }
}

#[test]
fn player_state_candidate_axes_require_the_exact_standalone_frame_and_tail() {
    for (group, marker, resource_group) in
        [(1107, 0x800186a0, "200000"), (1108, 0x80019a28, "210000")]
    {
        let mut payload = vec![0u8; 109];
        for (start, width, value) in [
            (0, 1, 1),
            (1, 12, 222),
            (13, 32, group),
            (45, 1, 1),
            (72, 3, 4),
            (123, 3, 4),
            (235, 1, 1),
            (236, 12, 309),
            (248, 32, marker),
            (280, 17, 0x8000),
            (297, 17, 0x14000),
            (314, 17, 0),
        ] {
            set_bits(&mut payload, start, width, value);
        }
        let chunk = Chunk {
            protocol: 5,
            port: Some(101),
            payload: payload.clone(),
        };
        let fields = semantic_fields(&chunk);
        for (name, value, span) in [
            ("PlayerStateX", "128.002", [280, 297]),
            ("PlayerStateY", "-64.001", [297, 314]),
            ("PlayerStateZ", "0.000", [314, 331]),
            ("PlayerStateGroupId", resource_group, [247, 279]),
            ("PlayerStateMatchingValue", "-1", [584, 589]),
        ] {
            let actual = fields.iter().find(|field| field.name == name).unwrap();
            assert_eq!(actual.value, value);
            assert_eq!(actual.bits, Some(span));
        }
        for (start, width, bad) in [
            (0, 1, 0),
            (1, 12, 221),
            (45, 1, 0),
            (72, 3, 3),
            (123, 3, 3),
            (235, 1, 0),
            (236, 12, 308),
            (248, 32, marker ^ 1),
            (247, 32, 200001),
            (279, 1, 0),
            (866, 6, 1),
        ] {
            let mut invalid = payload.clone();
            set_bits(&mut invalid, start, width, bad);
            assert!(
                !semantic_fields(&Chunk {
                    protocol: 5,
                    port: Some(101),
                    payload: invalid
                })
                .iter()
                .any(|field| field.name.starts_with("PlayerState")),
                "group={group} start={start}"
            );
        }
        assert!(
            !semantic_fields(&Chunk {
                protocol: 5,
                port: Some(101),
                payload: payload[..108].to_vec()
            })
            .iter()
            .any(|field| field.name.starts_with("PlayerState"))
        );
        assert!(
            !semantic_fields(&Chunk {
                protocol: 5,
                port: Some(100),
                payload
            })
            .iter()
            .any(|field| field.name.starts_with("PlayerState"))
        );
    }
}

#[test]
fn player_state_candidate_axes_in_host_composite_require_nested_frame() {
    for payload_bytes in [118, 138] {
        let mut payload = vec![0u8; payload_bytes];
        for (start, width, value) in [
            (0, 1, 1),
            (1, 12, 63),
            (13, 32, 1104),
            (45, 1, 1),
            (75, 1, 0),
            (76, 12, 222),
            (88, 32, 1107),
            (120, 1, 1),
            (147, 3, 4),
            (198, 3, 4),
            (310, 1, 1),
            (311, 12, 309),
            (323, 32, 0x800186a0),
            (355, 17, 0x8000),
            (372, 17, 0x14000),
            (389, 17, 0),
            (452, 12, 0),
            (598, 1, 0),
            (514, 13, 8191),
            (527, 12, 4095),
            (673, 1, 1),
        ] {
            set_bits(&mut payload, start, width, value);
        }
        let fields = semantic_fields(&Chunk {
            protocol: 5,
            port: Some(101),
            payload: payload.clone(),
        });
        for (name, value, span) in [
            ("PlayerStateX", "128.002", [355, 372]),
            ("PlayerStateY", "-64.001", [372, 389]),
            ("PlayerStateZ", "0.000", [389, 406]),
            ("PlayerStateHorizontalAngleRaw", "8191", [514, 527]),
            ("PlayerStateHorizontalAngleDeg", "-180.000000", [514, 527]),
            ("PlayerStateCameraPitchRaw", "4095", [527, 539]),
            ("PlayerStateCameraPitchControl", "1.000000000", [527, 539]),
            ("PlayerStateForm", "Human", [673, 674]),
            ("PlayerStateGroupId", "200000", [322, 354]),
            ("PlayerStateMatchingValue", "-1", [659, 664]),
        ] {
            let actual = fields.iter().find(|field| field.name == name).unwrap();
            assert_eq!(actual.value, value);
            assert_eq!(actual.bits, Some(span));
        }
        for (start, width, bad) in [
            (1, 12, 62),
            (75, 1, 1),
            (76, 12, 221),
            (88, 32, 1108),
            (120, 1, 0),
            (147, 3, 3),
            (198, 3, 3),
            (310, 1, 0),
            (311, 12, 308),
            (323, 32, 0x800186a1),
            (322, 32, 210000),
            (354, 1, 0),
        ] {
            let mut invalid = payload.clone();
            set_bits(&mut invalid, start, width, bad);
            assert!(
                !semantic_fields(&Chunk {
                    protocol: 5,
                    port: Some(101),
                    payload: invalid
                })
                .iter()
                .any(|field| field.name.starts_with("PlayerState")),
                "start={start}"
            );
        }
        if payload_bytes == 118 {
            let mut invalid = payload.clone();
            set_bits(&mut invalid, 941, 3, 1);
            assert!(
                !semantic_fields(&Chunk {
                    protocol: 5,
                    port: Some(101),
                    payload: invalid
                })
                .iter()
                .any(|field| field.name.starts_with("PlayerState"))
            );
        }
        assert!(
            !semantic_fields(&Chunk {
                protocol: 5,
                port: Some(101),
                payload: payload[..117].to_vec()
            })
            .iter()
            .any(|field| field.name.starts_with("PlayerState"))
        );
        assert!(
            !semantic_fields(&Chunk {
                protocol: 5,
                port: Some(100),
                payload
            })
            .iter()
            .any(|field| field.name.starts_with("PlayerState"))
        );
    }
}

#[test]
fn player_state_matching_value_preserves_absence_and_five_bit_range() {
    // A wrong body offset or omission of the codec's minus-one transform
    // must change the exported matching value, including its -1 sentinel.
    for (payload_bytes, group, resource_group, body_start, span) in [
        (109, 1107, 200000, 280, [584, 589]),
        (109, 1108, 210000, 280, [584, 589]),
        (118, 1104, 200000, 355, [659, 664]),
        (138, 1104, 200000, 355, [659, 664]),
    ] {
        for (wire, expected) in [(0, "-1"), (1, "0"), (16, "15"), (31, "30")] {
            let mut payload = vec![0u8; payload_bytes];
            let header = if group == 1104 {
                vec![
                    (0, 1, 1),
                    (1, 12, 63),
                    (13, 32, group),
                    (45, 1, 1),
                    (75, 1, 0),
                    (76, 12, 222),
                    (88, 32, 1107),
                    (120, 1, 1),
                    (147, 3, 4),
                    (198, 3, 4),
                    (310, 12, 619),
                ]
            } else {
                vec![
                    (0, 1, 1),
                    (1, 12, 222),
                    (13, 32, group),
                    (45, 1, 1),
                    (72, 3, 4),
                    (123, 3, 4),
                    (235, 12, 619),
                ]
            };
            for (start, width, value) in header {
                set_bits(&mut payload, start, width, value);
            }
            set_bits(&mut payload, body_start - 33, 32, resource_group);
            set_bits(&mut payload, body_start - 1, 1, 1);
            set_bits(&mut payload, body_start + 304, 5, wire);
            let fields = semantic_fields(&Chunk {
                protocol: 5,
                port: Some(101),
                payload,
            });
            let actual = fields
                .iter()
                .find(|field| field.name == "PlayerStateMatchingValue")
                .unwrap();
            assert_eq!(
                actual.value, expected,
                "bytes={payload_bytes} group={group}"
            );
            assert_eq!(actual.bits, Some(span));
        }
    }
}

#[test]
fn regular_stage_resource_names_match_owned_localization() {
    for (resource, expected) in [
        ("Cop_Shakeup", "Spawning Grounds"),
        ("Cop_Shakespiral", "Sockeye Station"),
        ("Cop_Shakelift", "Salmonid Smokeyard"),
        ("Cop_Shakeship", "Marooner's Bay"),
        ("Cop_Shakedent", "Gone Fission Hydroplant"),
        ("Cop_Shakehighway", "Jammin' Salmon Junction"),
        ("Cop_Shakerail", "Bonerattle Arena"),
    ] {
        assert_eq!(stage_name(resource), expected);
    }
    assert_eq!(stage_name("Cop_Unknown"), "Unknown");
}

#[test]
fn unverified_replies_cannot_evict_proof_verified_key() {
    let mut candidates = vec![];
    push_candidate(
        &mut candidates,
        Candidate {
            key: PiaKey([1; 16]),
            reply_record: 1,
            verified: true,
        },
    );
    for record in 2..22 {
        push_candidate(
            &mut candidates,
            Candidate {
                key: PiaKey([record as u8; 16]),
                reply_record: record,
                verified: false,
            },
        );
    }
    assert_eq!(candidates.len(), 9);
    assert!(candidates.iter().any(|c| c.verified && c.reply_record == 1));
    assert_eq!(candidates.iter().filter(|c| !c.verified).count(), 8);
}

#[test]
fn room_key_is_tried_for_client_to_client_traffic() {
    let host = Ipv4Addr::new(192, 0, 2, 1);
    let mut decoder = Decoder::new(None, Some((host, PiaKey([7; 16]))), None);
    decoder.add_candidate(
        host,
        Candidate {
            key: PiaKey([3; 16]),
            reply_record: 1,
            verified: true,
        },
    );
    let packet = UdpPacket {
        record: 2,
        timestamp_ns: 0,
        source: Ipv4Addr::new(192, 0, 2, 2),
        destination: Ipv4Addr::new(192, 0, 2, 3),
        source_port: 49152,
        destination_port: 49152,
        source_mac: [0; 6],
        destination_mac: [0; 6],
        payload: vec![],
    };
    let keys = decoder.keys_for(&packet);
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].0, [3; 16]);
    assert_eq!(keys[1].0, [7; 16]);
}

#[test]
fn unauthenticated_header_is_not_labelled_authenticated() {
    let mut wire = vec![0; 45];
    wire[..5].copy_from_slice(b"\x32\xab\x98\x64\x8f");
    wire[12] = 1; // Unsupported footer, before GCM can authenticate.
    let peer = Ipv4Addr::new(192, 0, 2, 1);
    let packet = UdpPacket {
        record: 1,
        timestamp_ns: 0,
        source: peer,
        destination: Ipv4Addr::new(192, 0, 2, 2),
        source_port: 49152,
        destination_port: 49152,
        source_mac: [0; 6],
        destination_mac: [0; 6],
        payload: wire,
    };
    let mut decoder = Decoder::new(None, Some((peer, PiaKey([7; 16]))), None);
    assert_eq!(
        decoder.process(&packet, false)[0].status,
        "unsupported_layout"
    );
}

#[test]
fn manual_pia_check_is_peer_bound_and_distinguishes_authentication_from_layout() {
    let client = Ipv4Addr::new(192, 0, 2, 10);
    let server = Ipv4Addr::new(192, 0, 2, 11);
    let packet = authenticated_test_packet(client, server, 1, 1, 3, None, &protocol_fixture("rtt"));
    let mut correct = Decoder::new(None, Some((server, PiaKey([7; 16]))), None);
    assert_eq!(correct.manual_pia_authentication(&packet), Some(true));
    let mut corrupted =
        authenticated_test_packet(client, server, 2, 2, 3, None, &protocol_fixture("rtt"));
    corrupted.payload[21] ^= 0x80;
    assert_eq!(correct.manual_pia_authentication(&corrupted), Some(false));
    let foreign = authenticated_test_packet(
        Ipv4Addr::new(192, 0, 2, 12),
        Ipv4Addr::new(192, 0, 2, 13),
        3,
        3,
        3,
        None,
        &protocol_fixture("rtt"),
    );
    assert_eq!(correct.manual_pia_authentication(&foreign), None);
    let mut invalid =
        authenticated_test_packet(client, server, 4, 4, 3, None, &protocol_fixture("rtt"));
    invalid.payload[12] = 1;
    assert_eq!(correct.manual_pia_authentication(&invalid), None);
    correct.discard_manual_pia();
    assert_eq!(correct.manual_pia_authentication(&packet), None);

    let mut wrong = Decoder::new(None, Some((server, PiaKey([8; 16]))), None);
    assert_eq!(wrong.manual_pia_authentication(&packet), Some(false));
    wrong.replace_manual_pia(server, PiaKey([7; 16]));
    assert_eq!(wrong.manual_pia_authentication(&packet), Some(true));
}

#[test]
fn unverified_browse_candidate_can_be_confirmed_by_pia_authentication() {
    let client = Ipv4Addr::new(192, 0, 2, 10);
    let server = Ipv4Addr::new(192, 0, 2, 11);
    let mut decoder = Decoder::new(None, Some((server, PiaKey([8; 16]))), None);
    decoder.add_candidate(
        server,
        Candidate {
            key: PiaKey([7; 16]),
            reply_record: 0,
            verified: false,
        },
    );
    let packet = authenticated_test_packet(client, server, 1, 1, 3, None, &protocol_fixture("rtt"));
    assert_eq!(decoder.manual_pia_authentication(&packet), Some(false));
    assert_eq!(
        decoder.authenticated_candidate_key(&packet, server),
        Some([7; 16])
    );
    let mut corrupted =
        authenticated_test_packet(client, server, 2, 2, 3, None, &protocol_fixture("rtt"));
    corrupted.payload[21] ^= 0x80;
    assert_eq!(
        decoder.authenticated_candidate_key(&corrupted, server),
        None
    );
    assert_eq!(
        decoder.authenticated_candidate_key(&packet, Ipv4Addr::new(192, 0, 2, 12)),
        None
    );
}

#[test]
fn inherited_chunk_fields_and_bounds() {
    let body = [6, 0, 1, 1, 0x32, 0, 0xaa];
    let out = chunks(&body).unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(out[1].protocol, 1);
    assert_eq!(out[1].payload, [0xaa]);
    assert!(chunks(&[0x20]).is_err());
    assert!(chunks(&[4, 1]).is_err());
    assert!(chunks(&[6, 0, 4, 1, 1, 2]).is_err());
    let empty = chunks(&[4, 0]).unwrap();
    assert_eq!(empty[0].protocol, 0);
    assert!(empty[0].payload.is_empty());
}
#[test]
fn documented_enemy_spawn_field_offsets() {
    let mut payload = vec![0; 89];
    payload[0] = 7;
    payload[1..3].copy_from_slice(&81u16.to_be_bytes());
    payload[13..15].copy_from_slice(&72u16.to_be_bytes());
    payload[21..25].copy_from_slice(&0x0caefc01u32.to_le_bytes());
    payload[328 / 8] |= 3;
    let fields = semantic_fields(&Chunk {
        protocol: 8,
        port: Some(100),
        payload,
    });
    assert!(
        fields
            .iter()
            .any(|f| f.name == "SalmonId" && f.value == "3")
    );
    assert!(fields.iter().any(|f| f.name == "SpawnX"));
    assert!(fields.iter().any(|f| f.name == "GameFrame"));
    let mut different_layout = vec![0; 91];
    different_layout[0] = 7;
    different_layout[1..3].copy_from_slice(&83u16.to_be_bytes());
    different_layout[13..15].copy_from_slice(&74u16.to_be_bytes());
    different_layout[21..25].copy_from_slice(&0x0caefc01u32.to_le_bytes());
    let fields = semantic_fields(&Chunk {
        protocol: 8,
        port: Some(100),
        payload: different_layout,
    });
    assert!(fields.iter().any(|f| f.name == "TypeId"));
    assert!(fields.iter().any(|f| f.name == "SpawnX"));
    assert!(!fields.iter().any(|f| f.name == "GameFrame"));
    let mut unknown = vec![0; 29];
    unknown[0] = 7;
    unknown[1..3].copy_from_slice(&21u16.to_be_bytes());
    unknown[13..15].copy_from_slice(&12u16.to_be_bytes());
    unknown[21..25].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    assert_eq!(event_type(&unknown), None);
    assert_eq!(event_name(0x1234_5678), "Unknown");
    assert_eq!(event_name(0x4939c1ac), "ColPaintEvent");
    assert_eq!(event_name(0xe3f0eb9a), "ObjPaintEvent");
    // DirectBank from the documented packet example: classify by its ID,
    // then decode the fixed leading value and the documented trailer.
    let direct_bank = [
        0x07, 0x00, 0x15, 0x04, 0x44, 0x04, 0x3f, 0x00, 0x00, 0x04, 0x4a, 0x04, 0x4f, 0x00, 0x0c,
        0x00, 0x00, 0x52, 0x04, 0x00, 0x00, 0x59, 0x2c, 0x8d, 0x3d, 0x41, 0xda, 0x00, 0x00,
    ];
    assert_eq!(event_type(&direct_bank), Some(0x3d8d2c59));
    assert_eq!(
        describe_chunk(&Chunk {
            protocol: 8,
            port: Some(100),
            payload: direct_bank.to_vec(),
        }),
        "CoopItemGoldenIkuraDirectBankEvent"
    );
    let mut other_bitmap = direct_bank;
    other_bitmap[7] = 1;
    assert_eq!(event_type(&other_bitmap), None);
    let fields = semantic_fields(&Chunk {
        protocol: 8,
        port: Some(100),
        payload: direct_bank.to_vec(),
    });
    assert!(fields.iter().any(|f| f.name == "ValuePlusOne"));
    assert!(fields.iter().any(|f| f.name == "GameFrame"));
}

#[test]
fn authenticated_n0_events_expose_the_exact_resource_group() {
    let source = Ipv4Addr::new(192, 0, 2, 10);
    let peer = Ipv4Addr::new(192, 0, 2, 11);
    let mut decoder = Decoder::new(None, Some((peer, PiaKey([7; 16]))), None);
    for (record, type_id, length, resource_group, expected, frame_span, life_span) in [
        (
            1,
            0x74400e72u32,
            43,
            200000u32,
            "200000",
            [310, 333],
            [333, 337],
        ),
        (2, 0xcf9be194, 29, 210000, "210000", [204, 227], [227, 231]),
        (
            3,
            0x89e91844,
            39,
            0x12345678,
            "305419896",
            [278, 301],
            [301, 305],
        ),
        (4, 0x4c4d2b61, 29, 200000, "200000", [202, 225], [225, 229]),
    ] {
        let mut payload = vec![0u8; length];
        payload[0] = 7;
        payload[1..3].copy_from_slice(&((length - 8) as u16).to_be_bytes());
        payload[13..15].copy_from_slice(&((length - 17) as u16).to_be_bytes());
        payload[17..21].copy_from_slice(&resource_group.to_le_bytes());
        payload[21..25].copy_from_slice(&type_id.to_le_bytes());
        set_bits(&mut payload, frame_span[0], 23, 1193046);
        set_bits(&mut payload, life_span[0], 4, 11);
        let rows = decoder.process(
            &authenticated_test_packet(source, peer, record, record, 8, Some(100), &payload),
            false,
        );
        assert_eq!(rows[0].status, "authenticated");
        assert_eq!(rows[0].kind, event_name(type_id));
        assert_eq!(rows[0].layout.as_deref(), Some("confirmed_fields"));
        for (name, expected, span) in [
            ("GameFrame", "1193046", frame_span),
            ("LifeNumber", "11", life_span),
        ] {
            let actual = rows[0]
                .fields
                .iter()
                .find(|field| field.name == name)
                .expect("exact lifecycle body must expose its confirmed footer");
            assert_eq!(actual.value, expected);
            assert_eq!(actual.bits, Some(span));
        }
        let actual = rows[0]
            .fields
            .iter()
            .find(|field| field.name == "EventResourceGroupId")
            .expect("known n=0 event must expose its resource group");
        assert_eq!(actual.value, expected);
        assert_eq!(actual.bits, Some([136, 168]));
    }
}

fn item_motion_payload(type_id: u32, length: usize) -> Vec<u8> {
    let mut payload = vec![0u8; length];
    payload[0] = 7;
    payload[1..3].copy_from_slice(&((length - 8) as u16).to_be_bytes());
    payload[13..15].copy_from_slice(&((length - 17) as u16).to_be_bytes());
    payload[17..21].copy_from_slice(&200000u32.to_le_bytes());
    payload[21..25].copy_from_slice(&type_id.to_le_bytes());
    payload
}

#[test]
fn golden_ikura_shot_exports_native_start_other_vector_and_identity_fields() {
    // Native 0x379a090/0x379a590: XYZ A, packed direction (12+13),
    // magnitude 16, XYZ B, raw16, plus-one4, item ID32; then frame23/life4.
    let mut payload = item_motion_payload(0xf010cc79, 53);
    for (start, width, value) in [
        (200, 17, 0x8000),
        (217, 17, 0x14000),
        (234, 17, 0),
        (251, 12, 0xabc),
        (263, 13, 0x1234),
        (276, 16, 0xcdef),
        (292, 17, 0x4000),
        (309, 17, 0x18000),
        (326, 17, 0xffff),
        (343, 16, 0xbeef),
        (359, 4, 3),
        (363, 32, 0x0a00002a),
        (395, 23, 1193046),
        (418, 4, 11),
    ] {
        set_bits(&mut payload, start, width, value);
    }
    let source = Ipv4Addr::new(192, 0, 2, 10);
    let peer = Ipv4Addr::new(192, 0, 2, 11);
    let mut decoder = Decoder::new(None, Some((peer, PiaKey([7; 16]))), None);
    let rows = decoder.process(
        &authenticated_test_packet(source, peer, 1, 1, 8, Some(100), &payload),
        false,
    );
    assert_eq!(rows[0].layout.as_deref(), Some("confirmed_fields"));
    for (name, expected, span) in [
        ("ShotStartX", "128.002", [200, 217]),
        ("ShotStartY", "-64.001", [217, 234]),
        ("ShotStartZ", "0.000", [234, 251]),
        ("ShotVectorRawA", "2748", [251, 263]),
        ("ShotVectorRawB", "4660", [263, 276]),
        ("ShotVectorLengthRaw", "52719", [276, 292]),
        ("ShotOtherX", "64.001", [292, 309]),
        ("ShotOtherY", "-128.002", [309, 326]),
        ("ShotOtherZ", "256.000", [326, 343]),
        ("ShotValueRaw", "48879", [343, 359]),
        ("ShotValuePlusOne", "3", [359, 363]),
        ("ShotItemId", "167772202", [363, 395]),
        ("GameFrame", "1193046", [395, 418]),
        ("LifeNumber", "11", [418, 422]),
    ] {
        let actual = rows[0]
            .fields
            .iter()
            .find(|field| field.name == name)
            .expect(name);
        assert_eq!(actual.value, expected, "{name}");
        assert_eq!(actual.bits, Some(span), "{name}");
    }
    assert!(
        !rows[0]
            .fields
            .iter()
            .any(|field| field.name.contains("Landing"))
    );
}

#[test]
fn item_shoot_sleep_exports_identity_and_footer_without_a_landing_position() {
    let mut payload = item_motion_payload(0x1a894818, 33);
    for (start, width, value) in [
        (200, 32, 0x0a00002a),
        (232, 4, 3),
        (236, 23, 1193046),
        (259, 4, 11),
    ] {
        set_bits(&mut payload, start, width, value);
    }
    let fields = semantic_fields(&Chunk {
        protocol: 8,
        port: Some(100),
        payload,
    });
    for (name, expected, span) in [
        ("SleepItemId", "167772202", [200, 232]),
        ("SleepValuePlusOne", "3", [232, 236]),
        ("GameFrame", "1193046", [236, 259]),
        ("LifeNumber", "11", [259, 263]),
    ] {
        let actual = fields.iter().find(|field| field.name == name).expect(name);
        assert_eq!(actual.value, expected, "{name}");
        assert_eq!(actual.bits, Some(span), "{name}");
    }
    assert!(!fields.iter().any(|field| field.name.ends_with('X')
        || field.name.ends_with('Y')
        || field.name.ends_with('Z')));
}

#[test]
fn golden_ikura_shot_velocity_matches_owned_complete_decoder_vectors() {
    // Independent Unicorn execution of 0x379a590 and 0x31cbcb8, with the
    // original 256-entry trig table. These are synthetic, non-capture inputs.
    for (pitch, yaw, length, expected) in [
        (0, 0, 0, [0.0, 0.0, 0.0]),
        (0, 0, 65535, [0.0, 0.0, 34.13333511352539]),
        (0, 4095, 65535, [0.0, 0.0, -34.13333511352539]),
        (2047, 0, 65535, [0.0, 34.13333511352539, 0.0]),
        (4095, 0, 65535, [0.0, -34.13333511352539, 0.0]),
        (
            1024,
            2048,
            2048,
            [
                0.7539712190628052,
                0.7545464634895325,
                -0.0002892578486353159,
            ],
        ),
        (
            0xabc,
            0x1234,
            0xcdef,
            [-9.8920316696167, -14.050525665283203, 21.41693878173828],
        ),
        (
            333,
            7000,
            1920,
            [-0.7660775780677795, 0.252758651971817, -0.590989887714386],
        ),
    ] {
        let mut payload = item_motion_payload(0xf010cc79, 53);
        set_bits(&mut payload, 251, 12, pitch);
        set_bits(&mut payload, 263, 13, yaw);
        set_bits(&mut payload, 276, 16, length);
        let fields = semantic_fields(&Chunk {
            protocol: 8,
            port: Some(100),
            payload,
        });
        for (name, want) in ["ShotVelocityX", "ShotVelocityY", "ShotVelocityZ"]
            .into_iter()
            .zip(expected)
        {
            let actual = fields.iter().find(|field| field.name == name).expect(name);
            let value = actual.value.parse::<f64>().unwrap();
            assert!(
                (value - want).abs() <= 0.000005,
                "{name} raw=({pitch},{yaw},{length}): {value} != {want}"
            );
            assert_eq!(actual.bits, Some([251, 292]));
        }
    }
}

#[test]
fn item_motion_fields_require_exact_length_port_and_native_n0_header() {
    for (id, length) in [(0xf010cc79, 53), (0x1a894818, 33)] {
        let valid = item_motion_payload(id, length);
        let mut rejected = vec![(Some(101), valid.clone()), (None, valid.clone())];
        for changed_length in [length - 1, length + 1] {
            let mut invalid = valid.clone();
            invalid.resize(changed_length, 0);
            invalid[1..3].copy_from_slice(&((changed_length - 8) as u16).to_be_bytes());
            invalid[13..15].copy_from_slice(&((changed_length - 17) as u16).to_be_bytes());
            rejected.push((Some(100), invalid));
        }
        for byte in [0, 2, 7, 14, 21] {
            let mut invalid = valid.clone();
            invalid[byte] ^= 1;
            rejected.push((Some(100), invalid));
        }
        for (port, payload) in rejected {
            let fields = semantic_fields(&Chunk {
                protocol: 8,
                port,
                payload,
            });
            assert!(
                !fields.iter().any(|field| field.name.starts_with("Shot")
                    || field.name.starts_with("Sleep")
                    || matches!(field.name.as_str(), "GameFrame" | "LifeNumber")),
                "type={id:08x}, port={port:?}"
            );
        }
    }
}

#[test]
fn lifecycle_event_footers_require_exact_body_size_and_zero_padding() {
    let source = Ipv4Addr::new(192, 0, 2, 10);
    let peer = Ipv4Addr::new(192, 0, 2, 11);
    let mut decoder = Decoder::new(None, Some((peer, PiaKey([7; 16]))), None);
    for (type_id, length, padding_start) in [
        (0x74400e72u32, 43, 337),
        (0xcf9be194, 29, 231),
        (0x89e91844, 39, 305),
        (0x4c4d2b61, 29, 229),
    ] {
        let mut valid = vec![0u8; length];
        valid[0] = 7;
        valid[17..21].copy_from_slice(&200000u32.to_le_bytes());
        valid[21..25].copy_from_slice(&type_id.to_le_bytes());
        let truncated = valid[..length - 1].to_vec();
        let mut extended = valid.clone();
        extended.push(0);
        let mut rejected = vec![
            ("truncated body".to_string(), truncated),
            ("extended body".to_string(), extended),
        ];
        for padding_bit in padding_start..length * 8 {
            let mut bad_padding = valid.clone();
            set_bits(&mut bad_padding, padding_bit, 1, 1);
            rejected.push((format!("nonzero padding at {padding_bit}"), bad_padding));
        }
        for (reason, mut payload) in rejected {
            let payload_length = payload.len();
            payload[1..3].copy_from_slice(&((payload_length - 8) as u16).to_be_bytes());
            payload[13..15].copy_from_slice(&((payload_length - 17) as u16).to_be_bytes());
            let chunk = Chunk {
                protocol: 8,
                port: Some(100),
                payload: payload.clone(),
            };
            assert_eq!(describe_chunk(&chunk), event_name(type_id), "{reason}");
            let rows = decoder.process(
                &authenticated_test_packet(source, peer, 1, 1, 8, Some(100), &chunk.payload),
                false,
            );
            assert_eq!(rows[0].status, "authenticated");
            assert_eq!(rows[0].layout.as_deref(), Some("type_fields"), "{reason}");
            let fields = &rows[0].fields;
            assert!(
                fields.iter().any(|field| field.name == "TypeId"),
                "{reason}"
            );
            let resource_group = fields
                .iter()
                .find(|field| field.name == "EventResourceGroupId")
                .expect("known n=0 envelope retains independently proven resource metadata");
            assert_eq!(resource_group.value, "200000", "{reason}");
            assert!(
                !fields
                    .iter()
                    .any(|field| matches!(field.name.as_str(), "GameFrame" | "LifeNumber")),
                "type={type_id:08x} {reason}"
            );

            payload[7] = 1;
            let fields = semantic_fields(&Chunk {
                protocol: 8,
                port: Some(100),
                payload: payload.clone(),
            });
            assert!(
                !fields.iter().any(|field| matches!(
                    field.name.as_str(),
                    "TypeId" | "EventResourceGroupId" | "GameFrame" | "LifeNumber"
                )),
                "nonzero count type={type_id:08x} {reason}"
            );
        }
    }
}

#[test]
fn event_resource_group_requires_a_known_complete_n0_envelope() {
    let mut valid = vec![0u8; 29];
    valid[0] = 7;
    valid[1..3].copy_from_slice(&21u16.to_be_bytes());
    valid[13..15].copy_from_slice(&12u16.to_be_bytes());
    valid[17..21].copy_from_slice(&200000u32.to_le_bytes());
    valid[21..25].copy_from_slice(&0xcf9be194u32.to_le_bytes());
    let mut rejected = Vec::new();
    for length in [0, 7, 17, 24, 28] {
        rejected.push((format!("truncated {length}"), valid[..length].to_vec()));
    }
    for (offset, value, reason) in [
        (0, 6, "wrong marker"),
        (2, 20, "message length"),
        (14, 11, "event length"),
        (7, 1, "one entry"),
        (7, 23, "23 entries"),
        (7, 24, "invalid count"),
        (7, 255, "invalid maximum count"),
    ] {
        let mut payload = valid.clone();
        payload[offset] = value;
        rejected.push((reason.to_string(), payload));
    }
    let mut unknown = valid.clone();
    unknown[21..25].copy_from_slice(&0x12345678u32.to_le_bytes());
    rejected.push(("unknown type".to_string(), unknown));

    // n>0 uses a 12-byte header, shifting the inner lengths and event fields
    // by four bytes. Those offsets are outside this decoder's n=0 contract.
    let mut one_entry = vec![0; 33];
    one_entry[0] = 7;
    one_entry[7] = 1;
    one_entry[1..3].copy_from_slice(&21u16.to_be_bytes());
    one_entry[17..19].copy_from_slice(&12u16.to_be_bytes());
    one_entry[21..25].copy_from_slice(&200000u32.to_le_bytes());
    one_entry[25..29].copy_from_slice(&0xcf9be194u32.to_le_bytes());
    rejected.push(("nonzero-count header".to_string(), one_entry));

    for (reason, payload) in rejected {
        assert_eq!(event_type(&payload), None, "{reason}");
        let chunk = Chunk {
            protocol: 8,
            port: Some(100),
            payload,
        };
        assert_eq!(describe_chunk(&chunk), "BroadcastEventProtocol", "{reason}");
        assert!(
            !semantic_fields(&chunk)
                .iter()
                .any(|field| field.name == "EventResourceGroupId"),
            "{reason}"
        );
    }
    assert!(
        !semantic_fields(&Chunk {
            protocol: 7,
            port: Some(100),
            payload: valid,
        })
        .iter()
        .any(|field| field.name == "EventResourceGroupId")
    );
}

#[test]
fn authenticated_paint_events_expose_position_and_raw_scalars() {
    let source = Ipv4Addr::new(192, 0, 2, 10);
    let peer = Ipv4Addr::new(192, 0, 2, 11);
    let mut decoder = Decoder::new(None, Some((peer, PiaKey([7; 16]))), None);
    for (index, (type_id, body_bits)) in [(0x4939c1acu32, 154), (0xe3f0eb9au32, 155)]
        .into_iter()
        .enumerate()
    {
        let mut payload = vec![0u8; 48];
        payload[0] = 7;
        payload[1..3].copy_from_slice(&40u16.to_be_bytes());
        payload[13..15].copy_from_slice(&31u16.to_be_bytes());
        payload[21..25].copy_from_slice(&type_id.to_le_bytes());
        for (start, width, value) in [
            (200, 17, 0),
            (217, 17, 0xffff),
            (234, 17, 0x1ffff),
            (251, 12, 0xabc),
            (263, 12, 0x123),
            (200 + body_bits, 23, 12345),
        ] {
            set_bits(&mut payload, start, width, value);
        }
        let packet =
            authenticated_test_packet(source, peer, index as u64 + 1, 1, 8, None, &payload);
        let rows = decoder.process(&packet, false);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "authenticated");
        for (name, value, span) in [
            ("PaintX", "0.000", [200, 217]),
            ("PaintY", "256.000", [217, 234]),
            ("PaintZ", "-256.000", [234, 251]),
            ("PaintScalarRawA", "2748", [251, 263]),
            ("PaintScalarRawB", "291", [263, 275]),
        ] {
            let actual = rows[0]
                .fields
                .iter()
                .find(|field| field.name == name)
                .unwrap();
            assert_eq!(actual.value, value);
            assert_eq!(actual.bits, Some(span));
        }
        assert_eq!(field_value(&rows[0], "GameFrame"), Some("12345"));
    }
}

#[test]
fn paint_fields_require_fixed_typed_authenticated_event_shape() {
    let source = Ipv4Addr::new(192, 0, 2, 10);
    let peer = Ipv4Addr::new(192, 0, 2, 11);
    let mut valid = vec![0u8; 48];
    valid[0] = 7;
    valid[1..3].copy_from_slice(&40u16.to_be_bytes());
    valid[13..15].copy_from_slice(&31u16.to_be_bytes());
    valid[21..25].copy_from_slice(&0x4939c1acu32.to_le_bytes());
    for (start, width, value) in [(200, 17, 0x100), (251, 12, 17), (263, 12, 19)] {
        set_bits(&mut valid, start, width, value);
    }
    let assert_no_paint_fields = |payload: Vec<u8>| {
        let fields = semantic_fields(&Chunk {
            protocol: 8,
            port: None,
            payload,
        });
        assert!(!fields.iter().any(|field| field.name.starts_with("Paint")));
    };
    let mut bad_message_length = valid.clone();
    bad_message_length[1..3].copy_from_slice(&39u16.to_be_bytes());
    assert_no_paint_fields(bad_message_length);
    let mut bad_event_length = valid.clone();
    bad_event_length[13..15].copy_from_slice(&30u16.to_be_bytes());
    assert_no_paint_fields(bad_event_length);
    let mut bad_envelope = valid.clone();
    bad_envelope[7] = 1;
    assert_no_paint_fields(bad_envelope);
    let mut other_type = valid.clone();
    other_type[21..25].copy_from_slice(&0x6e7d7386u32.to_le_bytes());
    assert_no_paint_fields(other_type);
    for length in [47, 49] {
        let mut wrong_size = valid.clone();
        wrong_size.resize(length, 0);
        wrong_size[1..3].copy_from_slice(&((length - 8) as u16).to_be_bytes());
        wrong_size[13..15].copy_from_slice(&((length - 17) as u16).to_be_bytes());
        assert_no_paint_fields(wrong_size);
    }
    let mut decoder = Decoder::new(None, Some((peer, PiaKey([7; 16]))), None);
    let mut packet = authenticated_test_packet(source, peer, 1, 1, 8, None, &valid);
    packet.payload[21] ^= 0x80;
    let rows = decoder.process(&packet, false);
    assert_ne!(rows[0].status, "authenticated");
    assert!(
        !rows
            .iter()
            .flat_map(|row| &row.fields)
            .any(|field| field.name.starts_with("Paint"))
    );
}
