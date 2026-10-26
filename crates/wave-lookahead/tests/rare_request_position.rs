use salmon_wave_lookahead::{
    SeedWords,
    event_zero_counter::{CounterAdvance, CounterPhase, PostSpotOutcome},
    ordinary_stream::{
        CounterTracedDue, DueDraw, EventZeroDueContext, EventZeroItemRuntime, OrdinaryStream,
        PlannedDue, SpotDraw, SpotRoute,
    },
    placement_allocation::{OrdinaryPlacementInput, PlacementChannelSource},
    rare_request_position::{
        EvidenceBasis, PlacementObservation, PrimaryRequestOutcome, ProjectionError,
        RuntimeGenericSpot, project_primary_rare_request,
    },
    rng::XorShift128,
    schedule::{EntryKind, NominalEntry, ScheduleBranch, ScheduleInput},
    spawn_attempts::{AttemptBlock, AttemptInput, AttemptPlan},
    spawn_position::generic_rare_event_position,
    species::{
        PeriodicGoldieInput, RareCandidateState, RareSpeciesSelection, normal_first_rare_candidates,
    },
    stage_data::stage_spawns,
};

fn draw(species_id: u8, spot: SpotDraw, requested_count: i32) -> DueDraw {
    DueDraw {
        queue_index: 0,
        entry: NominalEntry {
            tick: 0,
            kind: EntryKind::Base,
        },
        direction: Some(0),
        selection: RareSpeciesSelection::Normal(species_id),
        species_id,
        requested_count,
        spot,
        shared_state_after: XorShift128::from_words([1, 2, 3, 4]),
    }
}

fn traced(
    species_id: u8,
    spot: SpotDraw,
    requested_count: i32,
    plan: AttemptPlan,
) -> CounterTracedDue {
    CounterTracedDue {
        due: PlannedDue::Selected {
            draw: draw(species_id, spot, requested_count),
            plan,
        },
        counter: CounterAdvance {
            before: 0,
            after: 1,
            phase: CounterPhase::LatePostSpot,
        },
    }
}

fn spot(count: u32, index: u32, origin: [f32; 3], direction: [f32; 3]) -> RuntimeGenericSpot {
    RuntimeGenericSpot {
        candidate_count: count,
        selected_index: index,
        origin,
        direction,
        basis: EvidenceBasis::ObservedRuntime,
    }
}

fn allocation(rare_id: u8, requested_count: i32) -> PlacementObservation {
    PlacementObservation {
        input: OrdinaryPlacementInput {
            rare_id,
            requested_count,
            primary_cap: 3,
            primary_active_before_create: 0,
            follower_cap: 5,
            follower_active_before_create: 1,
            follower_budget: 3,
            channel_source: PlacementChannelSource::OwnedRomfs1130,
        },
        basis: EvidenceBasis::AssumedIdealized,
    }
}

#[test]
fn stateful_stream_to_generic_primary_request_keeps_gate_and_spot_order() {
    let mut stream = OrdinaryStream::new(
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
    assert_eq!(stream.advance_clock(0).unwrap().due_count, 2);
    let context = EventZeroDueContext {
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
    };
    let due = stream
        .trace_next_with_counter(
            context,
            Ok::<[RareCandidateState; 32], ()>(normal_first_rare_candidates(0x10100, [true; 11])),
            |id, _, _| {
                assert_eq!(id, 6);
                Ok(SpotRoute::Generic(3))
            },
            |_| Ok(Some(PostSpotOutcome::Allowed)),
            |draw| {
                Ok(AttemptInput {
                    requested_count: 3,
                    selected_rare_id: draw.species_id,
                    active_rare_total: 0,
                    active_total: 0,
                    item_total_limit: 15,
                    spot_succeeded: true,
                    post_spot_allows: true,
                    selected_rare_cap: 3,
                    selected_rare_active: 0,
                    follower_cap: 5,
                    follower_active_after_rare: 2,
                    follower_budget: 0,
                    scene_mode: 0,
                })
            },
        )
        .unwrap();
    let PlannedDue::Selected { draw, .. } = due.due else {
        panic!("normal Rare expected")
    };
    assert_eq!(
        draw.spot,
        SpotDraw::Generic {
            candidate_count: 3,
            index: Some(2)
        }
    );
    assert_eq!(draw.requested_count, 3);
    let output = project_primary_rare_request(
        due,
        Some(spot(3, 2, [100.0, 200.0, 300.0], [1.0, 0.0, 0.0])),
        Some(allocation(6, 3)),
    )
    .unwrap();
    let PrimaryRequestOutcome::Conditional(value) = output else {
        panic!("request expected")
    };
    assert_eq!(
        (value.species_id, value.spot_index, value.channel),
        (6, 2, 0)
    );
    assert_eq!(value.position, [100.0, 200.0, 300.0]);
    assert_eq!(value.spot_basis, EvidenceBasis::ObservedRuntime);
    assert_eq!(value.allocation_basis, EvidenceBasis::AssumedIdealized);
}

#[test]
fn skipped_and_nonprimary_branches_do_not_require_position_inputs() {
    let generic = SpotDraw::Generic {
        candidate_count: 1,
        index: Some(0),
    };
    let mut blocked = traced(
        7,
        generic,
        3,
        AttemptPlan::Skipped(AttemptBlock::RareGlobalLimit),
    );
    blocked.counter.phase = CounterPhase::EarlyRareLimit;
    assert_eq!(
        project_primary_rare_request(blocked, None, None),
        Ok(PrimaryRequestOutcome::Skipped(
            AttemptBlock::RareGlobalLimit
        ))
    );

    let no_primary = traced(
        7,
        generic,
        3,
        AttemptPlan::Calls {
            effective_budget: 3,
            rare_create_call: false,
            follower_id: 0,
            follower_create_calls: 2,
        },
    );
    assert_eq!(
        project_primary_rare_request(no_primary, None, None),
        Ok(PrimaryRequestOutcome::NoPrimaryCall)
    );

    let callback = traced(
        8,
        SpotDraw::HandledByCallback,
        3,
        AttemptPlan::Calls {
            effective_budget: 3,
            rare_create_call: true,
            follower_id: 0,
            follower_create_calls: 0,
        },
    );
    assert_eq!(
        project_primary_rare_request(callback, None, None),
        Ok(PrimaryRequestOutcome::NeedsCallbackPosition)
    );

    let absent = CounterTracedDue {
        due: PlannedDue::NoSpecies(draw(0x20, SpotDraw::NoSpecies, 3)),
        counter: CounterAdvance {
            before: 0,
            after: 0,
            phase: CounterPhase::Unchanged,
        },
    };
    assert_eq!(
        project_primary_rare_request(absent, None, None),
        Ok(PrimaryRequestOutcome::NoSpecies)
    );
}

#[test]
fn count_index_and_counter_inconsistencies_are_errors() {
    let plan = AttemptPlan::Calls {
        effective_budget: 3,
        rare_create_call: true,
        follower_id: 0,
        follower_create_calls: 0,
    };
    let invalid_index = traced(
        7,
        SpotDraw::Generic {
            candidate_count: 2,
            index: Some(2),
        },
        3,
        plan,
    );
    assert_eq!(
        project_primary_rare_request(invalid_index, None, None),
        Err(ProjectionError::InconsistentTrace)
    );

    let missing_spot = traced(
        7,
        SpotDraw::Generic {
            candidate_count: 0,
            index: None,
        },
        3,
        plan,
    );
    assert_eq!(
        project_primary_rare_request(missing_spot, None, None),
        Err(ProjectionError::InconsistentTrace)
    );

    let valid = traced(
        7,
        SpotDraw::Generic {
            candidate_count: 2,
            index: Some(1),
        },
        3,
        plan,
    );
    assert_eq!(
        project_primary_rare_request(valid, None, None),
        Err(ProjectionError::MissingRuntimeSpot)
    );
    assert_eq!(
        project_primary_rare_request(valid, Some(spot(3, 1, [0.0; 3], [1.0, 0.0, 0.0])), None),
        Err(ProjectionError::RuntimeSpotMismatch)
    );
    assert_eq!(
        project_primary_rare_request(valid, Some(spot(2, 0, [0.0; 3], [1.0, 0.0, 0.0])), None),
        Err(ProjectionError::RuntimeSpotMismatch)
    );

    let mut wrong_phase = valid;
    wrong_phase.counter.phase = CounterPhase::Unchanged;
    assert_eq!(
        project_primary_rare_request(wrong_phase, None, None),
        Err(ProjectionError::InconsistentTrace)
    );

    let no_primary = AttemptPlan::Calls {
        effective_budget: 3,
        rare_create_call: false,
        follower_id: 0,
        follower_create_calls: 0,
    };
    for impossible_spot in [
        SpotDraw::Generic {
            candidate_count: 0,
            index: None,
        },
        SpotDraw::BlockedByRequestCount,
    ] {
        assert_eq!(
            project_primary_rare_request(traced(7, impossible_spot, 3, no_primary), None, None),
            Err(ProjectionError::InconsistentTrace)
        );
    }
    for (spot_draw, count, reason) in [
        (
            SpotDraw::Generic {
                candidate_count: 1,
                index: Some(0),
            },
            3,
            AttemptBlock::GoldiePreSpotCapacity,
        ),
        (
            SpotDraw::Generic {
                candidate_count: 1,
                index: Some(0),
            },
            0,
            AttemptBlock::RequestCount,
        ),
    ] {
        assert_eq!(
            project_primary_rare_request(
                traced(7, spot_draw, count, AttemptPlan::Skipped(reason)),
                None,
                None
            ),
            Err(ProjectionError::InconsistentTrace)
        );
    }
    let mut wrong_counter = valid;
    wrong_counter.counter.after = 2;
    assert_eq!(
        project_primary_rare_request(wrong_counter, None, None),
        Err(ProjectionError::InconsistentTrace)
    );
}

#[test]
fn original_request_count_and_primary_reservation_are_checked() {
    let due = traced(
        11,
        SpotDraw::Generic {
            candidate_count: 1,
            index: Some(0),
        },
        3,
        AttemptPlan::Calls {
            effective_budget: 1,
            rare_create_call: true,
            follower_id: 0,
            follower_create_calls: 0,
        },
    );
    let runtime_spot = spot(1, 0, [10.0; 3], [1.0, 0.0, 0.0]);
    assert_eq!(
        project_primary_rare_request(due, Some(runtime_spot), Some(allocation(11, 2))),
        Err(ProjectionError::PlacementRequestCountMismatch)
    );
    let mut no_reservation = allocation(11, 3);
    no_reservation.input.primary_active_before_create = 3;
    assert_eq!(
        project_primary_rare_request(due, Some(runtime_spot), Some(no_reservation)),
        Err(ProjectionError::PrimaryReservationMismatch)
    );
}

#[test]
fn channel_two_uses_precreate_allocation_even_when_later_follower_plan_differs() {
    let due = traced(
        4,
        SpotDraw::Generic {
            candidate_count: 1,
            index: Some(0),
        },
        3,
        AttemptPlan::Calls {
            effective_budget: 3,
            rare_create_call: true,
            follower_id: 0,
            follower_create_calls: 0,
        },
    );
    let output = project_primary_rare_request(
        due,
        Some(spot(1, 0, [100.0, 200.0, 300.0], [1.0, 0.0, 0.0])),
        Some(allocation(4, 3)),
    )
    .unwrap();
    let PrimaryRequestOutcome::Conditional(value) = output else {
        panic!("request expected")
    };
    assert_eq!(value.channel, 2);
    assert_eq!(value.position, [97.5, 200.0, 300.0]);
    assert_eq!(value.allocation.follower_reserved, 2);
}

#[test]
fn observed_shakedent_high_tide_generic_sources_are_ordinal_zero_channel_zero_origins() {
    let stage = stage_spawns("Cop_Shakedent").unwrap();
    for (direction, actor_index) in [(0, 56), (1, 57), (2, 58)] {
        let group = stage.group(2, direction).unwrap();
        assert_eq!(group.len(), 1);
        assert_eq!(group[0].actor_index, actor_index);
        let origin = group[0].translate;
        assert_eq!(
            generic_rare_event_position(origin, [1.0, 0.0, 0.0], 0, [1, 0], 0),
            Ok(origin)
        );
    }
}
