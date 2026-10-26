use ocean_calc::{
    SeedWords,
    specials::{
        MainRing, MainRingKind, PrivateSpecialInput, SpecialBranch, SpecialError, SpecialLookup,
        SpecialReorder, predict_private_specials,
    },
};

const LOW: [u16; 7] = [20006, 20012, 20010, 20009, 20014, 20013, 20007];
const PRIOR: [u16; 5] = [20006, 20012, 20010, 20009, 20014];
const V6: [u16; 4] = [20013, 20007, 20017, 20018];

fn input(effective_g: u32) -> PrivateSpecialInput<'static> {
    PrivateSpecialInput {
        seed: SeedWords::from_scene([1, 2, 3, 4]),
        effective_g,
        low_candidates: &LOW,
        prior_candidates: &PRIOR,
        v6_candidates: &V6,
        reorder: SpecialReorder::Inactive,
    }
}

#[test]
fn owned_low_vector_uses_private_domain_and_six_draws() {
    let prediction = predict_private_specials(input(0), SpecialLookup::Known).unwrap();
    assert_eq!(prediction.branch, SpecialBranch::Low);
    assert_eq!(prediction.initial_specials, [20013, 20010, 20007, 20014]);
    assert_eq!(prediction.final_specials, prediction.initial_specials);
    assert!(!prediction.remap_applied);
    assert_eq!(prediction.rng.draws, 6);
    assert_eq!(
        prediction.rng.state.as_array(),
        [0x30ad_ec15, 0x1872_0128, 0x84a3_ecff, 0x9eaf_0a16]
    );
}

#[test]
fn owned_high_vector_uses_all_three_shuffles_and_thirteen_draws() {
    let prediction = predict_private_specials(input(3 << 17), SpecialLookup::Known).unwrap();
    assert_eq!(prediction.branch, SpecialBranch::High);
    assert_eq!(prediction.initial_specials, [20010, 20013, 20006, 20014]);
    assert_eq!(prediction.final_specials, prediction.initial_specials);
    assert_eq!(prediction.rng.draws, 13);
    assert_eq!(
        prediction.rng.state.as_array(),
        [0x3488_34f0, 0x191c_8bf3, 0x2578_825e, 0x7ce8_54e4]
    );
}

#[test]
fn effective_g_branch_boundary_is_unsigned_shift_seventeen() {
    let low = predict_private_specials(input((3 << 17) - 1), SpecialLookup::Known).unwrap();
    let high = predict_private_specials(input(3 << 17), SpecialLookup::Known).unwrap();
    assert_eq!(low.branch, SpecialBranch::Low);
    assert_eq!(high.branch, SpecialBranch::High);
}

#[test]
fn low_looks_up_shuffled_candidates_and_stops_at_output_capacity() {
    let mut seen = Vec::new();
    let prediction = predict_private_specials(input(0), |candidate| {
        seen.push(candidate);
        SpecialLookup::Known(candidate + 1)
    })
    .unwrap();
    assert_eq!(seen, [20013, 20010, 20007, 20014]);
    assert_eq!(prediction.initial_specials, [20014, 20011, 20008, 20015]);
}

#[test]
fn high_missing_lookup_skips_without_resampling_or_extra_draws() {
    let mut seen = Vec::new();
    let result = predict_private_specials(input(3 << 17), |candidate| {
        seen.push(candidate);
        if candidate == 20013 {
            SpecialLookup::Missing
        } else {
            SpecialLookup::Known(candidate)
        }
    });
    assert_eq!(seen, [20010, 20013, 20006, 20014]);
    assert_eq!(
        result.unwrap_err(),
        SpecialError::InsufficientSpecials { found: 3 }
    );
}

#[test]
fn low_missing_selected_lookup_is_unsupported() {
    assert_eq!(
        predict_private_specials(input(0), |_| SpecialLookup::Missing).unwrap_err(),
        SpecialError::LowMissingAsset { candidate: 20013 }
    );
}

#[test]
fn unknown_selected_lookup_never_fabricates_special_ids() {
    for effective_g in [0, 3 << 17] {
        let candidate = if effective_g == 0 { 20013 } else { 20010 };
        assert_eq!(
            predict_private_specials(input(effective_g), |_| SpecialLookup::Unknown).unwrap_err(),
            SpecialError::UnknownAsset { candidate }
        );
    }
}

#[test]
fn short_or_empty_effective_source_cannot_return_a_four_player_assignment() {
    let mut short = input(0);
    short.low_candidates = &[1, 2, 3];
    assert_eq!(
        predict_private_specials(short, SpecialLookup::Known).unwrap_err(),
        SpecialError::InsufficientSpecials { found: 3 }
    );
    short.low_candidates = &[];
    assert_eq!(
        predict_private_specials(short, SpecialLookup::Known).unwrap_err(),
        SpecialError::InsufficientSpecials { found: 0 }
    );
    let mut short_high = input(3 << 17);
    short_high.prior_candidates = &[];
    short_high.v6_candidates = &[1, 2, 3, 4];
    assert_eq!(
        predict_private_specials(short_high, SpecialLookup::Known).unwrap_err(),
        SpecialError::InsufficientSpecials { found: 2 }
    );
}

fn ring(physical_ids: &[i32], start: usize, count: usize) -> MainRing<'_> {
    MainRing {
        physical_ids,
        start,
        count,
    }
}

#[test]
fn full_permutation_remaps_from_snapshot_without_consuming_rng() {
    let mut state = input(0);
    state.reorder = SpecialReorder::Active {
        current_main_ring: ring(&[40, 30, 20, 10], 0, 4),
        source_main_ring: ring(&[10, 20, 30, 40], 0, 4),
        active_count: 4,
    };
    let prediction = predict_private_specials(state, SpecialLookup::Known).unwrap();
    assert_eq!(prediction.initial_specials, [20013, 20010, 20007, 20014]);
    assert_eq!(prediction.final_specials, [20014, 20007, 20010, 20013]);
    assert!(prediction.remap_applied);
    assert_eq!(prediction.rng.draws, 6);
    assert_eq!(
        prediction.rng.state.as_array(),
        [0x30ad_ec15, 0x1872_0128, 0x84a3_ecff, 0x9eaf_0a16]
    );
}

#[test]
fn independently_wrapped_duplicate_rings_choose_first_logical_source_match() {
    let mut state = input(0);
    state.reorder = SpecialReorder::Active {
        current_main_ring: ring(&[10, 30, 10, 20], 1, 4),
        source_main_ring: ring(&[20, 10, 30, 10], 3, 4),
        active_count: 4,
    };
    let prediction = predict_private_specials(state, SpecialLookup::Known).unwrap();
    assert_eq!(prediction.final_specials, [20014, 20013, 20010, 20013]);
    assert!(prediction.remap_applied);
}

#[test]
fn too_few_matches_preserve_all_initial_assignments() {
    let mut state = input(0);
    state.reorder = SpecialReorder::Active {
        current_main_ring: ring(&[30, 10, 99, 98], 0, 4),
        source_main_ring: ring(&[10, 20, 30, 40], 0, 4),
        active_count: 3,
    };
    let prediction = predict_private_specials(state, SpecialLookup::Known).unwrap();
    assert_eq!(prediction.final_specials, [20013, 20010, 20007, 20014]);
    assert!(!prediction.remap_applied);
}

#[test]
fn gate_passing_partial_mapping_preserves_unmatched_destination() {
    let mut state = input(0);
    state.reorder = SpecialReorder::Active {
        current_main_ring: ring(&[30, 10, 99, 20], 0, 4),
        source_main_ring: ring(&[10, 20, 30, 40], 0, 4),
        active_count: 3,
    };
    let prediction = predict_private_specials(state, SpecialLookup::Known).unwrap();
    assert_eq!(prediction.final_specials, [20007, 20013, 20007, 20010]);
    assert!(prediction.remap_applied);
}

#[test]
fn shorter_logical_rings_only_remap_their_present_matching_slots() {
    let mut state = input(0);
    state.reorder = SpecialReorder::Active {
        current_main_ring: ring(&[10, 30, 99], 1, 2),
        source_main_ring: ring(&[10, 20, 30], 0, 3),
        active_count: 1,
    };
    let prediction = predict_private_specials(state, SpecialLookup::Known).unwrap();
    assert_eq!(prediction.final_specials, [20007, 20010, 20007, 20014]);
    assert!(prediction.remap_applied);
}

#[test]
fn zero_active_count_passes_even_for_empty_rings() {
    let mut state = input(0);
    state.reorder = SpecialReorder::Active {
        current_main_ring: ring(&[], 0, 0),
        source_main_ring: ring(&[], 0, 0),
        active_count: 0,
    };
    let prediction = predict_private_specials(state, SpecialLookup::Known).unwrap();
    assert_eq!(prediction.final_specials, [20013, 20010, 20007, 20014]);
    assert!(prediction.remap_applied);
}

#[test]
fn zero_active_count_still_applies_existing_matches() {
    let mut state = input(0);
    state.reorder = SpecialReorder::Active {
        current_main_ring: ring(&[30, 99], 0, 2),
        source_main_ring: ring(&[10, 20, 30, 40], 0, 4),
        active_count: 0,
    };
    let prediction = predict_private_specials(state, SpecialLookup::Known).unwrap();
    assert_eq!(prediction.final_specials, [20007, 20010, 20007, 20014]);
    assert!(prediction.remap_applied);
}

#[test]
fn malformed_rings_and_participant_counts_are_typed_errors() {
    for malformed in [
        ring(&[1, 2, 3, 4, 5], 0, 4),
        ring(&[1, 2, 3, 4], 4, 4),
        ring(&[1, 2, 3], 0, 4),
        ring(&[], 1, 0),
        ring(&[], 0, 1),
    ] {
        let mut state = input(0);
        state.reorder = SpecialReorder::Active {
            current_main_ring: malformed,
            source_main_ring: ring(&[1, 2, 3, 4], 0, 4),
            active_count: 0,
        };
        assert_eq!(
            predict_private_specials(state, SpecialLookup::Known).unwrap_err(),
            SpecialError::InvalidRing {
                ring: MainRingKind::Current
            }
        );
        state.reorder = SpecialReorder::Active {
            current_main_ring: ring(&[1, 2, 3, 4], 0, 4),
            source_main_ring: malformed,
            active_count: 0,
        };
        assert_eq!(
            predict_private_specials(state, SpecialLookup::Known).unwrap_err(),
            SpecialError::InvalidRing {
                ring: MainRingKind::Source
            }
        );
    }
    let mut state = input(0);
    state.reorder = SpecialReorder::Active {
        current_main_ring: ring(&[], 0, 0),
        source_main_ring: ring(&[], 0, 0),
        active_count: 5,
    };
    assert_eq!(
        predict_private_specials(state, SpecialLookup::Known).unwrap_err(),
        SpecialError::InvalidActiveCount(5)
    );
}

#[test]
fn active_remap_requires_a_full_special_list() {
    let mut state = input(0);
    state.low_candidates = &[1, 2, 3];
    state.reorder = SpecialReorder::Active {
        current_main_ring: ring(&[30, 10, 40, 20], 0, 4),
        source_main_ring: ring(&[10, 20, 30, 40], 0, 4),
        active_count: 4,
    };
    assert_eq!(
        predict_private_specials(state, SpecialLookup::Known).unwrap_err(),
        SpecialError::InsufficientSpecials { found: 3 }
    );
}
