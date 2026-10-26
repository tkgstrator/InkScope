use salmon_wave_lookahead::{
    SeedWords,
    event_zero_counter::PostSpotOutcome,
    event_zero_runner::{
        CreationReceipt, EventZeroPopulation, EventZeroRunner, EventZeroTraceItem,
        LateAttemptState, RunnerInputError, RunnerStateError, RunnerTraceError,
    },
    levels::OrdinaryItemLimitInput,
    ordinary_stream::{OrdinaryStream, PlannedDue, SpotRoute, TraceError},
    schedule::{ScheduleBranch, ScheduleInput},
    spawn_attempts::{AttemptPlan, GoldieAttemptPlan},
};

fn stream() -> OrdinaryStream {
    stream_for_event(0)
}

fn stream_for_event(event_type: u8) -> OrdinaryStream {
    OrdinaryStream::new(
        ScheduleInput {
            seed: SeedWords::from_scene([1, 2, 3, 4]),
            event_type,
            wave_change_num: 3,
            rare_appear_num: 4,
            assumed_branch: ScheduleBranch::Special,
        },
        &[0, 1, 2],
    )
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

#[test]
fn fog_runner_uses_updated_counter_after_same_tick_receipt() {
    let mut runner = EventZeroRunner::new(stream_for_event(5), 2, EventZeroPopulation::default());
    assert_eq!(runner.advance_clock(0).unwrap().due_count, 2);
    let mut fog = item();
    fog.periodic_event = Some(5);
    fog.fog_cycle = Some(3);
    fog.limits.scene_mode = 5;
    fog.forced_id15_capacity_available = Some(true);
    let first = runner
        .trace_next(
            fog,
            Err::<[bool; 11], _>("forced path has no normal callback input"),
            |id, _, _| {
                assert_eq!(id, 15);
                Ok(SpotRoute::Generic(3))
            },
            |_| Ok(Some(PostSpotOutcome::Allowed)),
            |_| {
                Ok(LateAttemptState {
                    follower_cap: 0,
                    follower_budget: 0,
                    ..late()
                })
            },
        )
        .unwrap();
    assert_eq!(first.counter.before, 2);
    assert_eq!(first.counter.after, 3);
    assert!(matches!(
        first.due,
        PlannedDue::ForcedGoldie {
            plan: GoldieAttemptPlan::Calls { follower_id: 0, .. },
            ..
        }
    ));
    runner
        .submit_receipt(CreationReceipt {
            selected_registered: true,
            follower_registered: 0,
        })
        .unwrap();
    assert_eq!(runner.population().active_by_id[15], 1);
    assert_eq!(runner.population().active_rare_total, 0);

    let second = runner
        .trace_next(
            fog,
            Ok::<_, &'static str>([true; 11]),
            |id, _, _| {
                assert_ne!(id, 15);
                Ok(SpotRoute::Generic(3))
            },
            |_| Ok(Some(PostSpotOutcome::Vetoed)),
            |_| panic!("vetoed path has no creation input"),
        )
        .unwrap();
    assert!(matches!(second.due, PlannedDue::Selected { .. }));
    assert_eq!(second.counter.before, 3);
    assert_eq!(runner.stream().next_queue_index(), 2);
}

#[test]
fn missile_runner_uses_normal_scan_despite_cycle_inputs() {
    let mut runner = EventZeroRunner::new(stream_for_event(6), 0, EventZeroPopulation::default());
    runner.advance_clock(0).unwrap();
    let mut missile = item();
    missile.periodic_event = Some(6);
    missile.limits.scene_mode = 6;
    missile.global_cycle_flag = true;
    missile.fog_cycle = Some(1);
    missile.event_none_cycle = Some(1);
    let result = runner
        .trace_next(
            missile,
            Ok::<_, &'static str>([true; 11]),
            |id, _, _| {
                assert_ne!(id, 15);
                Ok(SpotRoute::Generic(3))
            },
            |_| Ok(Some(PostSpotOutcome::Vetoed)),
            |_| panic!("vetoed path has no creation input"),
        )
        .unwrap();
    assert!(matches!(result.due, PlannedDue::Selected { .. }));
    assert_eq!(runner.accepted_counter(), 0);

    let allowed = runner
        .trace_next(
            missile,
            Ok::<_, &'static str>([true; 11]),
            |id, _, _| {
                assert_ne!(id, 15);
                Ok(SpotRoute::Generic(3))
            },
            |_| Ok(Some(PostSpotOutcome::Allowed)),
            |_| Ok(late()),
        )
        .unwrap();
    assert!(matches!(
        allowed.due,
        PlannedDue::Selected {
            plan: AttemptPlan::Calls { follower_id: 0, .. },
            ..
        }
    ));
    assert_eq!(runner.accepted_counter(), 1);
}

fn late() -> LateAttemptState {
    LateAttemptState {
        observed_active_total: 0,
        observed_active_rare_total: 0,
        selected_cap: 3,
        selected_active: 0,
        follower_cap: 20,
        follower_active_after_selected: 0,
        follower_budget: 20,
    }
}

fn allowed_trace(
    runner: &mut EventZeroRunner,
) -> salmon_wave_lookahead::ordinary_stream::CounterTracedDue {
    trace_with_late(runner, item(), late())
}

fn trace_with_late(
    runner: &mut EventZeroRunner,
    item: EventZeroTraceItem,
    late: LateAttemptState,
) -> salmon_wave_lookahead::ordinary_stream::CounterTracedDue {
    runner
        .trace_next(
            item,
            Ok::<_, &'static str>([true; 11]),
            |_, _, _| Ok(SpotRoute::Generic(3)),
            |_| Ok(Some(PostSpotOutcome::Allowed)),
            |_| Ok(late),
        )
        .unwrap()
}

#[test]
fn runner_keeps_accepted_counter_separate_from_reconciled_population() {
    let mut runner = EventZeroRunner::new(stream(), 7, EventZeroPopulation::default());
    assert_eq!(runner.accepted_counter(), 7);
    assert_eq!(runner.population().active_total, 0);
    assert_eq!(runner.advance_clock(0).unwrap().due_count, 2);

    let mut observed = EventZeroPopulation::default();
    observed.active_by_id[6] = 1;
    observed.active_total = 1;
    observed.active_rare_total = 1;
    runner.reconcile_population(observed).unwrap();
    assert_eq!(*runner.population(), observed);
    assert_eq!(runner.accepted_counter(), 7);
    assert_eq!(runner.stream().remaining_due_count(), 2);
}

#[test]
fn successful_trace_waits_for_receipt_before_next_same_tick_item() {
    let mut runner = EventZeroRunner::new(stream(), 0, EventZeroPopulation::default());
    assert_eq!(runner.advance_clock(0).unwrap().due_count, 2);
    let first = allowed_trace(&mut runner);
    assert_eq!(first.counter.after, 1);
    assert!(matches!(
        first.due,
        PlannedDue::Selected {
            plan: AttemptPlan::Calls {
                rare_create_call: true,
                follower_create_calls: 2,
                ..
            },
            ..
        }
    ));
    assert!(runner.pending_receipt());
    assert_eq!(runner.population().active_total, 0);
    let cursor = runner.stream().next_queue_index();
    let rng = runner.stream().shared_state();
    assert!(matches!(
        runner.trace_next(
            item(),
            Ok::<_, &'static str>([true; 11]),
            |_, _, _| panic!("spot callback must not run while pending"),
            |_| panic!("post-spot callback must not run while pending"),
            |_| panic!("late input callback must not run while pending"),
        ),
        Err(RunnerTraceError::PendingReceipt)
    ));
    assert_eq!(
        runner.advance_clock(1),
        Err(RunnerStateError::PendingReceipt)
    );
    assert_eq!(runner.stream().next_queue_index(), cursor);
    assert_eq!(runner.stream().shared_state(), rng);

    runner
        .submit_receipt(CreationReceipt {
            selected_registered: true,
            follower_registered: 2,
        })
        .unwrap();
    assert!(!runner.pending_receipt());
    assert_eq!(runner.population().active_by_id[6], 1);
    assert_eq!(runner.population().active_by_id[0], 2);
    assert_eq!(runner.population().active_total, 3);
    assert_eq!(runner.population().active_rare_total, 1);
    assert_eq!(runner.accepted_counter(), 1);
}

#[test]
fn invalid_receipt_is_retryable_without_changing_population() {
    let mut runner = EventZeroRunner::new(stream(), 0, EventZeroPopulation::default());
    assert_eq!(
        runner.submit_receipt(CreationReceipt {
            selected_registered: false,
            follower_registered: 0,
        }),
        Err(RunnerStateError::NoPendingReceipt)
    );
    runner.advance_clock(0).unwrap();
    allowed_trace(&mut runner);
    assert_eq!(
        runner.submit_receipt(CreationReceipt {
            selected_registered: true,
            follower_registered: 3,
        }),
        Err(RunnerStateError::TooManyFollowerRegistrations {
            registered: 3,
            planned_calls: 2,
        })
    );
    assert!(runner.pending_receipt());
    assert_eq!(runner.population().active_total, 0);
    runner
        .submit_receipt(CreationReceipt {
            selected_registered: false,
            follower_registered: 0,
        })
        .unwrap();
    assert_eq!(runner.population().active_total, 0);
    assert_eq!(runner.accepted_counter(), 1);
}

#[test]
fn a_same_call_receipt_changes_the_next_species_scan() {
    let mut population = EventZeroPopulation::default();
    population.active_by_id[6] = 2;
    population.active_by_id[9] = 3;
    population.active_total = 5;
    population.active_rare_total = 5;
    let mut runner = EventZeroRunner::new(stream(), 0, population);
    assert_eq!(runner.advance_clock(0).unwrap().due_count, 2);
    let mut first_late = late();
    first_late.observed_active_total = 5;
    first_late.observed_active_rare_total = 5;
    first_late.selected_active = 2;
    first_late.follower_cap = 0;
    first_late.follower_budget = 0;
    let first = trace_with_late(&mut runner, item(), first_late);
    let PlannedDue::Selected { draw, .. } = first.due else {
        panic!("a normal species was selected")
    };
    assert_eq!(draw.species_id, 6);
    runner
        .submit_receipt(CreationReceipt {
            selected_registered: true,
            follower_registered: 0,
        })
        .unwrap();
    assert_eq!(runner.population().active_by_id[6], 3);
    assert_eq!(runner.population().active_total, 6);
    assert_eq!(runner.population().active_rare_total, 6);
    assert!(!runner.needs_reconciliation());

    let second = runner
        .trace_next(
            item(),
            Ok::<_, &'static str>([true; 11]),
            |_, direction, _| {
                assert_eq!(direction, draw.direction);
                Ok(SpotRoute::Generic(3))
            },
            |_| Ok(Some(PostSpotOutcome::Allowed)),
            |_| {
                Ok(LateAttemptState {
                    observed_active_total: 6,
                    observed_active_rare_total: 6,
                    selected_active: 0,
                    follower_cap: 0,
                    follower_budget: 0,
                    ..late()
                })
            },
        )
        .unwrap();
    let PlannedDue::Selected { draw: second, .. } = second.due else {
        panic!("the second item selected a species")
    };
    assert_eq!(second.species_id, 7);
    assert_eq!(runner.stream().remaining_due_count(), 0);
}

#[test]
fn forced_goldie_does_not_require_normal_callbacks_or_raise_rare_total() {
    let population = EventZeroPopulation {
        active_rare_total: 15,
        ..EventZeroPopulation::default()
    };
    let mut runner = EventZeroRunner::new(stream(), 2, population);
    runner.advance_clock(0).unwrap();
    let mut forced = item();
    forced.global_cycle_flag = true;
    forced.event_none_cycle = Some(3);
    forced.forced_id15_capacity_available = Some(true);
    let result = runner
        .trace_next(
            forced,
            Err::<[bool; 11], _>("normal scan should be bypassed"),
            |id, _, _| {
                assert_eq!(id, 15);
                Ok(SpotRoute::Generic(3))
            },
            |_| Ok(Some(PostSpotOutcome::Allowed)),
            |_| {
                Ok(LateAttemptState {
                    observed_active_rare_total: 15,
                    follower_cap: 0,
                    follower_budget: 0,
                    ..late()
                })
            },
        )
        .unwrap();
    assert!(matches!(
        result.due,
        PlannedDue::ForcedGoldie {
            plan: GoldieAttemptPlan::Calls {
                goldie_create_call: true,
                follower_create_calls: 0,
                ..
            },
            ..
        }
    ));
    assert_eq!(runner.accepted_counter(), 3);
    runner
        .submit_receipt(CreationReceipt {
            selected_registered: true,
            follower_registered: 0,
        })
        .unwrap();
    assert_eq!(runner.population().active_by_id[15], 1);
    assert_eq!(runner.population().active_total, 1);
    assert_eq!(runner.population().active_rare_total, 15);
}

#[test]
fn late_total_change_is_rejected_without_committing_rng_cursor_or_counter() {
    let mut runner = EventZeroRunner::new(stream(), 0, EventZeroPopulation::default());
    runner.advance_clock(0).unwrap();
    let rng = runner.stream().shared_state();
    let error = runner.trace_next(
        item(),
        Ok::<_, &'static str>([true; 11]),
        |_, _, _| Ok(SpotRoute::Generic(3)),
        |_| Ok(Some(PostSpotOutcome::Allowed)),
        |_| {
            Ok(LateAttemptState {
                observed_active_total: 1,
                ..late()
            })
        },
    );
    assert!(matches!(
        error,
        Err(RunnerTraceError::Trace(TraceError::UnknownInput(
            RunnerInputError::MidItemPopulationChanged {
                expected_total: 0,
                observed_total: 1,
                ..
            }
        )))
    ));
    assert_eq!(runner.stream().shared_state(), rng);
    assert_eq!(runner.stream().next_queue_index(), 0);
    assert_eq!(runner.accepted_counter(), 0);
    assert!(!runner.pending_receipt());
    assert_eq!(runner.population().active_total, 0);
}

#[test]
fn late_per_id_change_requires_reconciliation_even_when_no_call_was_planned() {
    let mut population = EventZeroPopulation::default();
    population.active_by_id[6] = 2;
    population.active_by_id[9] = 3;
    population.active_total = 5;
    population.active_rare_total = 5;
    let mut runner = EventZeroRunner::new(stream(), 0, population);
    runner.advance_clock(0).unwrap();
    let mut later = late();
    later.observed_active_total = 5;
    later.observed_active_rare_total = 5;
    later.selected_active = 3;
    later.follower_cap = 0;
    later.follower_budget = 0;
    let first = trace_with_late(&mut runner, item(), later);
    assert!(matches!(
        first.due,
        PlannedDue::Selected {
            plan: AttemptPlan::Calls {
                rare_create_call: false,
                follower_create_calls: 0,
                ..
            },
            ..
        }
    ));
    assert!(!runner.pending_receipt());
    assert!(runner.needs_reconciliation());
    assert_eq!(runner.accepted_counter(), 1);
    assert!(matches!(
        runner.trace_next(
            item(),
            Ok::<_, &'static str>([true; 11]),
            |_, _, _| panic!("spot callback must not run before reconciliation"),
            |_| panic!("post-spot callback must not run before reconciliation"),
            |_| panic!("late callback must not run before reconciliation"),
        ),
        Err(RunnerTraceError::NeedsReconciliation)
    ));
    assert_eq!(
        runner.advance_clock(1),
        Err(RunnerStateError::NeedsReconciliation)
    );
    let mut observed = population;
    observed.active_by_id[6] = 3;
    observed.active_by_id[9] = 2;
    runner.reconcile_population(observed).unwrap();
    assert!(!runner.needs_reconciliation());
    assert_eq!(runner.accepted_counter(), 1);
}

#[test]
fn unavailable_callback_outcome_is_retryable_without_state_change() {
    let mut runner = EventZeroRunner::new(stream(), 0, EventZeroPopulation::default());
    runner.advance_clock(0).unwrap();
    let rng = runner.stream().shared_state();
    assert_eq!(
        runner.trace_next(
            item(),
            Err::<[bool; 11], _>("candidate unknown"),
            |_, _, _| panic!("spot callback must not run"),
            |_| panic!("post-spot callback must not run"),
            |_| panic!("late callback must not run"),
        ),
        Err(RunnerTraceError::Trace(TraceError::UnknownInput(
            RunnerInputError::External("candidate unknown")
        )))
    );
    assert_eq!(runner.stream().shared_state(), rng);
    assert_eq!(runner.stream().next_queue_index(), 0);
    assert_eq!(runner.accepted_counter(), 0);
    allowed_trace(&mut runner);
    assert_eq!(runner.stream().next_queue_index(), 1);
}

#[test]
fn late_population_difference_persists_after_a_valid_receipt() {
    let mut population = EventZeroPopulation::default();
    population.active_by_id[6] = 2;
    population.active_by_id[9] = 3;
    population.active_total = 5;
    population.active_rare_total = 5;
    let mut runner = EventZeroRunner::new(stream(), 0, population);
    runner.advance_clock(0).unwrap();
    let first = trace_with_late(
        &mut runner,
        item(),
        LateAttemptState {
            observed_active_total: 5,
            observed_active_rare_total: 5,
            selected_active: 3,
            follower_cap: 20,
            follower_budget: 20,
            ..late()
        },
    );
    assert!(matches!(
        first.due,
        PlannedDue::Selected {
            plan: AttemptPlan::Calls {
                rare_create_call: false,
                follower_create_calls: 3,
                ..
            },
            ..
        }
    ));
    assert!(runner.pending_receipt());
    assert!(runner.needs_reconciliation());
    assert_eq!(
        runner.reconcile_population(population),
        Err(RunnerStateError::PendingReceipt)
    );
    assert_eq!(
        runner.submit_receipt(CreationReceipt {
            selected_registered: true,
            follower_registered: 0,
        }),
        Err(RunnerStateError::SelectedWithoutCall)
    );
    runner
        .submit_receipt(CreationReceipt {
            selected_registered: false,
            follower_registered: 1,
        })
        .unwrap();
    assert!(!runner.pending_receipt());
    assert!(runner.needs_reconciliation());
    assert_eq!(runner.population().active_by_id[0], 1);
    assert_eq!(runner.population().active_total, 6);
    assert_eq!(
        runner.advance_clock(1),
        Err(RunnerStateError::NeedsReconciliation)
    );
    let mut observed = population;
    observed.active_by_id[6] = 3;
    observed.active_by_id[9] = 2;
    observed.active_by_id[0] = 1;
    observed.active_total = 6;
    runner.reconcile_population(observed).unwrap();
    assert!(!runner.needs_reconciliation());
    assert_eq!(runner.accepted_counter(), 1);
}

#[test]
fn zero_call_no_species_allows_explicit_same_call_reconciliation() {
    let mut runner = EventZeroRunner::new(stream(), 0, EventZeroPopulation::default());
    runner.advance_clock(0).unwrap();
    let first = runner
        .trace_next(
            item(),
            Ok::<_, &'static str>([false; 11]),
            |_, _, _| panic!("no species has no spot callback"),
            |_| panic!("no species has no post-spot callback"),
            |_| panic!("no species has no late creation input"),
        )
        .unwrap();
    assert!(matches!(first.due, PlannedDue::NoSpecies(_)));
    assert!(!runner.pending_receipt());
    let mut observed = EventZeroPopulation::default();
    observed.active_by_id[0] = 1;
    observed.active_total = 1;
    runner.reconcile_population(observed).unwrap();
    assert_eq!(runner.population().active_total, 1);
    assert_eq!(runner.stream().remaining_due_count(), 1);
}

#[test]
fn late_follower_count_difference_requires_reconciliation_after_receipt() {
    let mut runner = EventZeroRunner::new(stream(), 0, EventZeroPopulation::default());
    runner.advance_clock(0).unwrap();
    let mut scene_seven = item();
    scene_seven.limits.scene_mode = 7;
    let traced = trace_with_late(
        &mut runner,
        scene_seven,
        LateAttemptState {
            follower_cap: 1,
            follower_active_after_selected: 1,
            ..late()
        },
    );
    assert!(matches!(
        traced.due,
        PlannedDue::Selected {
            plan: AttemptPlan::Calls {
                rare_create_call: true,
                follower_id: 1,
                follower_create_calls: 0,
                ..
            },
            ..
        }
    ));
    assert!(runner.pending_receipt());
    assert!(runner.needs_reconciliation());
    runner
        .submit_receipt(CreationReceipt {
            selected_registered: true,
            follower_registered: 0,
        })
        .unwrap();
    assert!(!runner.pending_receipt());
    assert!(runner.needs_reconciliation());
    assert!(matches!(
        runner.trace_next(
            item(),
            Ok::<_, &'static str>([true; 11]),
            |_, _, _| panic!("spot callback must not run before reconciliation"),
            |_| panic!("post-spot callback must not run before reconciliation"),
            |_| panic!("late callback must not run before reconciliation"),
        ),
        Err(RunnerTraceError::NeedsReconciliation)
    ));

    let mut observed = *runner.population();
    observed.active_by_id[1] = 1;
    observed.active_total = 2;
    runner.reconcile_population(observed).unwrap();
    assert!(!runner.needs_reconciliation());
    assert_eq!(runner.accepted_counter(), 1);
    assert_eq!(runner.stream().remaining_due_count(), 1);
}

#[test]
fn late_callback_error_after_species_and_spot_rolls_back_runner_and_stream() {
    let mut runner = EventZeroRunner::new(stream(), 0, EventZeroPopulation::default());
    runner.advance_clock(0).unwrap();
    let rng = runner.stream().shared_state();
    let population = *runner.population();
    let mut spot_calls = 0;
    let mut post_spot_calls = 0;
    let result = runner.trace_next(
        item(),
        Ok::<_, &'static str>([true; 11]),
        |_, _, _| {
            spot_calls += 1;
            Ok(SpotRoute::Generic(3))
        },
        |_| {
            post_spot_calls += 1;
            Ok(Some(PostSpotOutcome::Allowed))
        },
        |_| Err("late input unavailable"),
    );
    assert_eq!(
        result,
        Err(RunnerTraceError::Trace(TraceError::UnknownInput(
            RunnerInputError::External("late input unavailable")
        )))
    );
    assert_eq!(spot_calls, 1);
    assert_eq!(post_spot_calls, 1);
    assert_eq!(runner.stream().shared_state(), rng);
    assert_eq!(runner.stream().next_queue_index(), 0);
    assert_eq!(runner.accepted_counter(), 0);
    assert_eq!(*runner.population(), population);
    assert!(!runner.pending_receipt());
    assert!(!runner.needs_reconciliation());

    let retried = allowed_trace(&mut runner);
    assert_eq!(retried.counter.after, 1);
    assert_eq!(runner.stream().next_queue_index(), 1);
}
