use salmon_wave_lookahead::weapons::{
    HelperNormalPool, SceneRarePool, StandardRandomError, StandardRandomInput, ThirdPoolError,
    WEAPON_RNG_DOMAIN, WeaponCandidateRing, WeaponError, WeaponGridBuildError,
    WeaponGridBuildInput, WeaponPoolMembership, WeaponRandomPool, WeaponSlotSource,
    build_weapon_grid, choose_standard_random_pool, draw_standard_random_node_value,
    draw_third_pool_index, draw_third_pool_node_value, postprocess_weapon_grid,
    weapon_pool_membership, weapon_slot_sequences, weapon_slot_source,
};
use salmon_wave_lookahead::{
    SeedWords,
    rng::{XorShift128, derive},
    weapon_catalog::{EffectiveWeaponPool, resolve_normal_pool, resolve_third_pool},
};

fn normal_pool(id: i32) -> EffectiveWeaponPool {
    resolve_normal_pool(0, 1, |_| Ok::<_, ()>(Some(id))).unwrap()
}

fn grid_input<'a>(
    candidates: WeaponCandidateRing<'a>,
    normal: &'a EffectiveWeaponPool,
    third: &'a EffectiveWeaponPool,
) -> WeaponGridBuildInput<'a> {
    WeaponGridBuildInput {
        ready: true,
        candidates,
        wave_three_override: false,
        normal,
        third,
        threshold: 0.0,
        scene_rare: None,
    }
}

#[test]
fn full_id_membership_is_third_first_and_direct_packing_has_zero_high_bits() {
    let normal = normal_pool(0x1_0001);
    let third = resolve_third_pool(1, 1);
    assert_eq!(normal.stored_ids, [0x1_0001]);
    assert_eq!(normal.draw_ids, [1]);
    assert_eq!(
        weapon_pool_membership(0x1_0001, &normal, &third),
        WeaponPoolMembership::Normal
    );
    assert_eq!(
        weapon_pool_membership(1, &normal, &third),
        WeaponPoolMembership::Missing
    );
    let both = normal_pool(20900);
    assert_eq!(
        weapon_pool_membership(20900, &both, &third),
        WeaponPoolMembership::Third
    );

    let ids = [0x1_0001; 4];
    let ring = WeaponCandidateRing {
        physical_ids: &ids,
        start: 0,
        count: 4,
    };
    let before = XorShift128::from_words([1, 2, 3, 4]);
    let mut rng = before;
    let grid = build_weapon_grid(&mut rng, grid_input(ring, &normal, &third), |_| {
        panic!("validated direct IDs do not require asset lookups")
    })
    .unwrap()
    .unwrap();
    assert!(grid.iter().flatten().all(|&value| value == 1));
    assert_eq!(rng, before);
}

#[test]
fn low16_only_match_becomes_standard_random_and_consumes_rng() {
    let normal = normal_pool(0x1_0001);
    let third = resolve_third_pool(1, 1);
    let ids = [1; 4];
    let ring = WeaponCandidateRing {
        physical_ids: &ids,
        start: 0,
        count: 4,
    };
    let before = XorShift128::from_words([1, 2, 3, 4]);
    let mut rng = before;
    let grid = build_weapon_grid(&mut rng, grid_input(ring, &normal, &third), |_| false)
        .unwrap()
        .unwrap();
    assert!(
        grid.iter()
            .flatten()
            .all(|&value| value as u32 == 0x0101_0001)
    );
    assert_ne!(rng, before);
}

#[test]
fn grid_random_draws_interleave_row_major() {
    let normal = normal_pool(123);
    let third = resolve_third_pool(1, 2);
    let ids = [-2, -1, 123, -2];
    let ring = WeaponCandidateRing {
        physical_ids: &ids,
        start: 0,
        count: 4,
    };
    let input = grid_input(ring, &normal, &third);
    let initial = [0x1234_5678, 0x9abc_def0, 0x1357_9bdf, 0x2468_ace0];
    let mut actual_rng = XorShift128::from_words(initial);
    let grid = build_weapon_grid(&mut actual_rng, input, |_| panic!("helper branch only"))
        .unwrap()
        .unwrap();

    // Independently calculated from the row-major branch sequence and the
    // xorshift recurrence, without replaying the Rust slot/draw helpers.
    assert_eq!(
        grid,
        [
            [0x0101_5974, 0x0101_007b, 0x007b, 0x0101_51a4],
            [0x0101_007b, 0x007b, 0x0101_5974, 0x0101_5974],
            [0x007b, 0x0101_51a4, 0x0101_5974, 0x0101_007b],
            [0x0101_51a4, 0x0101_5974, 0x0101_007b, 0x007b],
            [0x0101_5974, 0x0101_007b, 0x007b, 0x0101_51a4],
        ]
    );
    assert_eq!(
        actual_rng.as_array(),
        [0x886d_7ade, 0xfa30_4203, 0x16dd_dbad, 0x7eed_b567]
    );
}

#[test]
fn wave_three_override_draws_four_third_pool_nodes_from_positive_candidates() {
    let normal = normal_pool(123);
    let third = resolve_third_pool(1, 2);
    let ids = [123; 4];
    let ring = WeaponCandidateRing {
        physical_ids: &ids,
        start: 0,
        count: 4,
    };
    let mut input = grid_input(ring, &normal, &third);
    input.wave_three_override = true;
    let mut rng = XorShift128::from_words([1, 2, 3, 4]);
    let mut expected_rng = rng;
    for _ in 0..4 {
        expected_rng.next32();
    }
    let grid = build_weapon_grid(&mut rng, input, |_| false)
        .unwrap()
        .unwrap();
    assert!(
        grid.iter()
            .enumerate()
            .all(|(wave, row)| row.iter().all(|&value| {
                if wave == 3 {
                    value as u32 & 0xffff_0000 == 0x0101_0000
                } else {
                    value == 123
                }
            }))
    );
    assert_eq!(rng, expected_rng);
}

#[test]
fn readiness_skip_and_kind_guard_do_not_draw() {
    let normal = normal_pool(123);
    let third = resolve_third_pool(1, 1);
    let ring = WeaponCandidateRing {
        physical_ids: &[],
        start: 0,
        count: 0,
    };
    let mut input = grid_input(ring, &third, &normal);
    let before = XorShift128::from_words([1, 2, 3, 4]);
    let mut rng = before;
    input.ready = false;
    assert_eq!(
        build_weapon_grid(&mut rng, input, |_| panic!("skip")),
        Ok(None)
    );
    assert_eq!(rng, before);
    input.ready = true;
    assert!(matches!(
        build_weapon_grid(&mut rng, input, |_| panic!("wrong kind")),
        Err(WeaponGridBuildError::NormalPoolKind(_))
    ));
    assert_eq!(rng, before);
    let wrong_third = grid_input(ring, &normal, &normal);
    assert_eq!(
        build_weapon_grid(&mut rng, wrong_third, |_| panic!("wrong third kind")),
        Err(WeaponGridBuildError::ThirdPoolKind(normal.kind))
    );
    assert_eq!(rng, before);

    let bad_ring = WeaponCandidateRing {
        physical_ids: &[1],
        start: 1,
        count: 1,
    };
    assert!(matches!(
        build_weapon_grid(&mut rng, grid_input(bad_ring, &normal, &third), |_| panic!(
            "invalid ring"
        )),
        Err(WeaponGridBuildError::Slot(WeaponError::InvalidRing { .. }))
    ));
    assert_eq!(rng, before);
}

#[test]
fn malformed_effective_pool_is_rejected_before_any_draw() {
    let normal = normal_pool(123);
    let third = resolve_third_pool(1, 1);
    let ids = [-1];
    let ring = WeaponCandidateRing {
        physical_ids: &ids,
        start: 0,
        count: 1,
    };
    let before = XorShift128::from_words([1, 2, 3, 4]);
    for flaw in 0..3 {
        let mut bad_normal = normal.clone();
        match flaw {
            0 => bad_normal.stored_ids.clear(),
            1 => bad_normal.sources.clear(),
            _ => bad_normal.draw_ids[0] = 124,
        }
        let mut rng = before;
        assert_eq!(
            build_weapon_grid(&mut rng, grid_input(ring, &bad_normal, &third), |_| panic!(
                "malformed normal"
            )),
            Err(WeaponGridBuildError::MalformedNormalPool)
        );
        assert_eq!(rng, before);
    }
    let mut bad_third = third.clone();
    bad_third.draw_ids[0] = 0;
    let mut rng = before;
    assert_eq!(
        build_weapon_grid(&mut rng, grid_input(ring, &normal, &bad_third), |_| panic!(
            "malformed third"
        )),
        Err(WeaponGridBuildError::MalformedThirdPool)
    );
    assert_eq!(rng, before);
}

#[test]
fn missing_pool_entry_errors_preserve_already_consumed_draws() {
    let normal =
        resolve_normal_pool(-1, 0, |_| -> Result<Option<i32>, ()> { panic!("no rows") }).unwrap();
    let third = resolve_third_pool(0, 0);
    let initial = [0x1234_5678, 0x9abc_def0, 0x1357_9bdf, 0x2468_ace0];
    let third_ids = [-2];
    let third_ring = WeaponCandidateRing {
        physical_ids: &third_ids,
        start: 0,
        count: 1,
    };
    let mut rng = XorShift128::from_words(initial);
    let mut expected = rng;
    expected.next32();
    assert_eq!(
        build_weapon_grid(&mut rng, grid_input(third_ring, &normal, &third), |_| false),
        Err(WeaponGridBuildError::Third(ThirdPoolError::MissingEntry(0)))
    );
    assert_eq!(rng, expected);

    let normal_ids = [-1];
    let normal_ring = WeaponCandidateRing {
        physical_ids: &normal_ids,
        start: 0,
        count: 1,
    };
    let mut rng = XorShift128::from_words(initial);
    let mut expected = rng;
    expected.next32();
    expected.next32();
    assert_eq!(
        build_weapon_grid(&mut rng, grid_input(normal_ring, &normal, &third), |_| {
            false
        }),
        Err(WeaponGridBuildError::Standard(
            StandardRandomError::MissingNormalEntry(0)
        ))
    );
    assert_eq!(rng, expected);
}

#[test]
fn built_grid_can_enter_existing_seven_draw_postprocess() {
    let normal = normal_pool(123);
    let third = resolve_third_pool(1, 1);
    let ids = [123; 4];
    let ring = WeaponCandidateRing {
        physical_ids: &ids,
        start: 0,
        count: 4,
    };
    let mut rng = XorShift128::from_words([1, 2, 3, 4]);
    let mut grid = build_weapon_grid(&mut rng, grid_input(ring, &normal, &third), |_| false)
        .unwrap()
        .unwrap();
    let before_postprocess = rng;
    let sources = postprocess_weapon_grid(&mut rng, &mut grid, false);
    assert_eq!(sources.slot_sources.len(), 4);
    assert_eq!(sources.wave_sources.len(), 5);
    assert!(grid.iter().flatten().all(|&value| value == 123));
    assert_ne!(rng, before_postprocess);
}

fn source(ids: &[i32], start: usize, count: usize, wave: u8, slot: u8) -> WeaponSlotSource {
    weapon_slot_source(
        &WeaponCandidateRing {
            physical_ids: ids,
            start,
            count,
        },
        wave,
        slot,
        false,
    )
    .unwrap()
}

#[test]
fn four_candidates_rotate_over_wave_and_slot_indices() {
    let ids = [100, 101, 102, 103];
    for (wave, expected) in [
        (0, [100, 101, 102, 103]),
        (1, [101, 102, 103, 100]),
        (3, [103, 100, 101, 102]),
        (4, [100, 101, 102, 103]),
    ] {
        for (slot, id) in expected.into_iter().enumerate() {
            assert_eq!(
                source(&ids, 0, 4, wave, slot as u8),
                WeaponSlotSource::CandidateNeedsValidation(id)
            );
        }
    }
}

#[test]
fn short_ring_uses_physical_first_for_out_of_count_rotation() {
    let ids = [100, 101, 102];
    assert_eq!(
        source(&ids, 0, 3, 1, 2),
        WeaponSlotSource::CandidateNeedsValidation(100)
    );
    assert_eq!(source(&ids, 0, 3, 0, 3), WeaponSlotSource::StandardRandom);
    assert_eq!(
        source(&[200, 201], 0, 2, 1, 1),
        WeaponSlotSource::CandidateNeedsValidation(200)
    );
    assert_eq!(
        source(&[300], 0, 1, 2, 0),
        WeaponSlotSource::CandidateNeedsValidation(300)
    );
    assert_eq!(source(&[300], 0, 1, 2, 1), WeaponSlotSource::StandardRandom);
}

#[test]
fn nonzero_ring_start_wraps_but_fallback_stays_at_physical_first() {
    let ids = [10, 11, 12, 13];
    assert_eq!(
        source(&ids, 3, 4, 0, 0),
        WeaponSlotSource::CandidateNeedsValidation(13)
    );
    assert_eq!(
        source(&ids, 3, 4, 0, 1),
        WeaponSlotSource::CandidateNeedsValidation(10)
    );
    assert_eq!(
        source(&ids, 3, 4, 0, 3),
        WeaponSlotSource::CandidateNeedsValidation(12)
    );
    assert_eq!(
        source(&ids, 2, 3, 0, 0),
        WeaponSlotSource::CandidateNeedsValidation(12)
    );
    assert_eq!(
        source(&ids, 2, 3, 1, 2),
        WeaponSlotSource::CandidateNeedsValidation(10)
    );
}

#[test]
fn negative_sentinels_and_wave_three_runtime_override_are_distinct() {
    let ids = [-2, -1, -7, 42];
    assert_eq!(source(&ids, 0, 4, 0, 0), WeaponSlotSource::ThirdPoolRandom);
    assert_eq!(source(&ids, 0, 4, 0, 1), WeaponSlotSource::StandardRandom);
    assert_eq!(source(&ids, 0, 4, 0, 2), WeaponSlotSource::StandardRandom);
    assert_eq!(
        source(&ids, 0, 4, 0, 3),
        WeaponSlotSource::CandidateNeedsValidation(42)
    );
    for slot in 0..4 {
        assert_eq!(
            weapon_slot_source(
                &WeaponCandidateRing {
                    physical_ids: &[],
                    start: 0,
                    count: 0,
                },
                3,
                slot,
                true,
            ),
            Ok(WeaponSlotSource::ThirdPoolRandom)
        );
    }
    assert_eq!(source(&[], 0, 0, 3, 0), WeaponSlotSource::StandardRandom);
}

#[test]
fn invalid_wave_slot_and_ring_state_are_rejected() {
    let ring = WeaponCandidateRing {
        physical_ids: &[1, 2],
        start: 0,
        count: 2,
    };
    assert_eq!(
        weapon_slot_source(&ring, 5, 0, false),
        Err(WeaponError::InvalidWaveIndex(5))
    );
    assert_eq!(
        weapon_slot_source(&ring, 0, 4, false),
        Err(WeaponError::InvalidSlotIndex(4))
    );
    for (ids, start, count) in [(&[][..], 0, 1), (&[1, 2][..], 2, 1), (&[1, 2][..], 0, 3)] {
        assert_eq!(
            weapon_slot_source(
                &WeaponCandidateRing {
                    physical_ids: ids,
                    start,
                    count,
                },
                0,
                0,
                false,
            ),
            Err(WeaponError::InvalidRing {
                capacity: ids.len(),
                start,
                count,
            })
        );
    }
}

#[test]
fn third_pool_index_uses_signed_count_and_consumes_zero_count_draw() {
    let initial = [0x1234_5678, 0x9abc_def0, 0x1357_9bdf, 0x2468_ace0];
    let after_one = [0x9abc_def0, 0x1357_9bdf, 0x2468_ace0, 0x945f_b983];

    let mut rng = XorShift128::from_words(initial);
    assert_eq!(draw_third_pool_index(&mut rng, -1), 0);
    assert_eq!(rng.as_array(), initial);

    for (count, expected_index) in [(0, 0), (1, 0), (3, 1), (10, 5)] {
        let mut rng = XorShift128::from_words(initial);
        assert_eq!(draw_third_pool_index(&mut rng, count), expected_index);
        assert_eq!(rng.as_array(), after_one);
    }
}

#[test]
fn third_pool_resolves_id_and_packed_value_with_signed_draw_rules() {
    let initial = [0x1234_5678, 0x9abc_def0, 0x1357_9bdf, 0x2468_ace0];
    let mut rng = XorShift128::from_words(initial);
    let result = draw_third_pool_node_value(&mut rng, 3, &[100, 200, 300]).unwrap();
    assert_eq!(result.resolved_id, 200);
    assert_eq!(result.packed_value as u32, 0x0101_00c8);
    assert_eq!(rng.as_array()[3], 0x945f_b983);

    for (count, expected_last) in [(-1, initial[3]), (0, 0x945f_b983)] {
        let mut rng = XorShift128::from_words(initial);
        let result = draw_third_pool_node_value(&mut rng, count, &[42]).unwrap();
        assert_eq!(result.resolved_id, 42);
        assert_eq!(rng.as_array()[3], expected_last);
    }
}

#[test]
fn third_pool_reports_missing_index_without_undoing_its_draw() {
    let initial = [0x1234_5678, 0x9abc_def0, 0x1357_9bdf, 0x2468_ace0];
    let mut rng = XorShift128::from_words(initial);
    assert_eq!(
        draw_third_pool_node_value(&mut rng, 0, &[]),
        Err(ThirdPoolError::MissingEntry(0))
    );
    assert_eq!(rng.as_array()[3], 0x945f_b983);
}

#[test]
fn standard_random_pool_uses_f32_bits_threshold_and_one_draw() {
    let initial = [0x1234_5678, 0x9abc_def0, 0x1357_9bdf, 0x2468_ace0];
    let after_one = [0x9abc_def0, 0x1357_9bdf, 0x2468_ace0, 0x945f_b983];
    let sample = f32::from_bits(0x3fca_2fdc) - 1.0;
    assert_eq!(sample, 0.579_585_55_f32);
    for (threshold, expected) in [
        (0.5, WeaponRandomPool::HelperNormal),
        (0.6, WeaponRandomPool::SceneRare),
        (sample, WeaponRandomPool::HelperNormal),
        (f32::NAN, WeaponRandomPool::SceneRare),
    ] {
        let mut rng = XorShift128::from_words(initial);
        assert_eq!(choose_standard_random_pool(&mut rng, threshold), expected);
        assert_eq!(rng.as_array(), after_one);
    }
}

fn standard_input<'a>(normal: HelperNormalPool<'a>) -> StandardRandomInput<'a> {
    StandardRandomInput {
        source_id: -1,
        threshold: 0.5,
        normal,
        scene_rare: None,
    }
}

#[test]
fn standard_random_helper_pool_resolves_packed_node_after_two_draws() {
    let initial = [0x1234_5678, 0x9abc_def0, 0x1357_9bdf, 0x2468_ace0];
    let mut rng = XorShift128::from_words(initial);
    let result = draw_standard_random_node_value(
        &mut rng,
        standard_input(HelperNormalPool {
            signed_count: 3,
            ids: &[100, 200, 300],
        }),
        |_| panic!("normal pool does not request asset lookup"),
    )
    .unwrap();
    assert_eq!(result.pool, WeaponRandomPool::HelperNormal);
    assert_eq!(result.resolved_id, 300);
    assert_eq!(result.packed_value as u32, 0x0101_012c);
    assert_eq!(
        rng.as_array(),
        [0x1357_9bdf, 0x2468_ace0, 0x945f_b983, 0xe868_bea6]
    );
}

#[test]
fn scene_rare_ring_wraps_and_asset_gate_can_replace_id() {
    let initial = [0x1234_5678, 0x9abc_def0, 0x1357_9bdf, 0x2468_ace0];
    let input = StandardRandomInput {
        threshold: 0.6,
        scene_rare: Some(SceneRarePool {
            physical_ids: &[1000, 2000, 3000],
            start: 1,
            signed_count: 3,
        }),
        ..standard_input(HelperNormalPool {
            signed_count: 1,
            ids: &[99],
        })
    };
    let mut rng = XorShift128::from_words(initial);
    let result = draw_standard_random_node_value(&mut rng, input, |id| id == 1000).unwrap();
    assert_eq!(result.pool, WeaponRandomPool::SceneRare);
    assert_eq!(result.resolved_id, 1000);
    assert_eq!(result.packed_value as u32, 0x0101_03e8);
    assert_eq!(
        rng.as_array(),
        [0x1357_9bdf, 0x2468_ace0, 0x945f_b983, 0xe868_bea6]
    );

    let mut rng = XorShift128::from_words(initial);
    let rejected = draw_standard_random_node_value(&mut rng, input, |_| false).unwrap();
    assert_eq!(rejected.resolved_id, 20900);
    assert_eq!(rejected.packed_value as u32, 0x0101_51a4);

    let mut rng = XorShift128::from_words(initial);
    let two_entries = StandardRandomInput {
        scene_rare: Some(SceneRarePool {
            physical_ids: &[111, 222],
            start: 1,
            signed_count: 2,
        }),
        ..input
    };
    let result = draw_standard_random_node_value(&mut rng, two_entries, |_| true).unwrap();
    assert_eq!(result.resolved_id, 111);
    assert_eq!(rng.as_array()[3], 0xe868_bea6);
}

#[test]
fn short_and_missing_pool_branches_have_distinct_draw_counts() {
    let initial = [0x1234_5678, 0x9abc_def0, 0x1357_9bdf, 0x2468_ace0];
    for (count, expected_last) in [(-1, 0x945f_b983), (0, 0xe868_bea6), (1, 0xe868_bea6)] {
        let mut rng = XorShift128::from_words(initial);
        let result = draw_standard_random_node_value(
            &mut rng,
            standard_input(HelperNormalPool {
                signed_count: count,
                ids: &[42],
            }),
            |_| false,
        )
        .unwrap();
        assert_eq!(result.resolved_id, 42);
        assert_eq!(rng.as_array()[3], expected_last);
    }

    let mut rng = XorShift128::from_words(initial);
    let absent = draw_standard_random_node_value(
        &mut rng,
        StandardRandomInput {
            threshold: 0.6,
            ..standard_input(HelperNormalPool {
                signed_count: 1,
                ids: &[42],
            })
        },
        |_| panic!("absent scene has no asset lookup"),
    )
    .unwrap();
    assert_eq!(absent.resolved_id, 20900);
    assert_eq!(rng.as_array()[3], 0x945f_b983);

    for count in [-1, 0, 1] {
        let mut rng = XorShift128::from_words(initial);
        let input = StandardRandomInput {
            threshold: 0.6,
            scene_rare: Some(SceneRarePool {
                physical_ids: &[777],
                start: 0,
                signed_count: count,
            }),
            ..standard_input(HelperNormalPool {
                signed_count: 1,
                ids: &[42],
            })
        };
        let expected_candidate = if count <= 0 { -1 } else { 777 };
        let result = draw_standard_random_node_value(&mut rng, input, |candidate| {
            assert_eq!(candidate, expected_candidate);
            true
        })
        .unwrap();
        assert_eq!(result.resolved_id, expected_candidate);
        assert_eq!(rng.as_array()[3], 0x945f_b983);
    }
}

#[test]
fn standard_random_rejects_wrong_source_and_missing_pool_entries() {
    let initial = [0x1234_5678, 0x9abc_def0, 0x1357_9bdf, 0x2468_ace0];
    for source_id in [-2, 0] {
        let mut rng = XorShift128::from_words(initial);
        let input = StandardRandomInput {
            source_id,
            ..standard_input(HelperNormalPool {
                signed_count: 1,
                ids: &[42],
            })
        };
        assert_eq!(
            draw_standard_random_node_value(&mut rng, input, |_| true),
            Err(StandardRandomError::UnsupportedSource(source_id))
        );
        assert_eq!(rng.as_array(), initial);
    }
    let mut rng = XorShift128::from_words(initial);
    assert_eq!(
        draw_standard_random_node_value(
            &mut rng,
            standard_input(HelperNormalPool {
                signed_count: -1,
                ids: &[]
            }),
            |_| true,
        ),
        Err(StandardRandomError::MissingNormalEntry(0))
    );
    assert_eq!(rng.as_array()[3], 0x945f_b983);

    let mut rng = XorShift128::from_words(initial);
    let invalid_rare = StandardRandomInput {
        threshold: 0.6,
        scene_rare: Some(SceneRarePool {
            physical_ids: &[777],
            start: 0,
            signed_count: 2,
        }),
        ..standard_input(HelperNormalPool {
            signed_count: 1,
            ids: &[42],
        })
    };
    assert_eq!(
        draw_standard_random_node_value(&mut rng, invalid_rare, |_| true),
        Err(StandardRandomError::InvalidSceneRing {
            capacity: 1,
            start: 0,
            count: 2,
        })
    );
    assert_eq!(rng.as_array()[3], 0x945f_b983);
}

fn concrete_weapon_grid() -> [[i32; 4]; 5] {
    std::array::from_fn(|wave| std::array::from_fn(|slot| (10 * wave + slot) as i32))
}

fn postpool_test_rng() -> XorShift128 {
    let mut rng = derive(SeedWords::from_scene([1, 2, 3, 4]), WEAPON_RNG_DOMAIN);
    assert_eq!(
        rng.as_array(),
        [0x0331_7883, 0x4b97_a6b1, 0xc9d8_f373, 0xa4ee_fb34]
    );
    // Six earlier draws are a test-only boundary state, not a claim about the
    // game's actual preprocessing and pool-draw count.
    for _ in 0..6 {
        rng.next32();
    }
    rng
}

#[test]
fn seven_draw_weapon_postprocess_permutes_slots_then_wave_rows_and_copies_last() {
    let mut grid = concrete_weapon_grid();
    let mut rng = postpool_test_rng();
    let sources = postprocess_weapon_grid(&mut rng, &mut grid, false);
    assert_eq!(sources.slot_sources, [0, 2, 1, 3]);
    assert_eq!(sources.wave_sources, [3, 1, 0, 2, 3]);
    assert_eq!(
        grid,
        [
            [30, 32, 31, 33],
            [10, 12, 11, 13],
            [0, 2, 1, 3],
            [20, 22, 21, 23],
            [30, 32, 31, 33],
        ]
    );
    assert_eq!(
        rng.as_array(),
        [0x841e_52f5, 0x128d_e263, 0xab71_ade8, 0x4c03_f737]
    );
}

#[test]
fn postprocessed_grid_becomes_four_ordered_slot_sequences() {
    let mut grid = concrete_weapon_grid();
    let mut rng = postpool_test_rng();
    postprocess_weapon_grid(&mut rng, &mut grid, false);
    assert_eq!(
        weapon_slot_sequences(&grid),
        [
            vec![30, 10, 0, 20],
            vec![32, 12, 2, 22],
            vec![31, 11, 1, 21],
            vec![33, 13, 3, 23],
        ]
    );
}

#[test]
fn slot_duplicate_filter_uses_low_sixteen_bits_and_skips_shifted_neighbors() {
    let ids = [0x10001, 0x20001, 0x30001, 0x40002, 0x50003];
    let grid = std::array::from_fn(|wave| [ids[wave], 7, wave as i32, 100 + wave as i32]);
    let sequences = weapon_slot_sequences(&grid);
    assert_eq!(sequences[0], [0x10001, 0x30001, 0x40002, 0x50003]);
    assert_eq!(sequences[1], [7, 7]);
    assert_eq!(sequences[2], [0, 1, 2, 3, 4]);
    assert_eq!(sequences[3], [100, 101, 102, 103, 104]);
}

#[test]
fn wave_three_swap_suppression_preserves_its_draw_and_all_later_steps() {
    let mut grid = concrete_weapon_grid();
    let mut rng = postpool_test_rng();
    let sources = postprocess_weapon_grid(&mut rng, &mut grid, true);
    assert_eq!(sources.slot_sources, [0, 2, 1, 3]);
    assert_eq!(sources.wave_sources, [2, 1, 0, 3, 2]);
    assert_eq!(
        grid,
        [
            [20, 22, 21, 23],
            [10, 12, 11, 13],
            [0, 2, 1, 3],
            [30, 32, 31, 33],
            [20, 22, 21, 23],
        ]
    );
    assert_eq!(
        rng.as_array(),
        [0x841e_52f5, 0x128d_e263, 0xab71_ade8, 0x4c03_f737]
    );
}
