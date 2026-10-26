use salmon_wave_lookahead::{
    SeedWords,
    event_zero_counter::PostSpotOutcome,
    event_zero_location::{
        DirectorRngBoundary, DirectorRngOrigin, EventZeroFinishInput, EventZeroLocationError,
        EventZeroLocationRunner, EventZeroLocationStart, PopulationBasis, PopulationSnapshot,
        RareRequestPathBasis,
    },
    event_zero_runner::{
        CreationReceipt, EventZeroPopulation, EventZeroTraceItem, LateAttemptState,
    },
    levels::OrdinaryItemLimitInput,
    ordinary_stream::{PlannedDue, SpotRoute},
    rng::XorShift128,
    rush_location::{RushSelectorInput, RushTimerGates},
    schedule::{ScheduleBranch, ScheduleInput},
    spawn_attempts::AttemptPlan,
    zako_spot::{RuntimeZakoSpot, ZakoSpotOutcome},
};

const POINT: RuntimeZakoSpot = RuntimeZakoSpot {
    origin: [1.0, 2.0, 3.0],
    forward: [0.0, 0.0, 1.0],
};

fn runner() -> EventZeroLocationRunner {
    EventZeroLocationRunner::new(EventZeroLocationStart {
        schedule: ScheduleInput {
            seed: SeedWords::from_scene([1, 2, 3, 4]),
            event_type: 0,
            wave_change_num: 3,
            rare_appear_num: 4,
            assumed_branch: ScheduleBranch::Special,
        },
        stage_keys: &[0, 1, 2],
        accepted_counter: 0,
        population: EventZeroPopulation::default(),
        population_basis: PopulationBasis::Idealized,
        initial_timer: 0,
        rare_request_path_basis: RareRequestPathBasis::AssumedStableCaps,
    })
    .unwrap()
}

fn item() -> EventZeroTraceItem {
    EventZeroTraceItem {
        periodic_event: Some(0),
        global_cycle_flag: false,
        fog_cycle: None,
        event_none_cycle: None,
        global_word: 0x10100,
        limits: OrdinaryItemLimitInput {
            effective_rare_follower_num: 2,
            scene_mode: 0,
            queue_item_flag: false,
            deadline_marker: i32::MIN,
            director_time: 0,
        },
        forced_id15_capacity_available: None,
    }
}

fn one_rare_call(runner: &mut EventZeroLocationRunner) {
    let traced = runner
        .trace_next(
            item(),
            Ok::<_, &'static str>([true; 11]),
            |_, _, _| Ok(SpotRoute::Generic(3)),
            |_| Ok(Some(PostSpotOutcome::Allowed)),
            |_| {
                Ok(LateAttemptState {
                    observed_active_total: 0,
                    observed_active_rare_total: 0,
                    selected_cap: 3,
                    selected_active: 0,
                    follower_cap: 0,
                    follower_active_after_selected: 0,
                    follower_budget: 0,
                })
            },
        )
        .unwrap();
    assert!(matches!(
        traced.due,
        PlannedDue::Selected {
            plan: AttemptPlan::Calls {
                rare_create_call: true,
                follower_create_calls: 0,
                ..
            },
            ..
        }
    ));
}

fn blocked() -> EventZeroFinishInput<'static> {
    EventZeroFinishInput {
        timer: RushTimerGates {
            pre_countdown_allows: false,
            population_gate_allows: None,
            reset_interval: None,
            post_reset_allows: None,
        },
        selector: None,
        rng_boundary: DirectorRngBoundary::Predicted,
    }
}

fn selector<'a>(group: &'a [Option<RuntimeZakoSpot>]) -> RushSelectorInput<'a> {
    let mut runtime_groups: [Option<&[Option<RuntimeZakoSpot>]>; 9] = [None; 9];
    runtime_groups[0] = Some(group);
    RushSelectorInput {
        observed_rng_before_selector: None,
        eligible_visit_count: 3,
        selection_succeeded: true,
        default_callback_route_confirmed: true,
        applied_mode: 0,
        tide: 0,
        runtime_groups,
        tree_30: &[],
        tree_50: &[],
        scratch_capacity: 64,
    }
}

fn due_two_with_failed_registration(runner: &mut EventZeroLocationRunner) {
    let clock = runner.advance_clock(0, true, true).unwrap();
    assert_eq!(clock.due_count, 2);
    one_rare_call(runner);
    runner
        .submit_receipt(CreationReceipt {
            selected_registered: false,
            follower_registered: 0,
        })
        .unwrap();
    one_rare_call(runner);
    runner
        .submit_receipt(CreationReceipt {
            selected_registered: false,
            follower_registered: 0,
        })
        .unwrap();
}

#[test]
fn rare_request_draws_precede_the_zako_timer_even_when_registration_fails() {
    let mut runner = runner();
    runner.advance_clock(0, true, true).unwrap();
    assert_eq!(
        runner.finish_call(blocked()),
        Err(EventZeroLocationError::PendingRareItems(2))
    );
    one_rare_call(&mut runner);
    assert_eq!(
        runner.director_rng_words(),
        [0x442f_e07b, 0x215a_da25, 0x5a98_dd9d, 0x1b52_7e6b]
    );
    assert_eq!(
        runner.finish_call(blocked()),
        Err(EventZeroLocationError::PendingRareReceipt)
    );
    runner
        .submit_receipt(CreationReceipt {
            selected_registered: false,
            follower_registered: 0,
        })
        .unwrap();
    one_rare_call(&mut runner);
    runner
        .submit_receipt(CreationReceipt {
            selected_registered: false,
            follower_registered: 0,
        })
        .unwrap();
    let finished = runner.finish_call(blocked()).unwrap();
    assert_eq!(finished.rare_request_draws, 2);
    assert_eq!(finished.held_direction, Some(0));
    assert_eq!(
        finished.rng_after_rare,
        [0x215a_da25, 0x5a98_dd9d, 0x1b52_7e6b, 0x2045_6942]
    );
    assert_eq!(finished.rng_after_direct_path, finished.rng_after_rare);
    assert!(finished.spot.is_none());
    assert!(!finished.timer.timer_reset);
}

#[test]
fn true_rng_mismatch_requires_an_explicit_observed_rebase() {
    let mut runner = runner();
    due_two_with_failed_registration(&mut runner);
    let before = runner.director_rng_words();
    let mut verify = blocked();
    verify.rng_boundary = DirectorRngBoundary::Verify([9, 8, 7, 6]);
    assert!(matches!(
        runner.finish_call(verify),
        Err(EventZeroLocationError::ObservedRngMismatch { .. })
    ));
    assert_eq!(runner.director_rng_words(), before);
    assert_eq!(runner.timer(), 0);

    let mut rebased = blocked();
    rebased.rng_boundary = DirectorRngBoundary::UseObserved([9, 8, 7, 6]);
    let finished = runner.finish_call(rebased).unwrap();
    assert_eq!(finished.rng_origin, DirectorRngOrigin::ObservedRebased);
    assert_eq!(finished.rng_after_direct_path, [9, 8, 7, 6]);
}

#[test]
fn selected_zako_point_requires_full_population_before_the_next_call() {
    let mut runner = runner();
    due_two_with_failed_registration(&mut runner);
    let group = [Some(POINT)];
    let finished = runner
        .finish_call(EventZeroFinishInput {
            timer: RushTimerGates {
                pre_countdown_allows: true,
                population_gate_allows: Some(true),
                reset_interval: Some(10),
                post_reset_allows: Some(true),
            },
            selector: Some(selector(&group)),
            rng_boundary: DirectorRngBoundary::Predicted,
        })
        .unwrap();
    assert!(matches!(
        finished.spot.unwrap().outcome,
        ZakoSpotOutcome::SelectedPoint { spot: POINT, .. }
    ));
    assert!(finished.request_draw.is_some());
    assert_eq!(
        runner.advance_clock(1, true, true),
        Err(EventZeroLocationError::NeedsPostZakoPopulation)
    );
    runner
        .reconcile_between_calls(PopulationSnapshot {
            population: EventZeroPopulation::default(),
            basis: PopulationBasis::Idealized,
        })
        .unwrap();
    let next = runner.advance_clock(1, true, true).unwrap();
    assert_eq!(next.due_count, 0);
    let mut second = blocked();
    second.timer.pre_countdown_allows = true;
    let finished_second = runner.finish_call(second).unwrap();
    assert_eq!(finished_second.timer.timer_after, 9);
    assert_eq!(
        finished_second.rng_after_direct_path,
        finished.rng_after_direct_path
    );
    assert_eq!(
        finished_second.rng_origin,
        DirectorRngOrigin::SeedDerivedConditional
    );
    assert_eq!(runner.rare().accepted_counter(), 2);
}

#[test]
fn population_and_queue_path_cannot_be_fabricated_between_due_items() {
    let mut runner = runner();
    assert_eq!(
        runner.advance_clock(0, false, true),
        Err(EventZeroLocationError::QueuePathNotConfirmed)
    );
    assert_eq!(runner.advance_clock(0, true, true).unwrap().due_count, 2);
    assert_eq!(
        runner.reconcile_between_calls(PopulationSnapshot {
            population: EventZeroPopulation::default(),
            basis: PopulationBasis::Idealized,
        }),
        Err(EventZeroLocationError::PopulationDuringCall)
    );
    assert_eq!(runner.rare().stream().next_queue_index(), 0);
}

#[test]
fn rejected_rare_input_leaves_both_rng_streams_and_queue_cursor_intact() {
    let mut runner = runner();
    runner.advance_clock(0, true, true).unwrap();
    let before_director = runner.director_rng_words();
    let before_shared = runner.rare().stream().shared_state();
    assert!(
        runner
            .trace_next(
                item(),
                Err::<[bool; 11], _>("unobserved callback"),
                |_, _, _| panic!("unknown callbacks must stop before spot"),
                |_| panic!("unknown callbacks must stop before post-spot gate"),
                |_| panic!("unknown callbacks must stop before late attempt"),
            )
            .is_err()
    );
    assert_eq!(runner.director_rng_words(), before_director);
    assert_eq!(runner.rare().stream().shared_state(), before_shared);
    assert_eq!(runner.rare().stream().next_queue_index(), 0);
    one_rare_call(&mut runner);
}

#[test]
fn timer_reset_veto_keeps_direct_rare_draws_but_skips_the_zako_spot() {
    let mut runner = runner();
    due_two_with_failed_registration(&mut runner);
    let finished = runner
        .finish_call(EventZeroFinishInput {
            timer: RushTimerGates {
                pre_countdown_allows: true,
                population_gate_allows: Some(true),
                reset_interval: Some(10),
                post_reset_allows: Some(false),
            },
            selector: None,
            rng_boundary: DirectorRngBoundary::Predicted,
        })
        .unwrap();
    assert!(finished.timer.timer_reset);
    assert!(!finished.timer.spawn_path_entered);
    assert!(finished.spot.is_none());
    assert_eq!(finished.rng_after_direct_path, finished.rng_after_rare);
    assert_eq!(runner.advance_clock(1, true, true).unwrap().due_count, 0);
}

#[test]
fn auxiliary_activity_must_be_ruled_out_before_any_clock_or_rng_step() {
    let mut runner = runner();
    let before_director = runner.director_rng_words();
    let before_shared = runner.rare().stream().shared_state();
    assert_eq!(
        runner.advance_clock(0, true, false),
        Err(EventZeroLocationError::AuxiliaryPathNotConfirmed)
    );
    assert_eq!(runner.director_rng_words(), before_director);
    assert_eq!(runner.rare().stream().shared_state(), before_shared);
    assert_eq!(runner.advance_clock(0, true, true).unwrap().due_count, 2);
}

#[test]
fn elapsed_ticks_may_repeat_but_cannot_go_backwards() {
    let mut runner = runner();
    due_two_with_failed_registration(&mut runner);
    runner.finish_call(blocked()).unwrap();
    assert_eq!(runner.advance_clock(1, true, true).unwrap().due_count, 0);
    runner.finish_call(blocked()).unwrap();
    assert_eq!(
        runner.advance_clock(0, true, true),
        Err(EventZeroLocationError::DecreasingElapsedTick {
            previous: 1,
            current: 0,
        })
    );
    assert_eq!(runner.advance_clock(1, true, true).unwrap().due_count, 0);
}

#[test]
fn follower_request_draws_count_even_when_registration_fails() {
    let mut runner = runner();
    assert_eq!(runner.advance_clock(0, true, true).unwrap().due_count, 2);
    let mut expected_rng = XorShift128::from_words(runner.director_rng_words());
    let traced = runner
        .trace_next(
            item(),
            Ok::<_, &'static str>([true; 11]),
            |_, _, _| Ok(SpotRoute::Generic(3)),
            |_| Ok(Some(PostSpotOutcome::Allowed)),
            |_| {
                Ok(LateAttemptState {
                    observed_active_total: 0,
                    observed_active_rare_total: 0,
                    selected_cap: 3,
                    selected_active: 0,
                    follower_cap: 2,
                    follower_active_after_selected: 0,
                    follower_budget: 2,
                })
            },
        )
        .unwrap();
    assert!(
        matches!(
            traced.due,
            PlannedDue::Selected {
                plan: AttemptPlan::Calls {
                    rare_create_call: true,
                    follower_create_calls: 2,
                    ..
                },
                ..
            }
        ),
        "{traced:?}"
    );
    for _ in 0..3 {
        expected_rng.next32();
    }
    assert_eq!(runner.director_rng_words(), expected_rng.as_array());
    runner
        .submit_receipt(CreationReceipt {
            selected_registered: false,
            follower_registered: 0,
        })
        .unwrap();
    assert_eq!(runner.director_rng_words(), expected_rng.as_array());
    one_rare_call(&mut runner);
    runner
        .submit_receipt(CreationReceipt {
            selected_registered: false,
            follower_registered: 0,
        })
        .unwrap();
    let finished = runner.finish_call(blocked()).unwrap();
    assert_eq!(finished.rare_request_draws, 4);
    assert!(!finished.timer.spawn_path_entered);
}
