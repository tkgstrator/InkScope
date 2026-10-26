use salmon_wave_lookahead::{
    SeedWords,
    ordinary_stream::{
        EventZeroDueContext, EventZeroItemRuntime, ForcedGoldieSpotGate, OrdinaryStream,
        PlannedDue, SpotDraw, SpotRoute, TraceError,
    },
    schedule::{ScheduleBranch, ScheduleInput},
    spawn_attempts::{AttemptBlock, AttemptInput, AttemptPlan},
    species::{PeriodicGoldieInput, RareCandidateState, normal_first_rare_candidates},
};

fn context() -> EventZeroDueContext {
    EventZeroDueContext {
        periodic: PeriodicGoldieInput {
            event: Some(0),
            global_cycle_flag: false,
            accepted_rare_counter: 0,
            fog_cycle: None,
            event_none_cycle: None,
        },
        forced_goldie_spot: None,
        runtime: EventZeroItemRuntime {
            active_rare_total: 0,
            active_total: 0,
            item_total_limit: 15,
            requested_count: 3,
        },
    }
}

fn stream() -> OrdinaryStream {
    OrdinaryStream::new(
        ScheduleInput {
            seed: SeedWords::from_scene([1, 2, 3, 4]),
            event_type: 0,
            wave_change_num: 3,
            rare_appear_num: 4,
            assumed_branch: ScheduleBranch::Special,
        },
        &[0, 1, 2],
    )
    .unwrap()
}

fn candidates() -> [RareCandidateState; 32] {
    normal_first_rare_candidates(0x10100, [true; 11])
}

fn input(id: u8, spot_succeeded: bool) -> AttemptInput {
    AttemptInput {
        requested_count: 3,
        selected_rare_id: id,
        active_rare_total: 0,
        active_total: 0,
        item_total_limit: 15,
        spot_succeeded,
        post_spot_allows: true,
        selected_rare_cap: 3,
        selected_rare_active: 0,
        follower_cap: 20,
        follower_active_after_rare: 0,
        follower_budget: 20,
        scene_mode: 0,
    }
}

#[test]
fn a_due_item_traces_species_spot_and_creation_calls_together() {
    let mut wave = stream();
    assert_eq!(wave.advance_clock(0).unwrap().due_count, 2);
    let result = wave
        .trace_next(
            context(),
            Ok::<_, &'static str>(candidates()),
            |species, direction, _| {
                assert_eq!(species, 6);
                assert_eq!(direction, Some(0));
                Ok(SpotRoute::Generic(3))
            },
            |draw| {
                assert_eq!(
                    draw.spot,
                    SpotDraw::Generic {
                        candidate_count: 3,
                        index: Some(2)
                    }
                );
                // The trace supplies the drawn ID to the planner.
                Ok(input(0, true))
            },
        )
        .unwrap();
    assert!(matches!(
        result,
        PlannedDue::Selected {
            plan: AttemptPlan::Calls {
                effective_budget: 3,
                rare_create_call: true,
                follower_id: 0,
                follower_create_calls: 2,
            },
            ..
        }
    ));
    assert_eq!(wave.next_queue_index(), 1);
}

#[test]
fn unknown_inputs_leave_the_first_due_item_available_for_a_matching_retry() {
    let mut wave = stream();
    wave.advance_clock(0).unwrap();
    let initial_rng = wave.shared_state();
    assert_eq!(
        wave.trace_next(
            context(),
            Err::<[RareCandidateState; 32], _>("candidate callback unknown"),
            |_, _, _| panic!("spot needs a species"),
            |_| panic!("runtime needs a draw"),
        ),
        Err(TraceError::UnknownInput("candidate callback unknown"))
    );
    assert_eq!(wave.shared_state(), initial_rng);
    assert_eq!(wave.next_queue_index(), 0);

    assert_eq!(
        wave.trace_next(
            context(),
            Ok(candidates()),
            |_, _, _| Err("spot callback unknown"),
            |_| panic!("runtime needs a spot"),
        ),
        Err(TraceError::UnknownInput("spot callback unknown"))
    );
    assert_eq!(wave.shared_state(), initial_rng);
    assert_eq!(wave.next_queue_index(), 0);

    assert_eq!(
        wave.trace_next(
            context(),
            Ok(candidates()),
            |_, _, _| Ok(SpotRoute::Generic(3)),
            |_| Err("selected actor not inspected"),
        ),
        Err(TraceError::UnknownInput("selected actor not inspected"))
    );
    assert_eq!(wave.shared_state(), initial_rng);
    assert_eq!(wave.next_queue_index(), 0);

    let retry = wave
        .trace_next(
            context(),
            Ok::<_, &'static str>(candidates()),
            |_, _, _| Ok(SpotRoute::Generic(3)),
            |draw| Ok(input(draw.species_id, true)),
        )
        .unwrap();
    let PlannedDue::Selected { draw, .. } = retry else {
        panic!("first due item should select a Rare species")
    };
    assert_eq!(draw.queue_index, 0);
    assert_eq!(draw.species_id, 6);
    assert_eq!(
        draw.spot,
        SpotDraw::Generic {
            candidate_count: 3,
            index: Some(2)
        }
    );
    assert_eq!(wave.shared_state(), draw.shared_state_after);
}

#[test]
fn inconsistent_attempt_input_keeps_the_prior_species_and_spot_draws_uncommitted() {
    let mut wave = stream();
    wave.advance_clock(0).unwrap();
    let initial_rng = wave.shared_state();
    assert_eq!(
        wave.trace_next(
            context(),
            Ok::<_, &'static str>(candidates()),
            |_, _, _| Ok(SpotRoute::Generic(3)),
            |draw| {
                let mut value = input(draw.species_id, true);
                value.requested_count = 0;
                Ok(value)
            },
        ),
        Err(TraceError::InconsistentAttemptInput)
    );
    assert_eq!(wave.shared_state(), initial_rng);
    assert_eq!(wave.next_queue_index(), 0);
}

#[test]
fn no_species_skips_spot_and_attempt_inputs_but_consumes_the_queue_item() {
    let mut wave = stream();
    wave.advance_clock(0).unwrap();
    let initial_rng = wave.shared_state();
    let result = wave
        .trace_next(
            context(),
            Ok::<_, &'static str>([RareCandidateState::Skip; 32]),
            |_, _, _| panic!("no species has no spot callback"),
            |_| panic!("no species has no creation attempts"),
        )
        .unwrap();
    let PlannedDue::NoSpecies(draw) = result else {
        panic!("no eligible species must make no creation calls")
    };
    assert_eq!(draw.species_id, 0x20);
    assert_eq!(draw.spot, SpotDraw::NoSpecies);
    assert_eq!(wave.shared_state(), initial_rng);
    assert_eq!(wave.next_queue_index(), 1);
}

#[test]
fn a_generic_index_does_not_prove_that_its_actor_pointer_is_valid() {
    let mut wave = stream();
    wave.advance_clock(0).unwrap();
    let initial_rng = wave.shared_state();
    let result = wave
        .trace_next(
            context(),
            Ok::<_, &'static str>(candidates()),
            |_, _, _| Ok(SpotRoute::Generic(3)),
            |draw| {
                assert_eq!(
                    draw.spot,
                    SpotDraw::Generic {
                        candidate_count: 3,
                        index: Some(2)
                    }
                );
                Ok(input(draw.species_id, false))
            },
        )
        .unwrap();
    assert!(matches!(
        result,
        PlannedDue::Selected {
            plan: AttemptPlan::Skipped(AttemptBlock::SpotFailed),
            ..
        }
    ));
    assert_ne!(wave.shared_state(), initial_rng);
    assert_eq!(wave.next_queue_index(), 1);
}

#[test]
fn an_empty_generic_actor_group_cannot_be_reported_as_a_successful_spot() {
    let mut wave = stream();
    wave.advance_clock(0).unwrap();
    let initial_rng = wave.shared_state();
    assert_eq!(
        wave.trace_next(
            context(),
            Ok::<_, &'static str>(candidates()),
            |_, _, _| Ok(SpotRoute::Generic(0)),
            |draw| {
                assert_eq!(
                    draw.spot,
                    SpotDraw::Generic {
                        candidate_count: 0,
                        index: None
                    }
                );
                Ok(input(draw.species_id, true))
            },
        ),
        Err(TraceError::ImpossibleSpotSuccess)
    );
    assert_eq!(wave.shared_state(), initial_rng);
    assert_eq!(wave.next_queue_index(), 0);
}

#[test]
fn a_same_call_registration_can_change_the_next_species_without_a_clock_step() {
    let mut wave = stream();
    assert_eq!(wave.advance_clock(0).unwrap().due_count, 2);
    // ID9 starts at its cap of three and ID6 starts at two. The first
    // item's conditional successful ID6 registration reaches that cap.
    let mut before_registration = candidates();
    before_registration[9] = RareCandidateState::AtLimit;
    let mut first_context = context();
    first_context.runtime.active_rare_total = 5;
    first_context.runtime.active_total = 5;
    let first = wave
        .trace_next(
            first_context,
            Ok::<_, &'static str>(before_registration),
            |_, _, _| Ok(SpotRoute::Generic(3)),
            |draw| {
                let mut value = input(draw.species_id, true);
                value.active_rare_total = 5;
                value.active_total = 5;
                value.selected_rare_active = 2;
                value.follower_budget = 0;
                Ok(value)
            },
        )
        .unwrap();
    let PlannedDue::Selected { draw: first, .. } = first else {
        panic!("first item selects a species")
    };
    assert_eq!(first.species_id, 6);

    // Conditional successful registration of ID6 during this scheduler call.
    // The caller supplies the updated count/callback state before item two.
    let mut after_registration = before_registration;
    after_registration[6] = RareCandidateState::AtLimit;
    let mut second_context = context();
    second_context.runtime.active_rare_total = 6;
    second_context.runtime.active_total = 6;
    let second = wave
        .trace_next(
            second_context,
            Ok::<_, &'static str>(after_registration),
            |_, direction, _| {
                assert_eq!(direction, Some(0));
                Ok(SpotRoute::Generic(3))
            },
            |draw| {
                let mut value = input(draw.species_id, true);
                value.active_rare_total = 6;
                value.active_total = 6;
                value.follower_budget = 0;
                Ok(value)
            },
        )
        .unwrap();
    let PlannedDue::Selected { draw: second, .. } = second else {
        panic!("second item selects a species")
    };
    assert_eq!(second.queue_index, 1);
    assert_eq!(second.species_id, 7);
    assert_eq!(second.direction, first.direction);
    assert_eq!(wave.remaining_due_count(), 0);

    let mut without_registration = stream();
    without_registration.advance_clock(0).unwrap();
    let baseline_first = without_registration
        .draw_next(context(), before_registration, |_, _, _| {
            SpotRoute::Generic(3)
        })
        .unwrap();
    assert_eq!(baseline_first.species_id, first.species_id);
    let baseline_second = without_registration
        .draw_next(context(), before_registration, |_, _, _| {
            SpotRoute::Generic(3)
        })
        .unwrap();
    assert_eq!(baseline_second.species_id, 5);
    assert_ne!(second.species_id, baseline_second.species_id);
}

#[test]
fn global_creation_limit_does_not_undo_species_or_spot_draws() {
    let mut wave = stream();
    wave.advance_clock(0).unwrap();
    let initial_rng = wave.shared_state();
    let mut item = context();
    item.runtime.active_rare_total = 15;
    let result = wave
        .trace_next(
            item,
            Ok::<_, &'static str>(candidates()),
            |_, _, _| Ok(SpotRoute::Generic(3)),
            |_| panic!("early Rare limit bypasses creation input"),
        )
        .unwrap();
    assert!(matches!(
        result,
        PlannedDue::Selected {
            plan: AttemptPlan::Skipped(AttemptBlock::RareGlobalLimit),
            ..
        }
    ));
    assert_ne!(wave.shared_state(), initial_rng);
    assert_eq!(wave.next_queue_index(), 1);
}

#[test]
fn a_forced_goldie_trace_uses_its_separate_creation_planner() {
    let mut wave = stream();
    wave.advance_clock(0).unwrap();
    let mut item = context();
    item.periodic.global_cycle_flag = true;
    item.periodic.event_none_cycle = Some(3);
    item.periodic.accepted_rare_counter = 2;
    item.forced_goldie_spot = Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
        id_fifteen_capacity_available: true,
    });
    let result = wave
        .trace_next(
            item,
            Ok::<_, &'static str>(candidates()),
            |id, _, _| {
                assert_eq!(id, 15);
                Ok(SpotRoute::HandledByCallback)
            },
            |_| Ok(input(15, true)),
        )
        .unwrap();
    let PlannedDue::ForcedGoldie { draw, plan } = result else {
        panic!("forced ID15 must be identified separately")
    };
    assert_eq!(draw.species_id, 15);
    assert_eq!(draw.spot, SpotDraw::HandledByCallback);
    assert!(matches!(
        plan,
        salmon_wave_lookahead::spawn_attempts::GoldieAttemptPlan::Calls {
            goldie_create_call: true,
            follower_create_calls: 2,
            ..
        }
    ));
    assert_eq!(wave.next_queue_index(), 1);
}

#[test]
fn forced_goldie_does_not_need_normal_candidate_callback_results() {
    let mut wave = stream();
    wave.advance_clock(0).unwrap();
    let before = wave.shared_state();
    let mut item = context();
    item.periodic.global_cycle_flag = true;
    item.periodic.event_none_cycle = Some(1);
    item.forced_goldie_spot = Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
        id_fifteen_capacity_available: false,
    });
    let result = wave
        .trace_next(
            item,
            Err::<[RareCandidateState; 32], _>("normal callbacks unknown"),
            |_, _, _| panic!("blocked Goldie skips spot"),
            |_| panic!("forced Goldie skips normal attempt planner"),
        )
        .unwrap();
    let PlannedDue::ForcedGoldie { draw, plan } = result else {
        panic!("forced ID15 must bypass normal candidates")
    };
    assert_eq!(draw.spot, SpotDraw::BlockedBeforeSpot);
    assert_eq!(
        plan,
        salmon_wave_lookahead::spawn_attempts::GoldieAttemptPlan::Skipped(
            AttemptBlock::GoldiePreSpotCapacity
        )
    );
    assert_eq!(wave.shared_state(), before);
    assert_eq!(wave.next_queue_index(), 1);
}
