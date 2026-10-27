use super::*;
use aes::Aes128;
use aes_gcm::aead::{
    AeadInOut, KeyInit,
    consts::{U8, U12},
};
use aes_gcm::{AesGcm, Nonce};

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
