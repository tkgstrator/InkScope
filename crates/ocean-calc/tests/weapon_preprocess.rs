use ocean_calc::{
    SeedWords,
    weapon_preprocess::{
        PreprocessCandidateStreams, PreprocessError, PreprocessShape, preprocess_weapon_candidates,
        weapon_rng_after_preprocess,
    },
};

const SEED: [u32; 4] = [1, 2, 3, 4];
const BASE: [u32; 4] = [0x0331_7883, 0x4b97_a6b1, 0xc9d8_f373, 0xa4ee_fb34];

fn assert_advance(shape: PreprocessShape, draws: u8, state: [u32; 4]) {
    let result = weapon_rng_after_preprocess(SeedWords::from_scene(SEED), shape).unwrap();
    assert_eq!(result.draws, draws);
    assert_eq!(result.state.as_array(), state);
}

#[test]
fn low_branch_uses_one_draw_per_shuffle_step() {
    assert_advance(PreprocessShape::Low { n: 0 }, 0, BASE);
    assert_advance(
        PreprocessShape::Low { n: 4 },
        3,
        [0xa4ee_fb34, 0x2c93_7a4a, 0xdac7_f347, 0xd48a_c007],
    );
    assert_advance(
        PreprocessShape::Low { n: 8 },
        7,
        [0x076e_b6f9, 0xb098_dd74, 0x5520_5deb, 0xd728_25b0],
    );
}

#[test]
fn high_branch_counts_both_partial_lists_and_final_shuffle() {
    assert_advance(
        PreprocessShape::High {
            a: 0,
            second_added: 0,
        },
        0,
        BASE,
    );
    // First list empty: B = 4, M = 2, hence 3 + 1 draws.
    assert_advance(
        PreprocessShape::High {
            a: 0,
            second_added: 4,
        },
        4,
        [0x2c93_7a4a, 0xdac7_f347, 0xd48a_c007, 0x076e_b6f9],
    );
    // Second list empty: B = 0, M = 1.
    assert_advance(
        PreprocessShape::High {
            a: 1,
            second_added: 0,
        },
        0,
        BASE,
    );
    // B = 3, M = 4, giving (3 - 1) + (3 - 1) + (4 - 1) = 7.
    assert_advance(
        PreprocessShape::High {
            a: 3,
            second_added: 2,
        },
        7,
        [0x076e_b6f9, 0xb098_dd74, 0x5520_5deb, 0xd728_25b0],
    );
    // B = 7, M = 4.
    assert_advance(
        PreprocessShape::High {
            a: 9,
            second_added: 0,
        },
        17,
        [0x3afc_8db8, 0x471f_ee7e, 0x6127_1598, 0x32ce_ecc4],
    );
    // Both source lists reach capacity: B = 9, M = 4; 19 is the maximum.
    assert_advance(
        PreprocessShape::High {
            a: 9,
            second_added: 2,
        },
        19,
        [0x6127_1598, 0x32ce_ecc4, 0xec81_3668, 0x5455_5998],
    );
    // The work list has capacity nine even if every entry comes from the
    // second source; the final selected array still has at most two entries.
    assert_advance(
        PreprocessShape::High {
            a: 0,
            second_added: 9,
        },
        9,
        [0x5520_5deb, 0xd728_25b0, 0xa583_98d2, 0xd386_826b],
    );
}

#[test]
fn rejects_counts_outside_runtime_list_capacities() {
    let seed = SeedWords::from_scene(SEED);
    assert_eq!(
        weapon_rng_after_preprocess(seed, PreprocessShape::Low { n: 9 }).unwrap_err(),
        PreprocessError::LowCountTooLarge(9)
    );
    assert_eq!(
        weapon_rng_after_preprocess(
            seed,
            PreprocessShape::High {
                a: 10,
                second_added: 0
            }
        )
        .unwrap_err(),
        PreprocessError::FirstCountTooLarge(10)
    );
    assert_eq!(
        weapon_rng_after_preprocess(
            seed,
            PreprocessShape::High {
                a: 0,
                second_added: 10
            }
        )
        .unwrap_err(),
        PreprocessError::SecondAddedTooLarge(10)
    );
    assert_eq!(
        weapon_rng_after_preprocess(
            seed,
            PreprocessShape::High {
                a: 8,
                second_added: 4
            }
        )
        .unwrap_err(),
        PreprocessError::SecondCountTooLarge(10)
    );
}

#[test]
fn low_candidate_stream_is_shuffled_after_taking_eight() {
    let seed = SeedWords::from_scene(SEED);
    let result =
        preprocess_weapon_candidates(seed, PreprocessCandidateStreams::Low(&[11, 22, 33, 44]));
    assert_eq!(result.candidates, [44, 22, 33, 11]);
    assert_eq!(result.rng.draws, 3);
    assert_eq!(
        result.rng.state.as_array(),
        [0xa4ee_fb34, 0x2c93_7a4a, 0xdac7_f347, 0xd48a_c007]
    );

    let long = preprocess_weapon_candidates(
        seed,
        PreprocessCandidateStreams::Low(&[11, 12, 13, 14, 15, 16, 17, 18, 99]),
    );
    assert_eq!(long.candidates.len(), 8);
    assert!(!long.candidates.contains(&99));
    assert_eq!(long.rng.draws, 7);
    assert_eq!(
        long.rng,
        weapon_rng_after_preprocess(seed, PreprocessShape::Low { n: 8 }).unwrap()
    );
}

#[test]
fn high_candidate_streams_use_forward_order_head_selection_and_three_shuffles() {
    let seed = SeedWords::from_scene(SEED);
    let result = preprocess_weapon_candidates(
        seed,
        PreprocessCandidateStreams::High {
            first: &[11, 22, 33],
            second: &[44, 55],
        },
    );
    assert_eq!(result.candidates, [11, 22, 33, 44]);
    assert_eq!(result.rng.draws, 7);
    assert_eq!(
        result.rng.state.as_array(),
        [0x076e_b6f9, 0xb098_dd74, 0x5520_5deb, 0xd728_25b0]
    );
    assert_eq!(
        result.rng,
        weapon_rng_after_preprocess(
            seed,
            PreprocessShape::High {
                a: 3,
                second_added: 2
            }
        )
        .unwrap()
    );
}

#[test]
fn high_capacity_keeps_only_first_two_second_entries_after_nine_first_entries() {
    let seed = SeedWords::from_scene(SEED);
    let result = preprocess_weapon_candidates(
        seed,
        PreprocessCandidateStreams::High {
            first: &[11, 12, 13, 14, 15, 16, 17, 18, 19, 99],
            second: &[21, 22, 23],
        },
    );
    assert_eq!(result.candidates, [15, 14, 17, 18]);
    assert_eq!(result.rng.draws, 19);
    assert_eq!(
        result.rng.state.as_array(),
        [0x6127_1598, 0x32ce_ecc4, 0xec81_3668, 0x5455_5998]
    );
    assert_eq!(
        result.rng,
        weapon_rng_after_preprocess(
            seed,
            PreprocessShape::High {
                a: 9,
                second_added: 2
            }
        )
        .unwrap()
    );
}

#[test]
fn high_empty_first_stream_can_fill_nine_work_list_slots() {
    let seed = SeedWords::from_scene(SEED);
    let result = preprocess_weapon_candidates(
        seed,
        PreprocessCandidateStreams::High {
            first: &[],
            second: &[21, 22, 23, 24, 25, 26, 27, 28, 29, 30],
        },
    );
    assert_eq!(result.candidates, [28, 25]);
    assert_eq!(result.rng.draws, 9);
    assert_eq!(
        result.rng.state.as_array(),
        [0x5520_5deb, 0xd728_25b0, 0xa583_98d2, 0xd386_826b]
    );
}

#[test]
fn empty_streams_and_duplicate_identities_are_preserved() {
    let seed = SeedWords::from_scene(SEED);
    for streams in [
        PreprocessCandidateStreams::Low(&[]),
        PreprocessCandidateStreams::High {
            first: &[],
            second: &[],
        },
    ] {
        let result = preprocess_weapon_candidates(seed, streams);
        assert!(result.candidates.is_empty());
        assert_eq!(result.rng.draws, 0);
        assert_eq!(result.rng.state.as_array(), BASE);
    }
    let duplicate = preprocess_weapon_candidates(seed, PreprocessCandidateStreams::Low(&[7, 7, 8]));
    let mut sorted = duplicate.candidates;
    sorted.sort_unstable();
    assert_eq!(sorted, [7, 7, 8]);
}
