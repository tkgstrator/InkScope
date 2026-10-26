use ocean_calc::{
    SeedWords,
    event_zero_counter::{
        CounterAdvance, CounterPhase, CounterProjectionError, PostSpotOutcome,
        project_event_zero_accepted_counter,
    },
    ordinary_stream::{
        DueDraw, EventZeroDueContext, EventZeroItemRuntime, ForcedGoldieSpotGate, OrdinaryStream,
        PlannedDue, SpotDraw, SpotRoute, TraceError,
    },
    rng::XorShift128,
    schedule::{EntryKind, NominalEntry, ScheduleBranch, ScheduleInput},
    spawn_attempts::{AttemptBlock, AttemptInput, AttemptPlan},
    species::{
        PeriodicGoldieInput, PeriodicGoldieSource, RareCandidateState, RareSpeciesSelection,
        normal_first_rare_candidates,
    },
};

fn context(counter: i32, rare: i32, total: i32, limit: i32, request: i32) -> EventZeroDueContext {
    EventZeroDueContext {
        periodic: PeriodicGoldieInput {
            event: Some(0),
            global_cycle_flag: false,
            accepted_rare_counter: counter,
            fog_cycle: None,
            event_none_cycle: None,
        },
        forced_goldie_spot: None,
        runtime: EventZeroItemRuntime {
            active_rare_total: rare,
            active_total: total,
            item_total_limit: limit,
            requested_count: request,
        },
    }
}

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

fn candidates() -> [RareCandidateState; 32] {
    normal_first_rare_candidates(0x10100, [true; 11])
}

fn attempt(item: EventZeroDueContext, drawn: DueDraw, rare_cap: i32) -> AttemptInput {
    AttemptInput {
        requested_count: item.runtime.requested_count,
        selected_rare_id: drawn.species_id,
        active_rare_total: item.runtime.active_rare_total,
        active_total: item.runtime.active_total,
        item_total_limit: item.runtime.item_total_limit,
        spot_succeeded: true,
        post_spot_allows: true,
        selected_rare_cap: rare_cap,
        selected_rare_active: 0,
        follower_cap: 20,
        follower_active_after_rare: 0,
        follower_budget: 0,
        scene_mode: 0,
    }
}

fn draw(selection: RareSpeciesSelection, spot: SpotDraw) -> DueDraw {
    DueDraw {
        queue_index: 0,
        entry: NominalEntry {
            tick: 0,
            kind: EntryKind::Base,
        },
        direction: Some(0),
        species_id: selection.species_id(),
        selection,
        requested_count: 1,
        spot,
        shared_state_after: XorShift128::from_words([1, 2, 3, 4]),
    }
}

#[test]
fn early_rare_limit_beats_total_limit_and_wraps_even_without_species() {
    let item = context(i32::MAX, 15, 20, 15, 0);
    let drawn = draw(RareSpeciesSelection::Normal(0x20), SpotDraw::NoSpecies);
    assert_eq!(
        project_event_zero_accepted_counter(item, drawn, None),
        Ok(CounterAdvance {
            before: i32::MAX,
            after: i32::MIN,
            phase: CounterPhase::EarlyRareLimit,
        })
    );
}

#[test]
fn total_limit_is_draw_only_even_when_request_count_is_zero() {
    let item = context(7, 2, 16, 15, 0);
    let drawn = draw(
        RareSpeciesSelection::Normal(6),
        SpotDraw::Generic {
            candidate_count: 1,
            index: Some(0),
        },
    );
    assert_eq!(
        project_event_zero_accepted_counter(item, drawn, None),
        Ok(CounterAdvance {
            before: 7,
            after: 7,
            phase: CounterPhase::Unchanged,
        })
    );
}

#[test]
fn forced_goldie_does_not_use_the_early_rare_limit() {
    let mut item = context(2, 15, 0, 15, 3);
    item.periodic.global_cycle_flag = true;
    item.periodic.event_none_cycle = Some(3);
    item.forced_goldie_spot = Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
        id_fifteen_capacity_available: false,
    });
    let drawn = draw(
        RareSpeciesSelection::ForcedGoldie(PeriodicGoldieSource::EventNoneGlobalCycle),
        SpotDraw::BlockedBeforeSpot,
    );
    assert_eq!(
        project_event_zero_accepted_counter(item, drawn, None),
        Ok(CounterAdvance {
            before: 2,
            after: 2,
            phase: CounterPhase::Unchanged,
        })
    );
}

#[test]
fn standalone_counter_projects_fog_force_and_missile_normal_branch() {
    let mut fog = context(2, 0, 0, 15, 1);
    fog.periodic.event = Some(5);
    fog.periodic.fog_cycle = Some(3);
    fog.forced_goldie_spot = Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
        id_fifteen_capacity_available: true,
    });
    let forced = draw(
        RareSpeciesSelection::ForcedGoldie(PeriodicGoldieSource::FogCycle),
        SpotDraw::Generic {
            candidate_count: 1,
            index: Some(0),
        },
    );
    assert_eq!(
        project_event_zero_accepted_counter(fog, forced, Some(PostSpotOutcome::Allowed)),
        Ok(CounterAdvance {
            before: 2,
            after: 3,
            phase: CounterPhase::LatePostSpot,
        })
    );

    let mut missile = context(2, 0, 0, 15, 1);
    missile.periodic.event = Some(6);
    missile.periodic.global_cycle_flag = true;
    missile.periodic.fog_cycle = Some(1);
    missile.periodic.event_none_cycle = Some(1);
    let normal = draw(
        RareSpeciesSelection::Normal(6),
        SpotDraw::Generic {
            candidate_count: 1,
            index: Some(0),
        },
    );
    assert_eq!(
        project_event_zero_accepted_counter(missile, normal, Some(PostSpotOutcome::Allowed))
            .unwrap()
            .after,
        3
    );
}

#[test]
fn late_increment_needs_spot_and_virtual_gate_but_not_creation_success() {
    let item = context(5, 0, 0, 15, 1);
    let drawn = draw(
        RareSpeciesSelection::Normal(6),
        SpotDraw::Generic {
            candidate_count: 1,
            index: Some(0),
        },
    );
    assert_eq!(
        project_event_zero_accepted_counter(item, drawn, Some(PostSpotOutcome::Allowed)),
        Ok(CounterAdvance {
            before: 5,
            after: 6,
            phase: CounterPhase::LatePostSpot,
        })
    );
    assert_eq!(
        project_event_zero_accepted_counter(item, drawn, Some(PostSpotOutcome::SpotFailed))
            .unwrap()
            .after,
        5
    );
    assert_eq!(
        project_event_zero_accepted_counter(item, drawn, Some(PostSpotOutcome::Vetoed))
            .unwrap()
            .after,
        5
    );
}

#[test]
fn public_projection_rejects_inconsistent_species_id() {
    let item = context(0, 0, 0, 15, 1);
    let mut drawn = draw(
        RareSpeciesSelection::Normal(6),
        SpotDraw::Generic {
            candidate_count: 1,
            index: Some(0),
        },
    );
    drawn.species_id = 7;
    assert_eq!(
        project_event_zero_accepted_counter(item, drawn, Some(PostSpotOutcome::Allowed)),
        Err(CounterProjectionError::InconsistentSpeciesId)
    );
}

#[test]
fn public_projection_rejects_a_spot_draw_after_the_request_gate() {
    let item = context(0, 0, 0, 15, 0);
    let drawn = draw(
        RareSpeciesSelection::Normal(6),
        SpotDraw::Generic {
            candidate_count: 1,
            index: Some(0),
        },
    );
    assert_eq!(
        project_event_zero_accepted_counter(item, drawn, Some(PostSpotOutcome::Allowed)),
        Err(CounterProjectionError::InconsistentDrawBranch)
    );
}

#[test]
fn public_projection_rejects_a_forced_species_when_periodic_predicate_is_false() {
    let mut item = context(0, 0, 0, 15, 1);
    item.forced_goldie_spot = Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
        id_fifteen_capacity_available: false,
    });
    let drawn = draw(
        RareSpeciesSelection::ForcedGoldie(PeriodicGoldieSource::EventNoneGlobalCycle),
        SpotDraw::BlockedBeforeSpot,
    );
    assert_eq!(
        project_event_zero_accepted_counter(item, drawn, None),
        Err(CounterProjectionError::InconsistentDrawBranch)
    );
}

#[test]
fn request_count_zero_blocks_spot_only_on_creation_capable_path() {
    let mut wave = stream();
    let before = wave.shared_state();
    let draw = wave
        .draw_next(context(0, 0, 0, 15, 0), candidates(), |_, _, _| {
            panic!("request gate returns before spot callback")
        })
        .unwrap();
    assert_eq!(draw.species_id, 6);
    assert_eq!(draw.spot, SpotDraw::BlockedByRequestCount);
    assert_ne!(draw.shared_state_after, before, "species scan still draws");
}

#[test]
fn early_rare_limit_and_over_total_still_draw_spot_with_zero_request() {
    for item in [context(0, 15, 16, 15, 0), context(0, 0, 16, 15, 0)] {
        let mut wave = stream();
        let draw = wave
            .draw_next(item, candidates(), |_, _, _| SpotRoute::Generic(3))
            .unwrap();
        assert!(matches!(
            draw.spot,
            SpotDraw::Generic { index: Some(_), .. }
        ));
    }
}

#[test]
fn no_species_precedes_request_count_gate() {
    let mut wave = stream();
    let drawn = wave
        .draw_next(
            context(0, 0, 0, 15, 0),
            [RareCandidateState::Skip; 32],
            |_, _, _| panic!("no species has no spot"),
        )
        .unwrap();
    assert_eq!(drawn.spot, SpotDraw::NoSpecies);
}

#[test]
fn forced_id_fifteen_capacity_gate_precedes_request_count_gate() {
    let mut item = context(0, 0, 0, 15, 0);
    item.periodic.global_cycle_flag = true;
    item.periodic.event_none_cycle = Some(1);
    item.forced_goldie_spot = Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
        id_fifteen_capacity_available: false,
    });
    let first = stream()
        .draw_next(item, [RareCandidateState::Skip; 32], |_, _, _| {
            panic!("capacity block prevents spot")
        })
        .unwrap();
    assert_eq!(first.spot, SpotDraw::BlockedBeforeSpot);

    item.forced_goldie_spot = Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
        id_fifteen_capacity_available: true,
    });
    let second = stream()
        .draw_next(item, [RareCandidateState::Skip; 32], |_, _, _| {
            panic!("request block prevents spot")
        })
        .unwrap();
    assert_eq!(second.spot, SpotDraw::BlockedByRequestCount);
}

#[test]
fn missing_virtual_gate_rolls_back_then_late_count_precedes_a_suppressed_create() {
    let mut wave = stream();
    let item = context(4, 0, 0, 15, 1);
    let before = wave.shared_state();
    assert_eq!(
        wave.trace_next_with_counter(
            item,
            Ok::<_, &'static str>(candidates()),
            |_, _, _| Ok(SpotRoute::Generic(3)),
            |_| Ok(None),
            |_| panic!("unknown virtual gate must stop before creation input"),
        ),
        Err(TraceError::Counter(
            CounterProjectionError::MissingPostSpotOutcome
        ))
    );
    assert_eq!(wave.shared_state(), before);
    assert_eq!(wave.next_queue_index(), 0);

    let traced = wave
        .trace_next_with_counter(
            item,
            Ok::<_, &'static str>(candidates()),
            |_, _, _| Ok(SpotRoute::Generic(3)),
            |_| Ok(Some(PostSpotOutcome::Allowed)),
            |drawn| Ok(attempt(item, drawn, 0)),
        )
        .unwrap();
    assert_eq!(traced.counter.after, 5);
    assert_eq!(traced.counter.phase, CounterPhase::LatePostSpot);
    assert!(matches!(
        traced.due,
        PlannedDue::Selected {
            draw: DueDraw { species_id: 6, .. },
            plan: AttemptPlan::Calls {
                rare_create_call: false,
                ..
            }
        }
    ));
    assert_eq!(wave.next_queue_index(), 1);
}

#[test]
fn early_limit_with_no_species_needs_no_post_spot_or_creation_input() {
    let mut wave = stream();
    let item = context(8, 15, 20, 15, 0);
    let traced = wave
        .trace_next_with_counter(
            item,
            Ok::<_, &'static str>([RareCandidateState::Skip; 32]),
            |_, _, _| panic!("no species has no spot"),
            |_| panic!("early no-species needs no post-spot outcome"),
            |_| panic!("early no-species needs no creation input"),
        )
        .unwrap();
    assert!(matches!(traced.due, PlannedDue::NoSpecies(_)));
    assert_eq!(traced.counter.after, 9);
    assert_eq!(traced.counter.phase, CounterPhase::EarlyRareLimit);
}

#[test]
fn total_limit_draw_only_path_needs_no_post_spot_or_creation_input() {
    let mut wave = stream();
    let item = context(3, 0, 16, 15, 0);
    let traced = wave
        .trace_next_with_counter(
            item,
            Ok::<_, &'static str>(candidates()),
            |_, _, _| Ok(SpotRoute::Generic(3)),
            |_| panic!("draw-only path ignores spot result and virtual gate"),
            |_| panic!("draw-only path makes no creation call"),
        )
        .unwrap();
    assert!(matches!(
        traced.due,
        PlannedDue::Selected {
            plan: AttemptPlan::Skipped(AttemptBlock::TotalGlobalLimit),
            ..
        }
    ));
    assert_eq!(traced.counter.after, 3);
}

#[test]
fn impossible_spot_success_is_rejected_before_creation_input_and_keeps_cursor() {
    let mut wave = stream();
    let item = context(1, 0, 0, 15, 1);
    let before = wave.shared_state();
    assert_eq!(
        wave.trace_next_with_counter(
            item,
            Ok::<_, &'static str>(candidates()),
            |_, _, _| Ok(SpotRoute::Generic(0)),
            |_| Ok(Some(PostSpotOutcome::Allowed)),
            |_| panic!("empty generic group cannot reach creation input"),
        ),
        Err(TraceError::Counter(
            CounterProjectionError::ImpossibleSpotSuccess
        ))
    );
    assert_eq!(wave.shared_state(), before);
    assert_eq!(wave.next_queue_index(), 0);
}

#[test]
fn same_call_next_periodic_check_sees_the_projected_early_increment() {
    let mut wave = stream();
    let mut first = context(1, 15, 15, 15, 0);
    first.periodic.global_cycle_flag = true;
    first.periodic.event_none_cycle = Some(3);
    let a = wave
        .trace_next_with_counter(
            first,
            Ok::<_, &'static str>(candidates()),
            |_, _, _| Ok(SpotRoute::HandledByCallback),
            |_| panic!("early limit does not need spot success"),
            |_| panic!("early limit does not create"),
        )
        .unwrap();
    assert_eq!(a.counter.after, 2);
    assert_eq!(a.counter.phase, CounterPhase::EarlyRareLimit);

    let mut second = context(a.counter.after, 15, 15, 15, 1);
    second.periodic.global_cycle_flag = true;
    second.periodic.event_none_cycle = Some(3);
    second.forced_goldie_spot = Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
        id_fifteen_capacity_available: false,
    });
    let b = wave
        .trace_next_with_counter(
            second,
            Err::<[RareCandidateState; 32], &'static str>("normal candidates irrelevant"),
            |_, _, _| panic!("blocked Goldie skips spot"),
            |_| panic!("blocked Goldie has no post-spot result"),
            |_| panic!("Goldie is outside ordinary creation planner"),
        )
        .unwrap();
    assert_eq!(b.counter.after, 2);
    assert!(matches!(
        b.due,
        PlannedDue::ForcedGoldie {
            draw: DueDraw {
                species_id: 15,
                spot: SpotDraw::BlockedBeforeSpot,
                ..
            },
            plan: ocean_calc::spawn_attempts::GoldieAttemptPlan::Skipped(
                AttemptBlock::GoldiePreSpotCapacity
            )
        }
    ));
    assert_eq!(wave.remaining_due_count(), 0);
}

#[test]
fn legacy_trace_rejects_a_pre_item_snapshot_that_disagrees_with_the_draw_branch() {
    let mut wave = stream();
    let item = context(0, 0, 0, 15, 1);
    let before = wave.shared_state();
    assert_eq!(
        wave.trace_next(
            item,
            Ok::<_, &'static str>(candidates()),
            |_, _, _| Ok(SpotRoute::Generic(3)),
            |drawn| {
                let mut value = attempt(item, drawn, 3);
                value.active_rare_total = 15;
                Ok(value)
            },
        ),
        Err(TraceError::InconsistentAttemptInput)
    );
    assert_eq!(wave.shared_state(), before);
    assert_eq!(wave.next_queue_index(), 0);
}

#[test]
fn forced_goldie_late_counter_update_precedes_its_creation_plan() {
    let mut wave = stream();
    let mut item = context(0, 15, 0, 15, 1);
    item.periodic.global_cycle_flag = true;
    item.periodic.event_none_cycle = Some(1);
    item.forced_goldie_spot = Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
        id_fifteen_capacity_available: true,
    });
    let traced = wave
        .trace_next_with_counter(
            item,
            Err::<[RareCandidateState; 32], &'static str>("normal candidates irrelevant"),
            |id, _, _| {
                assert_eq!(id, 15);
                Ok(SpotRoute::HandledByCallback)
            },
            |_| Ok(Some(PostSpotOutcome::Allowed)),
            |drawn| Ok(attempt(item, drawn, 3)),
        )
        .unwrap();
    assert!(matches!(
        traced.due,
        PlannedDue::ForcedGoldie {
            plan: ocean_calc::spawn_attempts::GoldieAttemptPlan::Calls {
                goldie_create_call: true,
                ..
            },
            ..
        }
    ));
    assert_eq!(traced.counter.after, 1);
    assert_eq!(traced.counter.phase, CounterPhase::LatePostSpot);
}

#[test]
fn failed_spot_needs_no_virtual_gate_and_does_not_increment() {
    let mut wave = stream();
    let item = context(3, 0, 0, 15, 1);
    let traced = wave
        .trace_next_with_counter(
            item,
            Ok::<_, &'static str>(candidates()),
            |_, _, _| Ok(SpotRoute::Generic(0)),
            |_| Ok(Some(PostSpotOutcome::SpotFailed)),
            |_| panic!("failed spot never reaches creation input"),
        )
        .unwrap();
    assert_eq!(traced.counter.after, 3);
    assert!(matches!(
        traced.due,
        PlannedDue::Selected {
            plan: AttemptPlan::Skipped(AttemptBlock::SpotFailed),
            ..
        }
    ));
}

#[test]
fn request_count_block_has_no_spot_or_post_spot_input() {
    let mut wave = stream();
    let item = context(0, 0, 0, 15, 0);
    let traced = wave
        .trace_next_with_counter(
            item,
            Ok::<_, &'static str>(candidates()),
            |_, _, _| panic!("request gate stops before spot"),
            |_| panic!("no spot means no post-spot input"),
            |_| panic!("request gate stops before creation input"),
        )
        .unwrap();
    assert_eq!(traced.counter.after, 0);
    assert!(matches!(
        traced.due,
        PlannedDue::Selected {
            plan: AttemptPlan::Skipped(AttemptBlock::RequestCount),
            draw: DueDraw {
                spot: SpotDraw::BlockedByRequestCount,
                ..
            },
        }
    ));
}
