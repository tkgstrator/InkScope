use super::*;

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
