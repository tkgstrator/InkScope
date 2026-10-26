use salmon_wave_lookahead::{
    SeedWords,
    applied_preview::JobOneSchedulerState,
    applied_wave::JobOneApplyState,
    first_rare::{
        FirstRareError, FirstRareInput, FirstRareSpotDraw, FirstSpotRoute,
        predict_first_ordinary_rare_with_spot,
    },
    levels::QueueCounts,
    preview::{CountOrigin, QueueCountSource},
    schedule::{
        EntryKind, NominalEntry, ScheduleBranch, ScheduleError, ScheduleInput, nominal_schedule,
    },
    selected_wave_rare::{
        AppliedValueEvidence, ConditionBasis, FirstRareRuntimeEvidence, FirstRareStatus,
        SelectedWaveFirstRareError, SelectedWaveFirstRareInput, SpotStatus,
        predict_selected_job_one_first_rare,
    },
    species::{PeriodicGoldieInput, PeriodicGoldieSource, RareCandidateState},
    stage_data::stage_spawns,
    stage_wave::{DifficultySource, StageWaveInput, WaveWeightSource, WaveWeights},
};

const COUNTS: QueueCounts = QueueCounts {
    wave_change_num: 10,
    rare_appear_num: 12,
};

fn weights(event: usize, tide: usize) -> [WaveWeights; 5] {
    let mut result = [WaveWeights {
        event: [0; 9],
        tide: [0; 3],
    }; 5];
    for wave in &mut result {
        wave.event[event] = 1;
        wave.tide[tide] = 1;
    }
    result
}

fn state(players: i32) -> JobOneApplyState {
    JobOneApplyState {
        previous_applied_tide: 0,
        active_player_count: players,
        mode_flag_591: false,
        special_flag_624: false,
        special_target_index_620: 0,
        runtime_mode_forces_mid_tide: false,
    }
}

fn evidence() -> FirstRareRuntimeEvidence {
    FirstRareRuntimeEvidence {
        scene_seed: ConditionBasis::AssumedIdealized,
        applied_values: AppliedValueEvidence::AssumedNoLaterOverride,
        zero_rare_population: ConditionBasis::AssumedIdealized,
        no_prequeue_effects: ConditionBasis::AssumedIdealized,
        normal_stage_registration: ConditionBasis::AssumedIdealized,
        unfiltered_actor_group: ConditionBasis::AssumedIdealized,
    }
}

fn input<'a>(
    weights: &'a [WaveWeights; 5],
    players: i32,
    wave_index: u8,
) -> SelectedWaveFirstRareInput<'a> {
    SelectedWaveFirstRareInput {
        stage_wave: StageWaveInput {
            seed: SeedWords::from_scene([1, 2, 3, 4]),
            stage: stage_spawns("Cop_Shakehighway").unwrap(),
            difficulty: DifficultySource::JobOne {
                match_mode: 11,
                scene_difficulty: 1665,
            },
            is_big_run: false,
            global_word: 0x10100,
            cutoff: None,
            special: None,
            weights: WaveWeightSource::Explicit(weights),
        },
        counts: QueueCountSource::Explicit(&[COUNTS; 5]),
        branches: [ScheduleBranch::Normal; 5],
        states: [state(players); 5],
        schedulers: [JobOneSchedulerState {
            runtime_mode_bypasses_rare_queue: false,
        }; 5],
        wave_index,
        first_scheduler_elapsed_tick: 0,
        periodic: PeriodicGoldieInput {
            event: Some(0),
            global_cycle_flag: false,
            accepted_rare_counter: 0,
            fog_cycle: None,
            event_none_cycle: None,
        },
        runtime_evidence: evidence(),
    }
}

#[test]
fn solo_uses_applied_event_zero_for_the_first_rare_draw_and_spot() {
    let source = weights(1, 0);
    let result = predict_selected_job_one_first_rare(input(&source, 1, 0), |_| {
        panic!("ID6 uses the known generic route")
    })
    .unwrap();
    assert_eq!(result.generated.event_id, 1);
    assert_eq!(result.direct.event_id, 0);
    assert_eq!(result.direct.tide_id, 1);
    assert_eq!(result.count_origin, CountOrigin::Explicit);
    assert_eq!(
        result.runtime_evidence.scene_seed,
        ConditionBasis::AssumedIdealized
    );
    assert_eq!(
        result.runtime_evidence.unfiltered_actor_group,
        ConditionBasis::AssumedIdealized
    );
    assert_eq!(result.counts, Some(COUNTS));
    assert_eq!(result.assumed_branch, Some(ScheduleBranch::Normal));
    match result.status {
        FirstRareStatus::Normal { rare, spot } => {
            assert_eq!(rare.nominal_first_entry.tick, 0);
            assert_eq!(rare.first_direction, 0);
            assert_eq!(rare.species_id, 6);
            assert!(matches!(spot, SpotStatus::Predicted { .. }));
        },
        other => panic!("unexpected first Rare status: {other:?}"),
    }
}

#[test]
fn generated_event_eight_is_not_a_handler_when_solo_application_changes_it_to_zero() {
    let source = weights(8, 0);
    let result = predict_selected_job_one_first_rare(input(&source, 1, 0), |_| {
        FirstSpotRoute::GenericStageGroup
    })
    .unwrap();
    assert_eq!(result.generated.event_id, 8);
    assert_eq!(result.direct.event_id, 0);
    assert!(!result.direct.requires_handler_observation);
    assert!(matches!(result.status, FirstRareStatus::Normal { .. }));
}

#[test]
fn event_six_uses_directly_applied_tide_zero_for_first_rare_spot() {
    let source = weights(6, 2);
    let mut selected = input(&source, 4, 0);
    selected.periodic.event = Some(6);
    let result =
        predict_selected_job_one_first_rare(selected, |_| FirstSpotRoute::GenericStageGroup)
            .unwrap();
    assert_eq!(result.generated.event_id, 6);
    assert_eq!(result.generated.tide_id, 2);
    assert_eq!(result.direct.event_id, 6);
    assert_eq!(result.direct.tide_id, 0);
    match result.status {
        FirstRareStatus::Normal { rare, spot } => {
            assert_eq!(rare.first_direction, 0);
            assert_eq!(rare.species_id, 6);
            let eligible: Vec<_> = rare
                .candidate_states
                .iter()
                .enumerate()
                .filter_map(|(id, state)| (*state == RareCandidateState::Eligible).then_some(id))
                .collect();
            assert_eq!(eligible, [4, 5, 6, 7, 8, 9, 10, 11, 12, 14]);
            assert_eq!(
                rare.shared_state_after_species.as_array(),
                [1873002430, 2467946925, 3195445786, 3438216410]
            );
            assert_eq!(
                spot,
                SpotStatus::Predicted {
                    draw: FirstRareSpotDraw::Generic {
                        candidate_count: 6,
                        index: Some(4),
                    },
                    shared_state_after_spot: salmon_wave_lookahead::rng::XorShift128::from_words([
                        2467946925, 3195445786, 3438216410, 3200453814,
                    ]),
                }
            );
        },
        other => panic!("unexpected first Rare status: {other:?}"),
    }
    let tide_two = predict_first_ordinary_rare_with_spot(
        FirstRareInput {
            schedule: ScheduleInput {
                seed: selected.stage_wave.seed,
                event_type: 6,
                wave_change_num: COUNTS.wave_change_num,
                rare_appear_num: COUNTS.rare_appear_num,
                assumed_branch: ScheduleBranch::Normal,
            },
            stage: selected.stage_wave.stage,
            tide: 2,
            global_word: selected.stage_wave.global_word,
            first_scheduler_elapsed_tick: 0,
            periodic: selected.periodic,
        },
        |_| FirstSpotRoute::GenericStageGroup,
    )
    .unwrap();
    assert_eq!(
        tide_two.spot,
        FirstRareSpotDraw::Generic {
            candidate_count: 2,
            index: Some(1),
        }
    );
}

#[test]
fn regenerated_suffix_keeps_the_original_record_index() {
    let source = weights(0, 0);
    let mut selected = input(&source, 4, 2);
    selected.stage_wave.cutoff = Some(1);
    let mut counts = [COUNTS; 5];
    counts[2].rare_appear_num = 13;
    selected.counts = QueueCountSource::Explicit(&counts);
    selected.branches[2] = ScheduleBranch::Special;
    let result =
        predict_selected_job_one_first_rare(selected, |_| FirstSpotRoute::GenericStageGroup)
            .unwrap();
    assert_eq!(result.generated.wave_index, 2);
    assert_eq!(result.direct.wave_index, 2);
    assert_eq!(result.counts, Some(counts[2]));
    assert_eq!(result.assumed_branch, Some(ScheduleBranch::Special));
    let entries = nominal_schedule(ScheduleInput {
        seed: selected.stage_wave.seed,
        event_type: 0,
        wave_change_num: counts[2].wave_change_num,
        rare_appear_num: counts[2].rare_appear_num,
        assumed_branch: ScheduleBranch::Special,
    })
    .unwrap();
    assert_eq!(
        entries[..2],
        [
            NominalEntry {
                tick: 0,
                kind: EntryKind::Base,
            },
            NominalEntry {
                tick: 0,
                kind: EntryKind::Extra,
            },
        ]
    );
    match result.status {
        FirstRareStatus::Normal { rare, .. } => assert_eq!(rare.nominal_first_entry, entries[0]),
        other => panic!("unexpected first Rare status: {other:?}"),
    }
    selected.wave_index = 1;
    assert_eq!(
        predict_selected_job_one_first_rare(selected, |_| panic!("excluded wave")),
        Err(SelectedWaveFirstRareError::WaveNotGenerated(1))
    );
}

#[test]
fn periodic_fog_forces_id_fifteen_without_normal_spot_callback() {
    let source = weights(5, 0);
    let mut selected = input(&source, 4, 0);
    selected.periodic.event = Some(5);
    selected.periodic.fog_cycle = Some(1);
    let result = predict_selected_job_one_first_rare(selected, |_| {
        panic!("forced ID15 never calls normal spot callback")
    })
    .unwrap();
    assert!(matches!(
        result.status,
        FirstRareStatus::ForcedGoldie {
            source: PeriodicGoldieSource::FogCycle,
            ..
        }
    ));
}

#[test]
fn null_runtime_event_pointer_skips_the_periodic_force() {
    let source = weights(5, 0);
    let mut selected = input(&source, 4, 0);
    selected.periodic.event = None;
    let result =
        predict_selected_job_one_first_rare(selected, |_| FirstSpotRoute::GenericStageGroup)
            .unwrap();
    assert!(matches!(result.status, FirstRareStatus::Normal { .. }));
}

#[test]
fn mismatched_runtime_event_pointer_is_rejected() {
    let source = weights(0, 0);
    let mut selected = input(&source, 4, 0);
    selected.periodic.event = Some(1);
    assert_eq!(
        predict_selected_job_one_first_rare(selected, |_| panic!("mismatch")),
        Err(SelectedWaveFirstRareError::FirstRare(
            FirstRareError::UnexpectedRuntimeEvent(1)
        ))
    );
}

#[test]
fn bypass_handler_unsupported_event_and_invalid_queue_do_not_draw_species() {
    let source = weights(0, 0);
    let mut selected = input(&source, 4, 0);
    selected.schedulers[0].runtime_mode_bypasses_rare_queue = true;
    let bypass = predict_selected_job_one_first_rare(selected, |_| panic!("bypassed")).unwrap();
    assert_eq!(bypass.status, FirstRareStatus::QueueBypassedByRuntimeMode);

    let source = weights(8, 0);
    let handler =
        predict_selected_job_one_first_rare(input(&source, 4, 0), |_| panic!("handler unresolved"))
            .unwrap();
    assert_eq!(
        handler.status,
        FirstRareStatus::RequiresHandlerObservation(8)
    );

    let source = weights(7, 0);
    let unsupported =
        predict_selected_job_one_first_rare(input(&source, 4, 0), |_| panic!("unsupported event"))
            .unwrap();
    assert_eq!(
        unsupported.status,
        FirstRareStatus::UnsupportedAppliedEvent(7)
    );

    let source = weights(0, 0);
    let bad_counts = [QueueCounts {
        wave_change_num: 1,
        rare_appear_num: 12,
    }; 5];
    let mut selected = input(&source, 4, 0);
    selected.counts = QueueCountSource::Explicit(&bad_counts);
    let rejected = predict_selected_job_one_first_rare(selected, |_| panic!("bad queue")).unwrap();
    assert_eq!(
        rejected.status,
        FirstRareStatus::RejectedSchedule(ScheduleError::InvalidCounts)
    );
    assert_eq!(rejected.counts, Some(bad_counts[0]));
    assert_eq!(rejected.assumed_branch, Some(ScheduleBranch::Normal));
}

#[test]
fn raw_applied_tide_must_not_truncate_before_validation() {
    let source = weights(0, 0);
    for raw_tide in [3, 258] {
        let mut selected = input(&source, 4, 0);
        selected.states[0].mode_flag_591 = true;
        selected.states[0].special_flag_624 = true;
        selected.states[0].previous_applied_tide = raw_tide;
        assert_eq!(
            predict_selected_job_one_first_rare(selected, |_| panic!("invalid tide")),
            Err(SelectedWaveFirstRareError::InvalidAppliedTide(raw_tide))
        );
    }
}

#[test]
fn observed_applied_values_must_match_direct_writes() {
    let source = weights(0, 0);
    let mut selected = input(&source, 4, 0);
    selected.runtime_evidence.applied_values = AppliedValueEvidence::Observed {
        event_id: 0,
        tide_id: 2,
    };
    assert_eq!(
        predict_selected_job_one_first_rare(selected, |_| panic!("mismatch")),
        Err(SelectedWaveFirstRareError::ObservedAppliedMismatch {
            direct_event: 0,
            direct_tide: 0,
            observed_event: 0,
            observed_tide: 2,
        })
    );
}

#[test]
fn matching_observed_applied_values_are_retained_as_observed() {
    let source = weights(0, 0);
    let mut selected = input(&source, 4, 0);
    selected.runtime_evidence.applied_values = AppliedValueEvidence::Observed {
        event_id: 0,
        tide_id: 0,
    };
    let result =
        predict_selected_job_one_first_rare(selected, |_| FirstSpotRoute::GenericStageGroup)
            .unwrap();
    assert_eq!(result.runtime_evidence, selected.runtime_evidence);
    assert!(matches!(result.status, FirstRareStatus::Normal { .. }));
}

#[test]
fn unknown_spot_callback_retains_species_but_no_post_spot_rng() {
    let called = std::cell::Cell::new(0);
    let source = weights(0, 0);
    let mut selected = input(&source, 4, 0);
    selected.stage_wave.seed = SeedWords::from_scene([6, 2, 3, 4]);
    let result = predict_selected_job_one_first_rare(selected, |species| {
        called.set(called.get() + 1);
        assert_eq!(species, 8);
        FirstSpotRoute::Unknown
    })
    .unwrap();
    assert_eq!(called.get(), 1);
    match result.status {
        FirstRareStatus::Normal { rare, spot } => {
            assert_eq!(rare.species_id, 8);
            assert_eq!(spot, SpotStatus::NeedsCallback);
            assert_ne!(rare.shared_state_after_species.as_array(), [0; 4]);
        },
        other => panic!("unexpected status {other:?}"),
    }
}
