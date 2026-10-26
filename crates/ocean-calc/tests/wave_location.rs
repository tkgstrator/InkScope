use ocean_calc::{
    SeedWords,
    applied_preview::JobOneSchedulerState,
    applied_wave::JobOneApplyState,
    levels::QueueCounts,
    preview::QueueCountSource,
    rush_location::{RushLocationCallInput, RushSelectorInput, RushTimerGates},
    schedule::ScheduleBranch,
    stage_data::stage_spawns,
    stage_wave::{DifficultySource, StageWaveInput, WaveWeightSource, WaveWeights},
    wave_location::{
        JobOneWaveLocationInput, LocationRuntime, LocationStatus, RushRuntimeInput,
        WaveLocationError, predict_job_one_wave_location,
    },
    zako_spot::{ModeZeroZakoSpotInput, RuntimeZakoSpot, ZakoSpotOutcome},
    zako_timer::ZakoTimerStep,
};

const NO_EVENT: WaveWeights = WaveWeights {
    event: [1, 0, 0, 0, 0, 0, 0, 0, 0],
    tide: [1, 0, 0],
};
const POINT: RuntimeZakoSpot = RuntimeZakoSpot {
    origin: [1.0, 2.0, 3.0],
    forward: [0.0, 0.0, 1.0],
};
const COUNTS: QueueCounts = QueueCounts {
    wave_change_num: 4,
    rare_appear_num: 12,
};

fn weights(event: usize) -> [WaveWeights; 5] {
    let mut values = [NO_EVENT; 5];
    values[0].event = [0; 9];
    values[0].event[event] = 1;
    values
}

fn runtime_state(players: i32) -> [JobOneApplyState; 5] {
    [JobOneApplyState {
        previous_applied_tide: 0,
        active_player_count: players,
        mode_flag_591: false,
        special_flag_624: false,
        special_target_index_620: 0,
        runtime_mode_forces_mid_tide: false,
    }; 5]
}

fn input<'a>(
    weights: &'a [WaveWeights; 5],
    states: [JobOneApplyState; 5],
    cutoff: Option<u8>,
    runtime: Option<LocationRuntime<'a>>,
) -> JobOneWaveLocationInput<'a> {
    JobOneWaveLocationInput {
        stage_wave: StageWaveInput {
            seed: SeedWords::from_scene([1, 2, 3, 4]),
            stage: stage_spawns("Cop_Shakehighway").unwrap(),
            difficulty: DifficultySource::JobOne {
                match_mode: 11,
                scene_difficulty: 1665,
            },
            is_big_run: false,
            global_word: 0x10100,
            cutoff,
            special: None,
            weights: WaveWeightSource::Explicit(weights),
        },
        counts: QueueCountSource::Explicit(&[COUNTS; 5]),
        branches: [ScheduleBranch::Normal; 5],
        states,
        schedulers: [JobOneSchedulerState {
            runtime_mode_bypasses_rare_queue: false,
        }; 5],
        wave_index: 0,
        runtime,
    }
}

#[test]
fn solo_override_prevents_rush_trace_for_generated_event_one() {
    let weights = weights(1);
    let result =
        predict_job_one_wave_location(input(&weights, runtime_state(1), None, None)).unwrap();
    assert_eq!(result.generated.event_id, 1);
    assert_eq!(result.direct.event_id, 0);
    assert_eq!(result.location, LocationStatus::MissingRuntime);
}

#[test]
fn event_eight_handler_is_not_presented_as_a_known_location() {
    let weights = weights(8);
    let result =
        predict_job_one_wave_location(input(&weights, runtime_state(4), None, None)).unwrap();
    assert_eq!(result.direct.event_id, 8);
    assert_eq!(
        result.location,
        LocationStatus::RequiresHandlerObservation(8)
    );
}

#[test]
fn a_rush_snapshot_cannot_be_used_after_the_solo_event_override() {
    let weights = weights(1);
    let calls = [];
    let runtime = LocationRuntime::Rush(RushRuntimeInput {
        stage_keys: &[0, 1, 2],
        wave_change_num: 4,
        initial_timer: 0,
        calls: &calls,
    });
    assert_eq!(
        predict_job_one_wave_location(input(&weights, runtime_state(1), None, Some(runtime),)),
        Err(WaveLocationError::RuntimeForWrongEvent { applied_event: 0 })
    );
}

#[test]
fn regeneration_suffix_does_not_relabel_a_wave() {
    let weights = weights(1);
    assert_eq!(
        predict_job_one_wave_location(input(&weights, runtime_state(4), Some(0), None)),
        Err(WaveLocationError::WaveNotGenerated(0))
    );
}

#[test]
fn rush_location_uses_the_selected_applied_wave_seed_and_runtime_calls() {
    let weights = weights(1);
    let group = [Some(POINT)];
    let mut groups: [Option<&[Option<RuntimeZakoSpot>]>; 9] = [None; 9];
    groups[1] = Some(&group);
    let selector = RushSelectorInput {
        observed_rng_before_selector: None,
        eligible_visit_count: 3,
        selection_succeeded: true,
        default_callback_route_confirmed: true,
        applied_mode: 0,
        tide: 1,
        runtime_groups: groups,
        tree_30: &[],
        tree_50: &[],
        scratch_capacity: 64,
    };
    let calls = [RushLocationCallInput {
        elapsed_tick: 0,
        timer: RushTimerGates {
            pre_countdown_allows: true,
            population_gate_allows: Some(true),
            reset_interval: Some(10),
            post_reset_allows: Some(true),
        },
        selector: Some(selector),
    }];
    let runtime = LocationRuntime::Rush(RushRuntimeInput {
        stage_keys: &[0, 1, 2],
        wave_change_num: 4,
        initial_timer: 0,
        calls: &calls,
    });
    let result =
        predict_job_one_wave_location(input(&weights, runtime_state(4), None, Some(runtime)))
            .unwrap();
    let LocationStatus::Rush(trace) = result.location else {
        panic!("expected Rush location trace");
    };
    assert_eq!(trace.calls[0].direction.direction, Some(0));
    assert!(matches!(
        trace.calls[0].spot.unwrap().outcome,
        ZakoSpotOutcome::SelectedPoint { spot: POINT, .. }
    ));
    assert!(trace.calls[0].request_draw.is_some());
}

#[test]
fn raw_applied_tide_outside_game_range_rejects_a_location_trace() {
    let weights = weights(1);
    let mut states = runtime_state(4);
    states[0].mode_flag_591 = true;
    states[0].special_flag_624 = true;
    states[0].previous_applied_tide = 3;
    let calls = [];
    let runtime = LocationRuntime::Rush(RushRuntimeInput {
        stage_keys: &[0, 1, 2],
        wave_change_num: 4,
        initial_timer: 0,
        calls: &calls,
    });
    assert_eq!(
        predict_job_one_wave_location(input(&weights, states, None, Some(runtime))),
        Err(WaveLocationError::InvalidAppliedTide(3))
    );
}

#[test]
fn an_empty_rush_call_list_is_not_a_location_trace() {
    let weights = weights(1);
    let calls = [];
    let runtime = LocationRuntime::Rush(RushRuntimeInput {
        stage_keys: &[0, 1, 2],
        wave_change_num: 4,
        initial_timer: 0,
        calls: &calls,
    });
    assert_eq!(
        predict_job_one_wave_location(input(&weights, runtime_state(4), None, Some(runtime),)),
        Err(WaveLocationError::EmptyRushCalls)
    );
}

#[test]
fn event_zero_accepts_only_an_explicit_one_attempt_snapshot() {
    let weights = weights(0);
    let group = [Some(POINT)];
    let mut groups: [Option<&[Option<RuntimeZakoSpot>]>; 9] = [None; 9];
    groups[0] = Some(&group);
    let attempt = ModeZeroZakoSpotInput {
        timer_step: ZakoTimerStep {
            timer_after: 10,
            timer_reset: true,
            spawn_path_entered: true,
        },
        species_selection_succeeded: true,
        default_callback_route_confirmed: true,
        applied_event: 0,
        applied_mode: 0,
        direction: 0,
        tide: 0,
        rng_after_species: [1, 2, 3, 4],
        runtime_groups: groups,
        tree_30: &[],
        tree_50: &[],
        scratch_capacity: 64,
    };
    let result = predict_job_one_wave_location(input(
        &weights,
        runtime_state(4),
        None,
        Some(LocationRuntime::EventZero(attempt)),
    ))
    .unwrap();
    assert!(matches!(
        result.location,
        LocationStatus::EventZeroAttempt {
            direction: Some(0),
            spot: ocean_calc::zako_spot::ZakoSpotTrace {
                outcome: ZakoSpotOutcome::SelectedPoint { spot: POINT, .. },
                ..
            }
        }
    ));
}

#[test]
fn blocked_event_zero_call_does_not_display_an_unchecked_direction() {
    let weights = weights(0);
    let attempt = ModeZeroZakoSpotInput {
        timer_step: ZakoTimerStep {
            timer_after: 9,
            timer_reset: false,
            spawn_path_entered: false,
        },
        species_selection_succeeded: true,
        default_callback_route_confirmed: true,
        applied_event: 0,
        applied_mode: 0,
        direction: 99,
        tide: 0,
        rng_after_species: [1, 2, 3, 4],
        runtime_groups: [None; 9],
        tree_30: &[],
        tree_50: &[],
        scratch_capacity: 64,
    };
    let result = predict_job_one_wave_location(input(
        &weights,
        runtime_state(4),
        None,
        Some(LocationRuntime::EventZero(attempt)),
    ))
    .unwrap();
    assert!(matches!(
        result.location,
        LocationStatus::EventZeroAttempt {
            direction: None,
            spot: ocean_calc::zako_spot::ZakoSpotTrace {
                outcome: ZakoSpotOutcome::TimerBlocked,
                ..
            }
        }
    ));
}
