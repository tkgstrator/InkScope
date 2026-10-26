use salmon_wave_lookahead::stage_data::{BankLayer, PillarStaticGate, all_stages, stage_spawns};

#[test]
fn bank_and_base_candidates_preserve_banc_identity_and_common_layer() {
    assert_eq!(
        all_stages()
            .iter()
            .map(|stage| stage.ikura_banks.len())
            .sum::<usize>(),
        35
    );
    assert_eq!(
        all_stages()
            .iter()
            .map(|stage| stage.ikura_bank_bases.len())
            .sum::<usize>(),
        35
    );

    let shakedent = stage_spawns("Cop_Shakedent").unwrap();
    assert_eq!(shakedent.ikura_banks.len(), 2);
    assert_eq!(shakedent.ikura_bank_bases.len(), 2);
    assert_eq!(shakedent.ikura_banks[0].layer, BankLayer::Common);
    assert_eq!(shakedent.ikura_banks[0].actor.actor_index, 15);
    assert_eq!(shakedent.ikura_banks[0].actor.hash, 0xaca8_baba_1f72_1a58);
    assert_eq!(shakedent.ikura_banks[0].actor.translate, [2.5, 9.0, 40.0]);
    assert_eq!(shakedent.ikura_banks[1].layer, BankLayer::Low);
    assert_eq!(shakedent.ikura_banks[1].actor.actor_index, 299);
    assert_eq!(shakedent.ikura_banks[1].actor.translate, [0.0, 1.5, -24.0]);
    assert_eq!(shakedent.ikura_bank_bases[0].layer, BankLayer::Common);
    assert_eq!(shakedent.ikura_bank_bases[0].actor.actor_index, 5);
    assert_eq!(
        shakedent.ikura_bank_bases[0].actor.translate,
        [2.5, 3.0, 40.0]
    );
    assert_eq!(shakedent.ikura_bank_bases[1].layer, BankLayer::Low);
    assert_eq!(shakedent.ikura_bank_bases[1].actor.actor_index, 319);
    assert_eq!(
        shakedent.ikura_bank_bases[1].actor.translate,
        [0.0, -4.5, -24.0]
    );
}

#[test]
fn bank_and_base_layers_retain_all_four_source_labels() {
    for actors in [
        all_stages()
            .iter()
            .flat_map(|stage| stage.ikura_banks)
            .collect::<Vec<_>>(),
        all_stages()
            .iter()
            .flat_map(|stage| stage.ikura_bank_bases)
            .collect::<Vec<_>>(),
    ] {
        let counts = [
            BankLayer::Common,
            BankLayer::Low,
            BankLayer::Mid,
            BankLayer::High,
        ]
        .map(|layer| actors.iter().filter(|actor| actor.layer == layer).count());
        assert_eq!(counts, [15, 14, 1, 5]);
    }
}

#[test]
fn tamaire_geyser_sources_cover_owned_packs_without_reordering() {
    assert_eq!(all_stages().len(), 15);
    assert_eq!(
        all_stages()
            .iter()
            .map(|stage| stage.tamaire_geysers.len())
            .sum::<usize>(),
        63
    );
    assert_eq!(
        all_stages()
            .iter()
            .filter(|stage| !stage.tamaire_geysers.is_empty())
            .count(),
        8
    );
    for (name, counts) in [
        ("Cop_Shakedent", [8, 8, 4]),
        ("Cop_Shakehighway", [8, 8, 4]),
        ("Cop_Shakelift", [7, 7, 5]),
        ("Cop_Shakerail", [9, 9, 6]),
        ("Cop_Shakeship", [8, 8, 4]),
        ("Cop_Shakespiral", [7, 7, 4]),
        ("Cop_Shakeup", [9, 9, 5]),
        ("Cop_Tutorial", [7, 7, 4]),
    ] {
        let stage = stage_spawns(name).unwrap();
        assert!(stage.tamaire_geysers.iter().all(|source| source.layer <= 2));
        for (tide, count) in counts.into_iter().enumerate() {
            assert_eq!(
                stage.tamaire_geyser_sources(tide as u8).unwrap().len(),
                count
            );
        }
    }
    let carousel = stage_spawns("Cop_Carousel").unwrap();
    assert!(carousel.tamaire_geysers.is_empty());
    assert!(carousel.tamaire_geyser_sources(0).unwrap().is_empty());
}

#[test]
fn tamaire_geyser_tide_lists_keep_mixed_layer_banc_order_and_source_identity() {
    let shakedent = stage_spawns("Cop_Shakedent").unwrap();
    for tide in [0, 1] {
        assert_eq!(
            shakedent
                .tamaire_geyser_sources(tide)
                .unwrap()
                .iter()
                .map(|source| source.actor.actor_index)
                .collect::<Vec<_>>(),
            [59, 60, 61, 62, 172, 180, 190, 191]
        );
    }
    assert_eq!(
        shakedent
            .tamaire_geyser_sources(2)
            .unwrap()
            .iter()
            .map(|source| source.actor.actor_index)
            .collect::<Vec<_>>(),
        [59, 60, 61, 62]
    );
    assert!(shakedent.tamaire_geyser_sources(3).is_none());

    let first = shakedent.tamaire_geysers[0];
    assert_eq!(first.layer, 2);
    assert_eq!(first.actor.actor_index, 59);
    assert_eq!(first.actor.hash, 0xeef2_e750_7231_a20c);
    assert_eq!(
        first.actor.translate.map(f32::to_bits),
        [0x40b0_0000, 0x413f_fffe, 0x41bc_0000]
    );
    assert_eq!(first.actor.rotate, None);
}

fn counts(name: &str) -> [usize; 9] {
    let stage = stage_spawns(name).expect("known 11.3.0 stage");
    std::array::from_fn(|key| stage.groups[key].len())
}

#[test]
fn every_owned_cop_pack_has_nine_spawn_groups() {
    assert_eq!(all_stages().len(), 15);
    for stage in all_stages() {
        assert_eq!(stage.available_keys(), (0..9).collect::<Vec<_>>());
        assert_eq!(stage.pack_sha256.len(), 64);
        assert!(stage.banc_path.starts_with("Banc/"));
        for tide in 0..3 {
            for direction in 0..3 {
                assert!(!stage.group(tide, direction).unwrap().is_empty());
            }
        }
    }
    assert_eq!(counts("Cop_Shakehighway"), [6, 6, 2, 4, 5, 2, 3, 6, 2]);
    assert_eq!(counts("Cop_Temple"), [3, 2, 2, 1, 2, 1, 2, 2, 3]);
    assert_eq!(counts("Cop_Carousel"), [1, 1, 3, 1, 2, 3, 1, 2, 4]);
}

#[test]
fn shakehighway_actor_values_preserve_banc_order_hash_width_and_optional_rotation() {
    let stage = stage_spawns("Cop_Shakehighway").unwrap();
    let high_direction_zero = stage.group(2, 0).unwrap();
    assert_eq!(high_direction_zero.len(), 2);
    assert_eq!(high_direction_zero[0].actor_index, 68);
    assert_eq!(high_direction_zero[0].hash, 5_153_945_711_718_460_144);
    assert_eq!(high_direction_zero[0].translate, [18.0, 7.5, 43.5]);
    assert_eq!(high_direction_zero[0].rotate, None);
    assert_eq!(high_direction_zero[1].actor_index, 70);
    assert_eq!(high_direction_zero[1].hash, 1_550_522_138_093_765_307);
    assert_eq!(high_direction_zero[1].translate, [15.5, 7.5, 41.75]);
    assert_eq!(high_direction_zero[1].rotate, None);

    let rotated = stage
        .group(2, 2)
        .unwrap()
        .iter()
        .find(|actor| actor.actor_index == 65)
        .unwrap();
    assert_eq!(rotated.hash, 16_774_586_064_209_673_668);
    assert_eq!(
        rotated.translate.map(f32::to_bits),
        [0xc1ea_0000, 0x40cf_0000, 0x4250_0000]
    );
    assert_eq!(
        rotated.rotate.unwrap().map(f32::to_bits),
        [0x4035_91ee, 0x3dd3_9af8, 0x4046_fcdb]
    );
}

#[test]
fn water_override_default_and_event_ban_match_stage_component() {
    assert_eq!(
        stage_spawns("Cop_Carousel").unwrap().water_override,
        Some([2, 2, 1])
    );
    assert_eq!(
        stage_spawns("Cop_Upland").unwrap().water_override,
        Some([1, 3, 1])
    );
    let temple = stage_spawns("Cop_Temple").unwrap();
    assert_eq!(temple.water_override, None);
    assert_eq!(temple.ban_event_mask, 1 << 3);
    assert_eq!(
        stage_spawns("Cop_Shakehighway").unwrap().water_override,
        None
    );
}

#[test]
fn stage_names_and_group_indices_are_bounded() {
    assert!(stage_spawns("Cop_Unknown").is_none());
    let highway = stage_spawns("Cop_Shakehighway").unwrap();
    assert!(highway.group(3, 0).is_none());
    assert!(highway.group(0, 3).is_none());
    assert_eq!(highway.available_keys(), (0..9).collect::<Vec<_>>());
    let tutorial = stage_spawns("Cop_Tutorial").unwrap();
    let spiral = stage_spawns("Cop_Shakespiral").unwrap();
    assert_eq!(tutorial.banc_path, spiral.banc_path);
    assert_eq!(counts("Cop_Tutorial"), counts("Cop_Shakespiral"));
}

#[test]
fn pillar_anchors_keep_individual_links_and_initial_static_gate() {
    let mut anchors = 0;
    let mut valid_links = 0;
    let mut dangling_links = 0;
    for stage in all_stages() {
        for tide in 0..3 {
            for direction in 0..3 {
                assert_eq!(
                    stage.pillar_initial_static_gate(tide, direction),
                    Some(PillarStaticGate::Available),
                    "{} tide {tide} direction {direction}",
                    stage.name
                );
                let group = &stage.pillar_anchors[3 * direction as usize + tide as usize];
                assert!(!group.is_empty());
                for anchor in *group {
                    assert!(!anchor.arrivals.is_empty());
                    anchors += 1;
                    valid_links += anchor.arrivals.len();
                    dangling_links += anchor.dangling_destinations.len();
                }
            }
        }
    }
    assert_eq!(anchors, 139);
    assert_eq!(valid_links, 406);
    assert_eq!(dangling_links, 3);
}

#[test]
fn pillar_link_identities_and_duplicate_anchors_remain_separate() {
    let highway = stage_spawns("Cop_Shakehighway").unwrap();
    let high_direction_zero = highway.pillar_anchors[2];
    assert_eq!(high_direction_zero.len(), 1);
    assert_eq!(high_direction_zero[0].actor_index, 129);
    assert_eq!(high_direction_zero[0].hash, 8_883_940_962_727_889_744);
    assert_eq!(
        high_direction_zero[0].arrivals[0].hash,
        8_190_429_554_042_228_555
    );
    assert_eq!(high_direction_zero[0].arrivals.len(), 3);

    let temple = stage_spawns("Cop_Temple").unwrap();
    assert_eq!(temple.pillar_anchors[0].len(), 3);
    assert_eq!(temple.pillar_initial_static_gate(3, 0), None);
    assert_eq!(temple.pillar_initial_static_gate(0, 3), None);
}

#[test]
fn candidate_gate_can_fallback_from_a_missing_requested_pillar_group() {
    let mut stage = *stage_spawns("Cop_Shakehighway").unwrap();
    // The candidate callback may use another direction, while the distinct
    // spot callback cannot directly select this missing requested group.
    stage.pillar_anchors[0] = &[];
    assert!(!stage.pillar_anchors[3].is_empty());
    assert_eq!(
        stage.pillar_initial_static_gate(0, 0),
        Some(PillarStaticGate::Available)
    );
    assert!(stage.pillar_first_untouched_anchor(0, 0).is_none());
}

#[test]
fn untouched_pillar_anchor_identity_and_raw_banc_transform_are_known_for_singletons() {
    let carousel = stage_spawns("Cop_Carousel").unwrap();
    let anchor = carousel.pillar_first_untouched_anchor(0, 1).unwrap();
    assert_eq!(anchor.actor_index, 312);
    assert_eq!(
        anchor.translate.map(f32::to_bits),
        [0x423bfff1, 0x40a6fa18, 0x40e00000]
    );
    assert_eq!(
        anchor.rotate.unwrap().map(f32::to_bits),
        [0x3c4a09be, 0x80000000, 0xbc75a565]
    );

    let highway = stage_spawns("Cop_Shakehighway").unwrap();
    let unrotated = highway.pillar_first_untouched_anchor(2, 0).unwrap();
    assert_eq!(unrotated.actor_index, 129);
    assert_eq!(unrotated.rotate, None);
}

#[test]
fn three_duplicate_pillar_groups_require_runtime_registration_order() {
    let mut known = 0;
    let mut ambiguous = 0;
    for stage in all_stages() {
        for direction in 0..3 {
            for tide in 0..3 {
                if stage
                    .pillar_first_untouched_anchor(tide, direction)
                    .is_some()
                {
                    known += 1;
                } else {
                    ambiguous += 1;
                }
            }
        }
    }
    assert_eq!((known, ambiguous), (132, 3));
    let temple = stage_spawns("Cop_Temple").unwrap();
    assert!(temple.pillar_first_untouched_anchor(0, 0).is_none());
    assert!(temple.pillar_first_untouched_anchor(0, 2).is_none());
    assert!(temple.pillar_first_untouched_anchor(3, 0).is_none());
    assert!(temple.pillar_first_untouched_anchor(0, 3).is_none());
}

#[test]
fn artillery_points_preserve_tide_direction_and_source_actor_identity() {
    let mut actors = 0;
    let mut singleton_groups = 0;
    let mut doubleton_groups = 0;
    let mut empty_high_groups = 0;
    for stage in all_stages() {
        for direction in 0..3 {
            for tide in 0..3 {
                let group = stage.artillery_group(tide, direction).unwrap();
                actors += group.len();
                match (tide, group.len()) {
                    (0 | 1, 1) => singleton_groups += 1,
                    (0 | 1, 2) => doubleton_groups += 1,
                    (2, 0) => empty_high_groups += 1,
                    _ => panic!(
                        "unexpected Artillery group: {} {direction} {tide}",
                        stage.name
                    ),
                }
            }
        }
    }
    assert_eq!(
        (
            actors,
            singleton_groups,
            doubleton_groups,
            empty_high_groups
        ),
        (97, 83, 7, 45)
    );

    let carousel = stage_spawns("Cop_Carousel").unwrap();
    let [point] = carousel.artillery_group(0, 0).unwrap() else {
        panic!("expected one Carousel low-tide Artillery point");
    };
    assert_eq!(point.actor_index, 369);
    assert_eq!(point.hash, 0x0c7e_9892_8f2e_8395);
    assert_eq!(
        point.translate.map(f32::to_bits),
        [0x41bc_d744, 0x4100_0000, 0xc1b6_7bb2]
    );
    assert_eq!(point.rotate, None);
    assert!(carousel.artillery_group(3, 0).is_none());
    assert!(carousel.artillery_group(0, 3).is_none());
}

#[test]
fn tower_arrivals_cover_every_tide_and_direction_without_inferring_runtime_order() {
    let mut total = 0;
    let mut counts = [0usize; 7];
    for stage in all_stages() {
        for direction in 0..3 {
            for tide in 0..3 {
                let group = stage.tower_arrival_group(tide, direction).unwrap();
                total += group.len();
                counts[group.len()] += 1;
            }
        }
    }
    assert_eq!(total, 433);
    assert_eq!(counts, [0, 0, 0, 116, 14, 1, 4]);
    assert_eq!(
        stage_spawns("Cop_Factory")
            .unwrap()
            .tower_arrival_group(0, 0)
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        stage_spawns("Cop_Shakehighway")
            .unwrap()
            .tower_arrival_group(0, 0)
            .unwrap()
            .len(),
        6
    );
    let carousel = stage_spawns("Cop_Carousel").unwrap();
    assert!(carousel.tower_arrival_group(3, 0).is_none());
    assert!(carousel.tower_arrival_group(0, 3).is_none());
}
