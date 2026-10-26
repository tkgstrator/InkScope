use ocean_calc::{
    SeedWords,
    event_zero_counter::{CounterPhase, PostSpotOutcome},
    ordinary_stream::{
        EventZeroDueContext, EventZeroItemRuntime, ForcedGoldieSpotGate, OrdinaryStream,
        PlannedDue, SpotDraw, SpotRoute, TraceError,
    },
    schedule::{ScheduleBranch, ScheduleInput},
    spawn_attempts::{
        AttemptBlock, AttemptInput, AttemptPlanError, GoldieAttemptPlan,
        plan_event_zero_forced_goldie_attempts,
    },
    species::{PeriodicGoldieInput, RareCandidateState},
    spot::choose_generic_spot_index,
};

fn stream() -> OrdinaryStream {
    let mut wave = OrdinaryStream::new(
        ScheduleInput {
            seed: SeedWords::from_scene([1, 2, 3, 4]),
            event_type: 0,
            wave_change_num: 3,
            rare_appear_num: 4,
            assumed_branch: ScheduleBranch::Special,
        },
        &[0, 1, 2],
    )
    .unwrap();
    assert_eq!(wave.advance_clock(0).unwrap().due_count, 2);
    wave
}

fn forced() -> EventZeroDueContext {
    EventZeroDueContext {
        periodic: PeriodicGoldieInput {
            event: Some(0),
            global_cycle_flag: true,
            accepted_rare_counter: 2,
            fog_cycle: None,
            event_none_cycle: Some(3),
        },
        forced_goldie_spot: Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
            id_fifteen_capacity_available: true,
        }),
        runtime: EventZeroItemRuntime {
            active_rare_total: 15,
            active_total: 2,
            item_total_limit: 15,
            requested_count: 4,
        },
    }
}

fn attempt() -> AttemptInput {
    AttemptInput {
        requested_count: 4,
        selected_rare_id: 15,
        active_rare_total: 15,
        active_total: 2,
        item_total_limit: 15,
        spot_succeeded: true,
        post_spot_allows: true,
        selected_rare_cap: 3,
        selected_rare_active: 1,
        follower_cap: 8,
        follower_active_after_rare: 2,
        follower_budget: 6,
        scene_mode: 0,
    }
}

#[test]
fn direct_goldie_plan_bypasses_rare_total_and_uses_id_zero_followers() {
    assert_eq!(
        plan_event_zero_forced_goldie_attempts(attempt()),
        Ok(GoldieAttemptPlan::Calls {
            requested_count: 4,
            goldie_create_call: true,
            follower_id: 0,
            follower_create_calls: 3,
        })
    );
    let mut capped = attempt();
    capped.selected_rare_active = 3;
    assert_eq!(
        plan_event_zero_forced_goldie_attempts(capped),
        Ok(GoldieAttemptPlan::Calls {
            requested_count: 4,
            goldie_create_call: false,
            follower_id: 0,
            follower_create_calls: 4,
        })
    );
}

#[test]
fn direct_goldie_plan_rejects_nonzero_scene_mode() {
    let mut input = attempt();
    input.scene_mode = 7;
    assert_eq!(
        plan_event_zero_forced_goldie_attempts(input),
        Err(AttemptPlanError::InvalidSceneMode(7))
    );
}

#[test]
fn fog_mode_five_forced_goldie_keeps_id_zero_followers() {
    let mut input = attempt();
    input.scene_mode = 5;
    assert_eq!(
        plan_event_zero_forced_goldie_attempts(input),
        Ok(GoldieAttemptPlan::Calls {
            requested_count: 4,
            goldie_create_call: true,
            follower_id: 0,
            follower_create_calls: 3,
        })
    );
}

#[test]
fn legacy_trace_requires_goldie_attempt_input_after_spot_and_retries() {
    let mut wave = stream();
    let before = wave.shared_state();
    assert_eq!(
        wave.trace_next(
            forced(),
            Err::<[RareCandidateState; 32], _>("unused normal states"),
            |id, _, _| {
                assert_eq!(id, 15);
                Ok(SpotRoute::Generic(3))
            },
            |_| Err("missing Goldie input"),
        ),
        Err(TraceError::UnknownInput("missing Goldie input"))
    );
    assert_eq!(wave.next_queue_index(), 0);
    assert_eq!(wave.shared_state(), before);

    let planned = wave
        .trace_next(
            forced(),
            Err::<[RareCandidateState; 32], _>("unused normal states"),
            |_, _, _| Ok(SpotRoute::Generic(3)),
            |_| Ok(attempt()),
        )
        .unwrap();
    assert!(matches!(
        planned,
        PlannedDue::ForcedGoldie {
            draw: ocean_calc::ordinary_stream::DueDraw {
                spot: SpotDraw::Generic { index: Some(_), .. },
                ..
            },
            plan: GoldieAttemptPlan::Calls {
                goldie_create_call: true,
                follower_create_calls: 3,
                ..
            },
        }
    ));
    assert_eq!(wave.next_queue_index(), 1);
}

#[test]
fn mismatched_goldie_attempt_and_scene_mode_roll_back() {
    let mut wave = stream();
    let before = wave.shared_state();
    for (bad, expected) in [
        (
            AttemptInput {
                active_total: 3,
                ..attempt()
            },
            TraceError::InconsistentAttemptInput,
        ),
        (
            AttemptInput {
                scene_mode: 7,
                ..attempt()
            },
            TraceError::Attempt(AttemptPlanError::InvalidSceneMode(7)),
        ),
        (
            AttemptInput {
                selected_rare_id: 4,
                ..attempt()
            },
            TraceError::InconsistentAttemptInput,
        ),
    ] {
        assert_eq!(
            wave.trace_next_with_counter(
                forced(),
                Ok::<_, &'static str>([RareCandidateState::Skip; 32]),
                |_, _, _| Ok(SpotRoute::Generic(1)),
                |_| Ok(Some(PostSpotOutcome::Allowed)),
                |_| Ok(bad),
            ),
            Err(expected)
        );
        assert_eq!(wave.next_queue_index(), 0);
        assert_eq!(wave.shared_state(), before);
    }
    let good = wave
        .trace_next_with_counter(
            forced(),
            Ok::<_, &'static str>([RareCandidateState::Skip; 32]),
            |_, _, _| Ok(SpotRoute::Generic(1)),
            |_| Ok(Some(PostSpotOutcome::Allowed)),
            |_| Ok(attempt()),
        )
        .unwrap();
    assert_eq!(good.counter.phase, CounterPhase::LatePostSpot);
    assert_eq!(good.counter.after, 3);
    assert_eq!(wave.next_queue_index(), 1);
}

#[test]
fn legacy_spot_failure_and_veto_require_observed_input_but_skip_calls() {
    for (route, input, expected) in [
        (
            SpotRoute::Generic(0),
            AttemptInput {
                spot_succeeded: false,
                ..attempt()
            },
            AttemptBlock::SpotFailed,
        ),
        (
            SpotRoute::HandledByCallback,
            AttemptInput {
                post_spot_allows: false,
                ..attempt()
            },
            AttemptBlock::PostSpotVeto,
        ),
    ] {
        let mut wave = stream();
        let before = wave.shared_state();
        assert_eq!(
            wave.trace_next(
                forced(),
                Ok::<_, &'static str>([RareCandidateState::Skip; 32]),
                |_, _, _| Ok(route),
                |_| Err("Goldie spot result missing"),
            ),
            Err(TraceError::UnknownInput("Goldie spot result missing"))
        );
        assert_eq!(wave.next_queue_index(), 0);
        assert_eq!(wave.shared_state(), before);
        let result = wave
            .trace_next(
                forced(),
                Ok::<_, &'static str>([RareCandidateState::Skip; 32]),
                |_, _, _| Ok(route),
                |_| Ok(input),
            )
            .unwrap();
        assert!(
            matches!(result, PlannedDue::ForcedGoldie { plan: GoldieAttemptPlan::Skipped(reason), .. } if reason == expected)
        );
    }
}

#[test]
fn accepted_counter_advances_before_capped_or_failed_goldie_create_call() {
    let mut wave = stream();
    let traced = wave
        .trace_next_with_counter(
            forced(),
            Ok::<_, &'static str>([RareCandidateState::Skip; 32]),
            |_, _, _| Ok(SpotRoute::HandledByCallback),
            |_| Ok(Some(PostSpotOutcome::Allowed)),
            |_| {
                Ok(AttemptInput {
                    selected_rare_active: 3,
                    ..attempt()
                })
            },
        )
        .unwrap();
    assert_eq!(traced.counter.phase, CounterPhase::LatePostSpot);
    assert!(matches!(
        traced.due,
        PlannedDue::ForcedGoldie {
            plan: GoldieAttemptPlan::Calls {
                goldie_create_call: false,
                follower_create_calls: 4,
                ..
            },
            ..
        }
    ));
    // A call plan does not promise successful creation or registration.
    let mut wave = stream();
    let traced = wave
        .trace_next_with_counter(
            forced(),
            Ok::<_, &'static str>([RareCandidateState::Skip; 32]),
            |_, _, _| Ok(SpotRoute::HandledByCallback),
            |_| Ok(Some(PostSpotOutcome::Allowed)),
            |_| Ok(attempt()),
        )
        .unwrap();
    assert_eq!(traced.counter.phase, CounterPhase::LatePostSpot);
    assert!(matches!(
        traced.due,
        PlannedDue::ForcedGoldie {
            plan: GoldieAttemptPlan::Calls {
                goldie_create_call: true,
                ..
            },
            ..
        }
    ));
}

#[test]
fn forced_skip_branches_do_not_request_attempt_input() {
    for (item, route, reason) in [
        (
            EventZeroDueContext {
                forced_goldie_spot: Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
                    id_fifteen_capacity_available: false,
                }),
                ..forced()
            },
            None,
            AttemptBlock::GoldiePreSpotCapacity,
        ),
        (
            EventZeroDueContext {
                runtime: EventZeroItemRuntime {
                    active_total: 16,
                    requested_count: 0,
                    ..forced().runtime
                },
                forced_goldie_spot: None,
                ..forced()
            },
            Some(SpotRoute::Generic(1)),
            AttemptBlock::TotalGlobalLimit,
        ),
        (
            EventZeroDueContext {
                runtime: EventZeroItemRuntime {
                    requested_count: 0,
                    ..forced().runtime
                },
                ..forced()
            },
            None,
            AttemptBlock::RequestCount,
        ),
    ] {
        let mut wave = stream();
        let expected_route = route;
        let traced = wave
            .trace_next_with_counter(
                item,
                Ok::<_, &'static str>([RareCandidateState::Skip; 32]),
                move |_, _, _| Ok(expected_route.expect("spot selector reached only over total")),
                |_| panic!("post-spot input is unreachable"),
                |_| panic!("attempt input is unreachable"),
            )
            .unwrap();
        assert!(
            matches!(traced.due, PlannedDue::ForcedGoldie { plan: GoldieAttemptPlan::Skipped(block), .. } if block == reason)
        );
        assert_eq!(traced.counter.phase, CounterPhase::Unchanged);
    }
}

#[test]
fn legacy_goldie_total_and_request_blocks_skip_attempt_input() {
    let over_total = EventZeroDueContext {
        runtime: EventZeroItemRuntime {
            active_total: 16,
            requested_count: 0,
            ..forced().runtime
        },
        forced_goldie_spot: None,
        ..forced()
    };
    let mut wave = stream();
    let result = wave
        .trace_next(
            over_total,
            Ok::<_, &'static str>([RareCandidateState::Skip; 32]),
            |_, _, _| Ok(SpotRoute::Generic(1)),
            |_| panic!("over-total Goldie does not need creation input"),
        )
        .unwrap();
    assert!(matches!(
        result,
        PlannedDue::ForcedGoldie {
            plan: GoldieAttemptPlan::Skipped(AttemptBlock::TotalGlobalLimit),
            ..
        }
    ));

    let mut wave = stream();
    let request_zero = EventZeroDueContext {
        runtime: EventZeroItemRuntime {
            requested_count: 0,
            ..forced().runtime
        },
        ..forced()
    };
    let result = wave
        .trace_next(
            request_zero,
            Ok::<_, &'static str>([RareCandidateState::Skip; 32]),
            |_, _, _| panic!("request-count block precedes spot"),
            |_| panic!("request-count block precedes creation input"),
        )
        .unwrap();
    assert!(matches!(
        result,
        PlannedDue::ForcedGoldie {
            plan: GoldieAttemptPlan::Skipped(AttemptBlock::RequestCount),
            ..
        }
    ));
}

#[test]
fn forced_spot_failure_and_veto_skip_attempt_input() {
    for (route, outcome, block) in [
        (
            SpotRoute::Generic(0),
            PostSpotOutcome::SpotFailed,
            AttemptBlock::SpotFailed,
        ),
        (
            SpotRoute::HandledByCallback,
            PostSpotOutcome::Vetoed,
            AttemptBlock::PostSpotVeto,
        ),
    ] {
        let mut wave = stream();
        let traced = wave
            .trace_next_with_counter(
                forced(),
                Ok::<_, &'static str>([RareCandidateState::Skip; 32]),
                |_, _, _| Ok(route),
                |_| Ok(Some(outcome)),
                |_| panic!("attempt input is unreachable"),
            )
            .unwrap();
        assert!(
            matches!(traced.due, PlannedDue::ForcedGoldie { plan: GoldieAttemptPlan::Skipped(reason), .. } if reason == block)
        );
        assert_eq!(traced.counter.phase, CounterPhase::Unchanged);
    }
}

#[test]
fn forced_spot_rng_depends_on_route_and_pre_spot_gate() {
    for (route, draws) in [
        (SpotRoute::Generic(3), true),
        (SpotRoute::Generic(0), false),
        (SpotRoute::HandledByCallback, false),
    ] {
        let mut wave = stream();
        let mut expected = wave.shared_state();
        if draws {
            choose_generic_spot_index(&mut expected, 3);
        }
        let due = wave
            .draw_next(forced(), [RareCandidateState::Skip; 32], |_, _, _| route)
            .unwrap();
        assert_eq!(due.shared_state_after, expected);
        assert_eq!(wave.shared_state(), expected);
    }
    let mut wave = stream();
    let before = wave.shared_state();
    let item = EventZeroDueContext {
        forced_goldie_spot: Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
            id_fifteen_capacity_available: false,
        }),
        ..forced()
    };
    let due = wave
        .draw_next(item, [RareCandidateState::Skip; 32], |_, _, _| {
            panic!("blocked before spot")
        })
        .unwrap();
    assert_eq!(due.spot, SpotDraw::BlockedBeforeSpot);
    assert_eq!(wave.shared_state(), before);
}

#[test]
fn over_total_goldie_still_routes_spot_with_route_specific_rng() {
    let over_total = EventZeroDueContext {
        runtime: EventZeroItemRuntime {
            active_total: 16,
            requested_count: 0,
            ..forced().runtime
        },
        forced_goldie_spot: None,
        ..forced()
    };
    for (route, consumes_shared_draw) in [
        (SpotRoute::Generic(3), true),
        (SpotRoute::Generic(0), false),
        (SpotRoute::HandledByCallback, false),
    ] {
        let mut wave = stream();
        let mut expected = wave.shared_state();
        if consumes_shared_draw {
            choose_generic_spot_index(&mut expected, 3);
        }
        let traced = wave
            .trace_next_with_counter(
                over_total,
                Ok::<_, &'static str>([RareCandidateState::Skip; 32]),
                |id, _, _| {
                    assert_eq!(id, 15);
                    Ok(route)
                },
                |_| panic!("over-total branch does not need post-spot input"),
                |_| panic!("over-total branch does not create"),
            )
            .unwrap();
        assert!(matches!(
            traced.due,
            PlannedDue::ForcedGoldie {
                plan: GoldieAttemptPlan::Skipped(AttemptBlock::TotalGlobalLimit),
                ..
            }
        ));
        assert_eq!(traced.counter.phase, CounterPhase::Unchanged);
        assert_eq!(wave.shared_state(), expected);
    }
}
