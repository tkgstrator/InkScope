use ocean_calc::{
    SeedWords,
    records::{
        GenerationError, InitialSpecialError, NormalWaveInput, SpecialWaveState, WaveSelection,
        initial_job_type_one_special_state, initial_job_type_zero_special_state,
        predict_selections, predict_unforced_selections,
    },
    rules::tide_candidates,
    wave::{WaveRng, draw_tide},
};

fn inputs() -> [NormalWaveInput; 5] {
    let mut event_weights = [0; 9];
    event_weights[5] = 1;
    [NormalWaveInput {
        event_weights,
        tide_weights: [0, 1, 0],
        mode_one: false,
        ban_event_mask: 0,
        global_word: 0x10100,
    }; 5]
}

fn varied_inputs() -> [NormalWaveInput; 5] {
    std::array::from_fn(|index| NormalWaveInput {
        // These positive event weights would consume RNG on the ordinary path.
        event_weights: [4, 2, 5, 1, 3, 6, 1, 2, 7],
        tide_weights: [1 + index as u32, 5 - index as u32, 2 + (index % 3) as u32],
        mode_one: index % 2 == 1,
        ban_event_mask: 0,
        global_word: 0x10100,
    })
}

fn tide_only_reference(
    seed: SeedWords,
    cutoff: Option<u8>,
    inputs: [NormalWaveInput; 5],
) -> Vec<WaveSelection> {
    let mut rng = WaveRng::from_seed(seed);
    inputs
        .into_iter()
        .enumerate()
        .filter(|(index, _)| cutoff.is_none_or(|last| *index > usize::from(last)))
        .map(|(index, input)| WaveSelection {
            wave_index: index as u8,
            event_id: 0,
            tide_id: draw_tide(
                &mut rng,
                &tide_candidates(input.tide_weights, input.mode_one, 0),
            ),
        })
        .collect()
}

#[test]
fn job_two_and_three_skip_event_draws_for_initial_and_cutoff_generation() {
    let seed = SeedWords::from_scene([1, 2, 3, 4]);
    for job_type in [2, 3] {
        for cutoff in [None, Some(2)] {
            let expected = tide_only_reference(seed, cutoff, varied_inputs());
            let outcome = predict_selections(seed, cutoff, varied_inputs(), job_type, None)
                .expect("JobType 2/3 tide-only generation should work");
            assert_eq!(
                outcome.selections, expected,
                "JobType {job_type}, {cutoff:?}"
            );
            assert_eq!(outcome.special, None);
        }
    }
}

#[test]
fn live_job_four_excludes_event_two_even_if_supplied_mode_flag_is_false() {
    let seed = SeedWords::from_scene([1, 2, 3, 4]);
    let mut wave_inputs = inputs();
    wave_inputs[0].event_weights = [0, 0, 1, 0, 0, 0, 0, 0, 0];
    wave_inputs[0].tide_weights = [1, 0, 0];
    // JobType 4 means a nonnull live runtime mode object whose mode field is 4.
    let job_four = predict_selections(seed, None, wave_inputs, 4, None).unwrap();
    let job_zero = predict_selections(seed, None, wave_inputs, 0, None).unwrap();
    assert_eq!(
        (
            job_four.selections[0].event_id,
            job_four.selections[0].tide_id
        ),
        (0, 0)
    );
    assert_eq!(
        (
            job_zero.selections[0].event_id,
            job_zero.selections[0].tide_id
        ),
        (2, 1)
    );
}

#[test]
fn live_job_four_allows_low_tide_for_event_one() {
    let seed = SeedWords::from_scene([1, 2, 3, 4]);
    let mut wave_inputs = inputs();
    wave_inputs[0].event_weights = [0, 1, 0, 0, 0, 0, 0, 0, 0];
    wave_inputs[0].tide_weights = [1, 0, 0];
    // JobType 4 means a nonnull live runtime mode object whose mode field is 4.
    let job_four = predict_selections(seed, None, wave_inputs, 4, None).unwrap();
    let job_zero = predict_selections(seed, None, wave_inputs, 0, None).unwrap();
    assert_eq!(
        (
            job_four.selections[0].event_id,
            job_four.selections[0].tide_id
        ),
        (1, 0)
    );
    assert_eq!(
        (
            job_zero.selections[0].event_id,
            job_zero.selections[0].tide_id
        ),
        (1, 1)
    );
}

#[test]
fn live_job_four_matches_forced_mode_through_multiple_waves_and_regeneration() {
    let seed = SeedWords::from_scene([0x1234, 0x5678, 0x9abc, 0xdef0]);
    let raw_inputs = varied_inputs().map(|input| NormalWaveInput {
        mode_one: false,
        ..input
    });
    let forced_inputs = raw_inputs.map(|input| NormalWaveInput {
        mode_one: true,
        ..input
    });
    // JobType 4 means a nonnull live runtime mode object whose mode field is 4.
    for cutoff in [None, Some(2)] {
        let job_four = predict_selections(seed, cutoff, raw_inputs, 4, None).unwrap();
        let forced_job_zero = predict_selections(seed, cutoff, forced_inputs, 0, None).unwrap();
        assert_eq!(
            job_four.selections, forced_job_zero.selections,
            "{cutoff:?}"
        );
        assert_eq!(
            job_four.selections.len(),
            if cutoff.is_none() { 5 } else { 2 }
        );
        assert_eq!(job_four.special, None);
    }
}

#[test]
fn inspected_initial_job_one_path_clears_special_state() {
    assert_eq!(
        initial_job_type_one_special_state(false),
        Err(InitialSpecialError::GenerationTargetAbsent)
    );
    assert_eq!(initial_job_type_one_special_state(true), Ok(None));
}

#[test]
fn initial_job_zero_special_uses_distinct_seed_fallbacks_and_threshold() {
    let ordinary = initial_job_type_zero_special_state(Some([1, 2, 3, 4]), 5);
    assert_eq!(ordinary.draw, 0xf1a9_ae74);
    assert_eq!(ordinary.threshold_bits, 0.015625_f32.to_bits());
    assert_eq!(ordinary.special, None);

    let null_seed = initial_job_type_zero_special_state(None, 20);
    let zero_seed = initial_job_type_zero_special_state(Some([0; 4]), 20);
    assert_eq!(null_seed.draw, 0x951d_958b);
    assert_eq!(zero_seed.draw, 0x57cb_51f9);
    assert_eq!(null_seed.threshold_bits, 1.0_f32.to_bits());
    assert_eq!(zero_seed.threshold_bits, 1.0_f32.to_bits());
    assert_eq!(
        null_seed.special,
        Some(SpecialWaveState {
            target_index: 3,
            stored_tide: 3
        })
    );
    assert_eq!(zero_seed.special, null_seed.special);
    assert_eq!(
        initial_job_type_zero_special_state(Some([1, 2, 3, 4]), 4).special,
        None
    );
    // The ELF uses >=, so a zero uniform draw activates even at threshold 0.
    let zero_uniform = initial_job_type_zero_special_state(Some([4_719_127, 0, 0, 0]), 4);
    assert_eq!(zero_uniform.draw, 0x1f1);
    assert_eq!(zero_uniform.uniform_bits, 0.0_f32.to_bits());
    assert_eq!(zero_uniform.threshold_bits, 0.0_f32.to_bits());
    assert_eq!(zero_uniform.special, null_seed.special);

    let generated = predict_selections(
        SeedWords::from_scene([1, 2, 3, 4]),
        None,
        inputs(),
        0,
        null_seed.special,
    )
    .unwrap();
    assert_eq!(generated.selections[3].event_id, 0);
    assert_eq!(
        generated.selections[3].tide_id,
        generated.selections[2].tide_id
    );
}

#[test]
fn supplied_job_one_special_state_remains_available_for_other_runtime_paths() {
    let generated = predict_selections(
        SeedWords::from_scene([1, 2, 3, 4]),
        None,
        inputs(),
        1,
        Some(SpecialWaveState {
            target_index: 3,
            stored_tide: 3,
        }),
    )
    .unwrap();
    assert_eq!(generated.selections[3].event_id, 0);
    assert_eq!(
        generated.selections[3].tide_id,
        generated.selections[2].tide_id
    );
}

#[test]
fn unforced_records_use_previous_generated_event() {
    let waves = predict_unforced_selections(SeedWords::from_scene([1, 2, 3, 4]), None, inputs())
        .expect("normal initial generation should work");
    assert_eq!(
        waves,
        vec![
            WaveSelection {
                wave_index: 0,
                event_id: 5,
                tide_id: 1
            },
            WaveSelection {
                wave_index: 1,
                event_id: 0,
                tide_id: 1
            },
            WaveSelection {
                wave_index: 2,
                event_id: 5,
                tide_id: 1
            },
            WaveSelection {
                wave_index: 3,
                event_id: 0,
                tide_id: 1
            },
            WaveSelection {
                wave_index: 4,
                event_id: 5,
                tide_id: 1
            },
        ]
    );
}

#[test]
fn regeneration_skips_old_waves_and_resets_previous_event() {
    let waves = predict_unforced_selections(SeedWords::from_scene([1, 2, 3, 4]), Some(2), inputs())
        .expect("normal regeneration should work");
    assert_eq!(
        waves,
        vec![
            WaveSelection {
                wave_index: 3,
                event_id: 5,
                tide_id: 1
            },
            WaveSelection {
                wave_index: 4,
                event_id: 0,
                tide_id: 1
            },
        ]
    );
    assert_eq!(
        predict_unforced_selections(SeedWords::from_scene([1, 2, 3, 4]), Some(4), inputs()),
        Ok(vec![])
    );
    assert_eq!(
        predict_unforced_selections(SeedWords::from_scene([1, 2, 3, 4]), Some(5), inputs()),
        Err(GenerationError::InvalidCutoff(5))
    );
}

#[test]
fn special_wave_inherits_prior_tide_and_resets_previous_event() {
    let result = predict_selections(
        SeedWords::from_scene([1, 2, 3, 4]),
        None,
        inputs(),
        1,
        Some(SpecialWaveState {
            target_index: 3,
            stored_tide: 3,
        }),
    )
    .expect("special wave generation should work");
    assert_eq!(result.special.unwrap().stored_tide, 1);
    assert_eq!(result.selections[2].event_id, 5);
    assert_eq!(
        result.selections[3],
        WaveSelection {
            wave_index: 3,
            event_id: 0,
            tide_id: 1
        }
    );
    assert_eq!(result.selections[4].event_id, 5);
}

#[test]
fn regenerated_target_uses_retained_tide_without_redrawing() {
    let result = predict_selections(
        SeedWords::from_scene([1, 2, 3, 4]),
        Some(2),
        inputs(),
        1,
        Some(SpecialWaveState {
            target_index: 3,
            stored_tide: 2,
        }),
    )
    .expect("regeneration should work");
    assert_eq!(result.special.unwrap().stored_tide, 2);
    assert_eq!(
        result.selections,
        vec![
            WaveSelection {
                wave_index: 3,
                event_id: 0,
                tide_id: 2
            },
            WaveSelection {
                wave_index: 4,
                event_id: 5,
                tide_id: 1
            },
        ]
    );
}

#[test]
fn unfilled_special_tide_draws_only_tide_at_target() {
    let result = predict_selections(
        SeedWords::from_scene([1, 2, 3, 4]),
        Some(2),
        inputs(),
        1,
        Some(SpecialWaveState {
            target_index: 3,
            stored_tide: 3,
        }),
    )
    .expect("regeneration should work");
    assert_eq!(result.special.unwrap().stored_tide, 3);
    assert_eq!(
        result.selections[0],
        WaveSelection {
            wave_index: 3,
            event_id: 0,
            tide_id: 1
        }
    );
    assert_eq!(result.selections[1].event_id, 5);
}

#[test]
fn special_wave_requires_supported_job_type_and_values() {
    let seed = SeedWords::from_scene([1, 2, 3, 4]);
    let special = SpecialWaveState {
        target_index: 3,
        stored_tide: 3,
    };
    for job_type in [5, 6, 7] {
        assert_eq!(
            predict_selections(seed, None, inputs(), job_type, None),
            Err(GenerationError::UnsupportedJobType(job_type))
        );
    }
    assert!(predict_selections(seed, None, inputs(), 0, Some(special)).is_ok());
    for job_type in [2, 3, 4] {
        assert_eq!(
            predict_selections(seed, None, inputs(), job_type, Some(special)),
            Err(GenerationError::SpecialRequiresJobTypeZeroOrOne)
        );
    }
    assert_eq!(
        predict_selections(
            seed,
            None,
            inputs(),
            1,
            Some(SpecialWaveState {
                target_index: 5,
                stored_tide: 3
            })
        ),
        Err(GenerationError::InvalidSpecialIndex(5))
    );
    assert_eq!(
        predict_selections(
            seed,
            None,
            inputs(),
            1,
            Some(SpecialWaveState {
                target_index: 3,
                stored_tide: 4
            })
        ),
        Err(GenerationError::InvalidStoredTide(4))
    );
}

#[test]
fn special_branch_draw_count_changes_the_following_wave() {
    let varied = [NormalWaveInput {
        event_weights: [1; 9],
        tide_weights: [1; 3],
        mode_one: false,
        ban_event_mask: 0,
        global_word: 0x10100,
    }; 5];
    let seed = SeedWords::from_scene([1, 2, 3, 4]);
    let stored = predict_selections(
        seed,
        Some(2),
        varied,
        1,
        Some(SpecialWaveState {
            target_index: 3,
            stored_tide: 2,
        }),
    )
    .unwrap();
    let sentinel = predict_selections(
        seed,
        Some(2),
        varied,
        1,
        Some(SpecialWaveState {
            target_index: 3,
            stored_tide: 3,
        }),
    )
    .unwrap();
    assert_eq!(stored.selections[0].tide_id, 2);
    assert_eq!(stored.selections[1].event_id, 6);
    assert_eq!(stored.selections[1].tide_id, 0);
    assert_eq!(sentinel.selections[0].tide_id, 1);
    assert_eq!(sentinel.selections[1].event_id, 2);
    assert_eq!(sentinel.selections[1].tide_id, 1);
}
