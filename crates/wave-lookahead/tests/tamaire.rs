use salmon_wave_lookahead::tamaire::{
    TamaireAttempt, TamaireError, TamaireGroup, TamaireNode, TamaireSkipReason, TamaireSnapshot,
    TamaireTypeChoice, TamaireTypeInput, TamaireTypeWeights, choose_candidate, choose_numeric_type,
    initial_candidate_rng_state, lookup_source_after_gates,
};

const POST_LOCATION_WORDS: [u32; 4] = [0x2b08_c134, 0xe245_0d87, 0x4815_f918, 0x101c_bb4b];
const POST_TYPE_DRAW_WORDS: [u32; 4] = [0xe245_0d87, 0x4815_f918, 0x101c_bb4b, 0x7d70_d91d];

fn type_input(large: i32, small: i32, standard: i32) -> TamaireTypeInput {
    TamaireTypeInput {
        weights: Some(TamaireTypeWeights {
            large,
            small,
            standard,
        }),
        forced_21: false,
        emitted_21: false,
        rng_words: POST_LOCATION_WORDS,
    }
}

#[test]
fn tamaire_type_null_config_keeps_default_twenty_without_a_draw() {
    let mut input = type_input(9, 3, 10);
    input.weights = None;
    input.forced_21 = true;
    assert_eq!(
        choose_numeric_type(input),
        TamaireTypeChoice {
            numeric_type: 20,
            emitted_21: false,
            random_word: None,
            roll: None,
            rng_words: POST_LOCATION_WORDS,
        }
    );
}

#[test]
fn tamaire_type_forced_once_then_regular_path_uses_same_local_stream() {
    let mut input = type_input(2, 3, 10);
    input.forced_21 = true;
    assert_eq!(
        choose_numeric_type(input),
        TamaireTypeChoice {
            numeric_type: 21,
            emitted_21: true,
            random_word: None,
            roll: None,
            rng_words: POST_LOCATION_WORDS,
        }
    );
    input.emitted_21 = true;
    assert_eq!(
        choose_numeric_type(input),
        TamaireTypeChoice {
            numeric_type: 20,
            emitted_21: true,
            random_word: Some(0x7d70_d91d),
            roll: Some(6),
            rng_words: POST_TYPE_DRAW_WORDS,
        }
    );
}

#[test]
fn tamaire_type_draw_uses_strict_large_greater_than_roll() {
    let expected = TamaireTypeChoice {
        numeric_type: 20,
        emitted_21: false,
        random_word: Some(0x7d70_d91d),
        roll: Some(6),
        rng_words: POST_TYPE_DRAW_WORDS,
    };
    assert_eq!(choose_numeric_type(type_input(2, 3, 10)), expected);
    assert_eq!(choose_numeric_type(type_input(6, 3, 10)), expected);
    assert_eq!(
        choose_numeric_type(type_input(7, 3, 10)),
        TamaireTypeChoice {
            numeric_type: 21,
            emitted_21: true,
            ..expected
        }
    );
}

#[test]
fn tamaire_type_negative_wrapped_sum_skips_draw_but_zero_sum_draws() {
    assert_eq!(
        choose_numeric_type(type_input(1, i32::MAX, 1)),
        TamaireTypeChoice {
            numeric_type: 21,
            emitted_21: true,
            random_word: None,
            roll: None,
            rng_words: POST_LOCATION_WORDS,
        }
    );
    assert_eq!(
        choose_numeric_type(type_input(0, i32::MAX, 1)),
        TamaireTypeChoice {
            numeric_type: 20,
            emitted_21: false,
            random_word: None,
            roll: None,
            rng_words: POST_LOCATION_WORDS,
        }
    );
    assert_eq!(
        choose_numeric_type(type_input(1, 0, 0)),
        TamaireTypeChoice {
            numeric_type: 21,
            emitted_21: true,
            random_word: Some(0x7d70_d91d),
            roll: Some(0),
            rng_words: POST_TYPE_DRAW_WORDS,
        }
    );
}

fn group(count: i32) -> TamaireGroup {
    TamaireGroup {
        count,
        nodes: (0..count.max(0) as u32)
            .map(|id| TamaireNode([id; 14]))
            .collect(),
    }
}

fn groups() -> [TamaireGroup; 3] {
    [group(5), group(2), group(1)]
}

fn snapshot(groups: [TamaireGroup; 3]) -> TamaireSnapshot {
    TamaireSnapshot {
        tide: Some(0),
        groups,
        work_capacity: 32,
        actor_manager_present: true,
        active_callback_indices: vec![],
        cooldown_marks: vec![i32::MIN; 5],
        now: 10,
        rng_words: [1, 2, 3, 0xf000_0000],
    }
}

#[test]
fn reset_rng_distinguishes_missing_zero_and_nonzero_shared_words() {
    assert_eq!(
        initial_candidate_rng_state(None),
        [0x9225_1cb7, 0xa9f4_306b, 0x5a1e_4a70, 0x78e0_d799]
    );
    assert_eq!(
        initial_candidate_rng_state(Some([0; 4])),
        [0xf14a_91f0, 0x9c2b_9fe1, 0x18fb_8f92, 0x0012_c69e]
    );
    assert_eq!(
        initial_candidate_rng_state(Some([1, 2, 3, 4])),
        [0x4243_424b, 0x2b08_c134, 0xe245_0d87, 0x4815_f918]
    );
}

#[test]
fn active_callback_and_cooldown_swap_removal_preserve_runtime_order() {
    let mut input = snapshot(groups());
    input.active_callback_indices = vec![1];
    input.cooldown_marks[2] = 20;
    assert_eq!(
        choose_candidate(&input),
        Ok(TamaireAttempt::Selected {
            group_index: 0,
            source_index: 3,
            remaining_before_draw: 3,
            rng_after_draw: [2, 3, 0xf000_0000, 0xf000_1609],
        })
    );
}

#[test]
fn missing_manager_and_empty_list_do_not_draw() {
    let mut input = snapshot(groups());
    input.actor_manager_present = false;
    input.cooldown_marks.clear();
    assert_eq!(
        choose_candidate(&input),
        Ok(TamaireAttempt::Skipped {
            group_index: 0,
            reason: TamaireSkipReason::MissingActorManager,
            rng_unchanged: input.rng_words,
        })
    );

    input.actor_manager_present = true;
    input.groups[0] = group(0);
    assert_eq!(
        choose_candidate(&input),
        Ok(TamaireAttempt::Skipped {
            group_index: 0,
            reason: TamaireSkipReason::EmptyList,
            rng_unchanged: input.rng_words,
        })
    );
}

#[test]
fn tide_fallback_and_unselected_bad_group_are_lazy() {
    let mut input = snapshot(groups());
    input.tide = Some(u32::MAX);
    input.groups[1].count = 33;
    let result = choose_candidate(&input).unwrap();
    assert!(matches!(
        result,
        TamaireAttempt::Selected { group_index: 0, .. }
    ));
    input.tide = None;
    assert!(matches!(
        choose_candidate(&input),
        Ok(TamaireAttempt::Selected { group_index: 0, .. })
    ));
    input.tide = Some(1);
    assert!(matches!(
        choose_candidate(&input),
        Err(TamaireError::InvalidSelectedGroup(_))
    ));
}

#[test]
fn cooldown_uses_nonnegative_now_and_min_sentinel() {
    let mut input = snapshot(groups());
    input.groups[0] = group(2);
    input.now = -100;
    input.cooldown_marks = vec![i32::MIN, 1];
    assert!(matches!(
        choose_candidate(&input),
        Ok(TamaireAttempt::Selected {
            source_index: 0,
            remaining_before_draw: 1,
            ..
        })
    ));
}

#[test]
fn exhausted_candidates_skip_draw_and_reached_metadata_is_validated() {
    let mut input = snapshot(groups());
    input.groups[0] = group(1);
    input.active_callback_indices = vec![0];
    assert_eq!(
        choose_candidate(&input),
        Ok(TamaireAttempt::Skipped {
            group_index: 0,
            reason: TamaireSkipReason::AllFiltered,
            rng_unchanged: input.rng_words,
        })
    );
    input.groups[0].count = 2;
    input.groups[0].nodes.clear();
    input.cooldown_marks = vec![i32::MIN; 2];
    assert!(matches!(
        choose_candidate(&input),
        Ok(TamaireAttempt::Selected {
            source_index: 1,
            ..
        })
    ));
    assert!(matches!(
        lookup_source_after_gates(&input.groups, 0, Some(0), true, true),
        Err(TamaireError::InvalidSelectedGroup(_))
    ));
    input.groups[0].count = 33;
    assert!(matches!(
        choose_candidate(&input),
        Err(TamaireError::InvalidSelectedGroup(_))
    ));
    input.groups[0] = group(1);
    input.cooldown_marks = vec![i32::MIN];
    input.work_capacity = -1;
    assert!(matches!(
        choose_candidate(&input),
        Err(TamaireError::InvalidWorkCapacity(-1))
    ));
    input.work_capacity = 0;
    assert!(matches!(
        choose_candidate(&input),
        Ok(TamaireAttempt::Skipped {
            reason: TamaireSkipReason::AllFiltered,
            rng_unchanged: [1, 2, 3, 0xf000_0000],
            ..
        })
    ));
    input.work_capacity = 32;
    input.active_callback_indices.clear();
    input.cooldown_marks.clear();
    assert!(matches!(
        choose_candidate(&input),
        Err(TamaireError::MissingCooldownMarks)
    ));
}

#[test]
fn source_lookup_requires_both_later_gates_and_rereads_tide() {
    let mut sources = groups();
    sources[1].nodes[1] = TamaireNode([101; 14]);
    assert_eq!(
        lookup_source_after_gates(&sources, 1, Some(1), false, true),
        Ok(None)
    );
    assert_eq!(
        lookup_source_after_gates(&sources, 1, Some(1), true, false),
        Ok(None)
    );
    assert_eq!(
        lookup_source_after_gates(&sources, 1, Some(1), true, true),
        Ok(Some(TamaireNode([101; 14])))
    );
}

#[test]
fn source_lookup_validates_only_the_later_selected_group() {
    let mut sources = groups();
    sources[2].count = 33;
    assert_eq!(
        lookup_source_after_gates(&sources, 1, None, true, true),
        Ok(Some(TamaireNode([1; 14])))
    );
    assert!(matches!(
        lookup_source_after_gates(&sources, 1, Some(2), true, true),
        Err(TamaireError::InvalidSelectedGroup(_))
    ));
    assert!(matches!(
        lookup_source_after_gates(&sources, 7, Some(0), true, true),
        Err(TamaireError::InvalidSelectedGroup(_))
    ));
}
