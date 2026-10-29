use super::*;

fn composite(bytes: usize, sequence_last_bit: u64) -> Vec<u8> {
    let mut payload = vec![0; bytes];
    for (start, width, value) in [
        (0, 1, 1),
        (1, 12, 63),
        (13, 32, 1104),
        (45, 1, 1),
        (46, 3, 1),
        (75, 1, sequence_last_bit),
        (76, 12, 222),
        (88, 32, 1107),
        (120, 1, 1),
        (121, 14, 16383),
        (135, 12, 4095),
        (147, 3, 4),
        (198, 3, 4),
    ] {
        set_bits(&mut payload, start, width, value);
    }
    for (index, value) in [0, 1, 2048, 4095].into_iter().enumerate() {
        set_bits(&mut payload, 150 + 12 * index, 12, value);
        set_bits(&mut payload, 201 + 12 * index, 12, 4095 - value);
    }
    payload
}

#[test]
fn obs_nested_stats_exports_complete_short_frames_with_both_sequence_final_bits() {
    for last in [0, 1] {
        let fields = semantic_fields(&Chunk {
            protocol: 5,
            port: Some(101),
            payload: composite(39, last),
        });
        for (name, value, span) in [
            ("NestedStatsFrameLength", "222", [76, 88]),
            ("NestedStatsGroupId", "1107", [88, 120]),
            ("NestedStatsPresent", "1", [120, 121]),
            ("NestedStatsIkuraNum", "16383", [121, 135]),
            ("NestedStatsHeldGoldenIkuraNum", "4095", [135, 147]),
            ("NestedStatsArrayACount", "4", [147, 150]),
            ("NestedStatsArrayBCount", "4", [198, 201]),
            ("NestedStatsArrayA[0]", "0", [150, 162]),
            ("NestedStatsArrayA[3]", "4095", [186, 198]),
            ("NestedStatsArrayB[0]", "4095", [201, 213]),
            ("NestedStatsArrayB[3]", "0", [237, 249]),
        ] {
            let actual = fields
                .iter()
                .find(|field| field.name == name)
                .unwrap_or_else(|| panic!("missing {name}, sequence bit75={last}"));
            assert_eq!(actual.value, value);
            assert_eq!(actual.bits, Some(span));
        }
        assert_eq!(
            fields.iter().find(|f| f.name == "Sequence").unwrap().value,
            "1"
        );
        assert!(!fields.iter().any(|f| f.name.starts_with("PlayerState")));
        assert!(!fields.iter().any(|f| f.name == "NestedStatsFrameMode"));
    }
}

#[test]
fn obs_nested_stats_rejects_each_marker_port_and_truncation_without_destroying_outer_sequence() {
    let valid = composite(39, 1);
    for (start, width, value) in [
        (0, 1, 0),
        (1, 12, 62),
        (13, 32, 1108),
        (45, 1, 0),
        (76, 12, 221),
        (88, 32, 1108),
        (120, 1, 0),
        (147, 3, 3),
        (198, 3, 5),
    ] {
        let mut payload = valid.clone();
        set_bits(&mut payload, start, width, value);
        let fields = semantic_fields(&Chunk {
            protocol: 5,
            port: Some(101),
            payload,
        });
        assert!(
            !fields.iter().any(|f| f.name.starts_with("NestedStats")),
            "accepted bad marker {start}"
        );
        if start >= 76 {
            assert!(fields.iter().any(|f| f.name == "Sequence"));
        }
    }
    for port in [None, Some(100), Some(102)] {
        let fields = semantic_fields(&Chunk {
            protocol: 5,
            port,
            payload: valid.clone(),
        });
        assert!(!fields.iter().any(|f| f.name.starts_with("NestedStats")));
    }
    for bytes in [0, 1, 10, 15, 30, 38] {
        let fields = semantic_fields(&Chunk {
            protocol: 5,
            port: Some(101),
            payload: valid[..bytes].to_vec(),
        });
        assert!(
            !fields.iter().any(|f| f.name.starts_with("NestedStats")),
            "accepted {bytes} byte prefix"
        );
    }
}

#[test]
fn obs_nested_stats_coexists_with_the_independently_guarded_actor_metadata() {
    for bytes in [118, 138] {
        let mut payload = composite(bytes, 0);
        for (start, width, value) in [(310, 1, 1), (311, 12, 309), (323, 32, 0x800186a0)] {
            set_bits(&mut payload, start, width, value);
        }
        let fields = semantic_fields(&Chunk {
            protocol: 5,
            port: Some(101),
            payload,
        });
        assert!(fields.iter().any(|f| f.name == "NestedStatsIkuraNum"));
        assert!(
            fields
                .iter()
                .any(|f| f.name == "PlayerStateGroupId" && f.value == "200000")
        );
    }
}

#[test]
fn obs_nested_stats_is_exposed_only_after_authenticated_protocol_decoding() {
    let source = Ipv4Addr::new(192, 0, 2, 10);
    let peer = Ipv4Addr::new(192, 0, 2, 11);
    let mut decoder = Decoder::new(None, Some((peer, PiaKey([7; 16]))), None);
    let packet =
        authenticated_test_packet(source, peer, 1, 1_000_000, 5, Some(101), &composite(39, 1));
    let rows = decoder.process(&packet, false);
    assert_eq!(rows[0].kind, "CoopSequenceNetState");
    assert_eq!(rows[0].status, "authenticated");
    assert!(
        rows[0]
            .fields
            .iter()
            .any(|f| f.name == "NestedStatsIkuraNum")
    );
    let mut bad = packet;
    *bad.payload.last_mut().unwrap() ^= 1;
    assert!(
        !decoder
            .process(&bad, false)
            .iter()
            .any(|row| row.fields.iter().any(|f| f.name.starts_with("NestedStats")))
    );
}
