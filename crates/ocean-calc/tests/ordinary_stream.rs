use ocean_calc::{
    SeedWords,
    ordinary_stream::{
        EventZeroDueContext, EventZeroItemRuntime, ForcedGoldieSpotGate, OrdinaryStream,
        OrdinaryStreamError, SpotDraw, SpotRoute,
    },
    schedule::{EntryKind, ScheduleBranch, ScheduleInput},
    species::{
        PeriodicGoldieError, PeriodicGoldieInput, RareCandidateState, RareSpeciesSelection,
        normal_first_rare_candidates,
    },
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

fn forced_context(counter: i32, gate: Option<ForcedGoldieSpotGate>) -> EventZeroDueContext {
    let mut input = context();
    input.periodic.global_cycle_flag = true;
    input.periodic.accepted_rare_counter = counter;
    input.periodic.event_none_cycle = Some(3);
    input.forced_goldie_spot = gate;
    input
}

fn schedule() -> ScheduleInput {
    ScheduleInput {
        seed: SeedWords::from_scene([1, 2, 3, 4]),
        event_type: 0,
        wave_change_num: 3,
        rare_appear_num: 4,
        assumed_branch: ScheduleBranch::Special,
    }
}

fn all_eligible() -> [RareCandidateState; 32] {
    normal_first_rare_candidates(0x10100, [true; 11])
}

fn reference_next(words: &mut [u32; 4]) -> u32 {
    let t = words[0] ^ words[0].wrapping_shl(11);
    let value = words[3] ^ (words[3] >> 19) ^ t ^ (t >> 8);
    *words = [words[1], words[2], words[3], value];
    value
}

#[test]
fn director_initialization_discards_one_shared_draw_before_initial_directions() {
    // Independently derived from scene seed [1, 2, 3, 4] and domain 0x8d56161f.
    let mut words = [342001191, 3820670053, 1860123201, 3422436164];
    reference_next(&mut words); // director initialization at 0x375b11c..0x375b17c
    assert_eq!(((u64::from(reference_next(&mut words)) * 3) >> 32) as u8, 2);
    assert_eq!(((u64::from(reference_next(&mut words)) * 2) >> 32) as u8, 0);

    let stream = OrdinaryStream::new(schedule(), &[0, 1, 2]).unwrap();
    assert_eq!(stream.wave_start_direction(), Some(2));
    assert_eq!(stream.current_direction(), Some(0));
    assert_eq!(stream.shared_state().as_array(), words);
}

#[test]
fn ordinary_species_stream_accepts_fog_and_missile() {
    for event_type in [0, 5, 6] {
        let mut value = schedule();
        value.event_type = event_type;
        let stream = OrdinaryStream::new(value, &[0, 1, 2]).unwrap();
        assert_eq!(stream.configured_event(), event_type);
        assert!(!stream.entries().is_empty());
    }
}

#[test]
fn fog_runtime_mismatch_rolls_back_and_null_event_is_valid() {
    let mut input = schedule();
    input.event_type = 5;
    let mut stream = OrdinaryStream::new(input, &[0, 1, 2]).unwrap();
    assert_eq!(stream.advance_clock(0).unwrap().due_count, 2);
    let before = stream.shared_state();
    let mut item = context();
    item.periodic.event = Some(6);
    assert_eq!(
        stream.draw_next(item, all_eligible(), |_, _, _| panic!(
            "mismatch skips spot"
        )),
        Err(OrdinaryStreamError::UnexpectedRuntimeEvent(6))
    );
    assert_eq!(stream.shared_state(), before);
    assert_eq!(stream.next_queue_index(), 0);
    item.periodic.event = None;
    let draw = stream
        .draw_next(item, all_eligible(), |_, _, _| SpotRoute::HandledByCallback)
        .unwrap();
    assert!(matches!(draw.selection, RareSpeciesSelection::Normal(_)));
    assert_eq!(stream.next_queue_index(), 1);
}

#[test]
fn fog_forces_goldie_at_cycle_minus_one_without_normal_species_draw() {
    let mut input = schedule();
    input.event_type = 5;
    let mut stream = OrdinaryStream::new(input, &[0, 1, 2]).unwrap();
    stream.advance_clock(0).unwrap();
    let before = stream.shared_state();
    let mut item = context();
    item.periodic.event = Some(5);
    item.periodic.fog_cycle = Some(3);
    item.periodic.accepted_rare_counter = 2;
    item.forced_goldie_spot = Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
        id_fifteen_capacity_available: true,
    });
    let draw = stream
        .draw_next(item, [RareCandidateState::Skip; 32], |id, _, rng| {
            assert_eq!(id, 15);
            assert_eq!(rng, before, "forced species does not draw");
            SpotRoute::Generic(3)
        })
        .unwrap();
    assert!(matches!(
        draw.selection,
        RareSpeciesSelection::ForcedGoldie(_)
    ));
    assert!(matches!(
        draw.spot,
        SpotDraw::Generic { index: Some(_), .. }
    ));
    assert_ne!(draw.shared_state_after, before, "generic spot may draw");
}

#[test]
fn fog_missing_cycle_rolls_back_and_missile_never_forces() {
    let mut fog_input = schedule();
    fog_input.event_type = 5;
    let mut fog = OrdinaryStream::new(fog_input, &[0, 1, 2]).unwrap();
    fog.advance_clock(0).unwrap();
    let before = fog.shared_state();
    let mut fog_item = context();
    fog_item.periodic.event = Some(5);
    assert_eq!(
        fog.draw_next(fog_item, all_eligible(), |_, _, _| panic!("missing cycle")),
        Err(OrdinaryStreamError::Periodic(
            PeriodicGoldieError::MissingFogCycle
        ))
    );
    assert_eq!(fog.shared_state(), before);
    assert_eq!(fog.next_queue_index(), 0);

    let mut missile_input = schedule();
    missile_input.event_type = 6;
    let mut missile = OrdinaryStream::new(missile_input, &[0, 1, 2]).unwrap();
    missile.advance_clock(0).unwrap();
    let mut missile_item = context();
    missile_item.periodic.event = Some(6);
    missile_item.periodic.global_cycle_flag = true;
    missile_item.periodic.fog_cycle = Some(1);
    missile_item.periodic.event_none_cycle = Some(1);
    let draw = missile
        .draw_next(missile_item, all_eligible(), |id, _, _| {
            assert_ne!(id, 15);
            SpotRoute::HandledByCallback
        })
        .unwrap();
    assert!(matches!(draw.selection, RareSpeciesSelection::Normal(_)));
}

#[test]
fn scheduler_clock_then_each_due_item_interleaves_shared_draws() {
    let mut stream = OrdinaryStream::new(schedule(), &[0, 1, 2]).unwrap();
    assert_eq!(stream.wave_start_direction(), Some(2));
    assert_eq!(stream.current_direction(), Some(0));

    let clock = stream.advance_clock(0).unwrap();
    assert!(!clock.direction_branch_invoked);
    assert_eq!(clock.direction, Some(0));
    assert_eq!(clock.due_count, 2);
    let first = stream
        .draw_next(
            context(),
            all_eligible(),
            |species, direction, after_species| {
                assert_eq!(species, 6);
                assert_eq!(direction, Some(0));
                assert_eq!(
                    after_species.as_array(),
                    [2467946925, 3195445786, 3438216410, 3200453814]
                );
                SpotRoute::Generic(3)
            },
        )
        .unwrap();
    assert_eq!(first.queue_index, 0);
    assert_eq!(first.entry.kind, EntryKind::Extra);
    assert_eq!(
        first.spot,
        SpotDraw::Generic {
            candidate_count: 3,
            index: Some(2)
        }
    );
    let second = stream
        .draw_next(context(), all_eligible(), |species, _, _| {
            assert_eq!(species, 9);
            SpotRoute::Generic(3)
        })
        .unwrap();
    assert_eq!(second.entry.kind, EntryKind::Base);
    assert_eq!(
        second.spot,
        SpotDraw::Generic {
            candidate_count: 3,
            index: Some(2)
        }
    );
    assert_eq!(
        stream.shared_state().as_array(),
        [2809141545, 1593149731, 2580101425, 3380625175]
    );

    let clock = stream.advance_clock(2160).unwrap();
    assert!(clock.direction_branch_invoked);
    assert_eq!(clock.direction, Some(1));
    assert_eq!(clock.due_count, 1);
    let third = stream
        .draw_next(context(), all_eligible(), |species, direction, _| {
            assert_eq!(species, 14);
            assert_eq!(direction, Some(1));
            SpotRoute::Generic(3)
        })
        .unwrap();
    assert_eq!(
        third.spot,
        SpotDraw::Generic {
            candidate_count: 3,
            index: Some(2)
        }
    );
    assert_eq!(
        stream.shared_state().as_array(),
        [2191561723, 1094236554, 80920014, 2980710553]
    );
}

#[test]
fn later_same_tick_input_can_use_previous_result_and_no_species_skips_spot_callback() {
    let mut stream = OrdinaryStream::new(schedule(), &[0, 1, 2]).unwrap();
    assert_eq!(stream.advance_clock(0).unwrap().due_count, 2);
    let first = stream
        .draw_next(context(), all_eligible(), |_, _, _| SpotRoute::Generic(0))
        .unwrap();
    assert_eq!(first.species_id, 6);
    assert_eq!(
        first.spot,
        SpotDraw::Generic {
            candidate_count: 0,
            index: None
        }
    );
    let later_states = if (4..=14).contains(&first.species_id) {
        [RareCandidateState::Skip; 32]
    } else {
        all_eligible()
    };
    let second = stream
        .draw_next(context(), later_states, |_, _, _| {
            panic!("no species has no spot callback")
        })
        .unwrap();
    assert_eq!(second.species_id, 0x20);
    assert_eq!(second.spot, SpotDraw::NoSpecies);
    assert_eq!(
        stream.shared_state().as_array(),
        [2467946925, 3195445786, 3438216410, 3200453814]
    );
}

#[test]
fn handled_spot_omits_generic_draw_and_changes_the_next_species() {
    let mut stream = OrdinaryStream::new(schedule(), &[0, 1, 2]).unwrap();
    let mut generic = OrdinaryStream::new(schedule(), &[0, 1, 2]).unwrap();
    stream.advance_clock(0).unwrap();
    generic.advance_clock(0).unwrap();
    let first = stream
        .draw_next(context(), all_eligible(), |_, _, _| {
            SpotRoute::HandledByCallback
        })
        .unwrap();
    assert_eq!(first.spot, SpotDraw::HandledByCallback);
    generic
        .draw_next(context(), all_eligible(), |_, _, _| SpotRoute::Generic(3))
        .unwrap();
    let second = stream
        .draw_next(context(), all_eligible(), |_, _, _| SpotRoute::Generic(3))
        .unwrap();
    let generic_second = generic
        .draw_next(context(), all_eligible(), |_, _, _| SpotRoute::Generic(3))
        .unwrap();
    assert_eq!(second.species_id, 10);
    assert_eq!(generic_second.species_id, 9);
    assert_ne!(second.species_id, generic_second.species_id);
}

#[test]
fn rejected_operations_keep_cursor_direction_and_rng() {
    let mut stream = OrdinaryStream::new(schedule(), &[0, 1, 2]).unwrap();
    let before = stream.shared_state();
    assert_eq!(
        stream.advance_clock(-1),
        Err(OrdinaryStreamError::NegativeElapsedTick(-1))
    );
    assert_eq!(
        stream.draw_next(context(), all_eligible(), |_, _, _| SpotRoute::Generic(3)),
        Err(OrdinaryStreamError::NoDueItem)
    );
    assert_eq!(stream.shared_state(), before);
    assert_eq!(stream.next_queue_index(), 0);
    assert_eq!(stream.current_direction(), Some(0));
    assert_eq!(stream.advance_clock(0).unwrap().due_count, 2);
    assert_eq!(
        stream.advance_clock(2160),
        Err(OrdinaryStreamError::PendingDueItems(2))
    );
    let after_clock = stream.shared_state();
    let mut bad_states = all_eligible();
    bad_states[3] = RareCandidateState::Eligible;
    assert_eq!(
        stream.draw_next(context(), bad_states, |_, _, _| SpotRoute::Generic(3)),
        Err(OrdinaryStreamError::InvalidCandidateId(3))
    );
    assert_eq!(stream.shared_state(), after_clock);
    assert_eq!(stream.next_queue_index(), 0);
}

#[test]
fn delayed_single_clock_call_updates_direction_once_before_all_due_items() {
    let mut stream = OrdinaryStream::new(schedule(), &[0, 1, 2]).unwrap();
    // A caller-supplied tick of 4320 is conditional; live WAVE end ordering
    // has not been verified.
    let clock = stream.advance_clock(4320).unwrap();
    assert!(clock.direction_branch_invoked);
    assert_eq!(clock.direction, Some(2));
    assert_eq!(clock.due_count, 4);
    let after_one_direction = [3417165730, 4265732950, 1776012269, 1405483401];
    assert_eq!(
        clock.shared_state_after_direction.as_array(),
        after_one_direction
    );
    for index in 0..4 {
        let draw = stream
            .draw_next(context(), [RareCandidateState::Skip; 32], |_, _, _| {
                panic!("unspecified species skips spot callback")
            })
            .unwrap();
        assert_eq!(draw.queue_index, index);
        assert_eq!(draw.direction, Some(2));
        assert_eq!(draw.species_id, 0x20);
        assert_eq!(draw.shared_state_after.as_array(), after_one_direction);
    }
    assert_eq!(stream.remaining_due_count(), 0);
}

#[test]
fn panicking_spot_callback_does_not_commit_a_partial_shared_rng_step() {
    let mut stream = OrdinaryStream::new(schedule(), &[0, 1, 2]).unwrap();
    stream.advance_clock(0).unwrap();
    let before = stream.shared_state();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = stream.draw_next(context(), all_eligible(), |_, _, _| {
            panic!("callback failed")
        });
    }));
    assert!(result.is_err());
    assert_eq!(stream.shared_state(), before);
    assert_eq!(stream.next_queue_index(), 0);
    assert_eq!(stream.remaining_due_count(), 2);
}

#[test]
fn forced_goldie_skips_normal_species_draw_but_keeps_its_spot_draw() {
    let mut stream = OrdinaryStream::new(schedule(), &[0, 1, 2]).unwrap();
    stream.advance_clock(0).unwrap();
    let before = stream.shared_state();
    let mut independent = before.as_array();
    let expected_spot = ((u64::from(reference_next(&mut independent)) * 3) >> 32) as u32;

    let draw = stream
        .draw_next(
            forced_context(
                2,
                Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
                    id_fifteen_capacity_available: true,
                }),
            ),
            all_eligible(),
            |species, direction, after_species| {
                assert_eq!(species, 15);
                assert_eq!(direction, Some(0));
                assert_eq!(after_species, before);
                SpotRoute::Generic(3)
            },
        )
        .unwrap();
    assert!(matches!(
        draw.selection,
        RareSpeciesSelection::ForcedGoldie(_)
    ));
    assert_eq!(
        draw.spot,
        SpotDraw::Generic {
            candidate_count: 3,
            index: Some(expected_spot)
        }
    );
    assert_eq!(draw.shared_state_after.as_array(), independent);
    assert_eq!(stream.next_queue_index(), 1);
}

#[test]
fn blocked_forced_goldie_returns_before_spot_without_consuming_shared_rng() {
    let mut stream = OrdinaryStream::new(schedule(), &[0, 1, 2]).unwrap();
    stream.advance_clock(0).unwrap();
    let before = stream.shared_state();
    let draw = stream
        .draw_next(
            forced_context(
                2,
                Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
                    id_fifteen_capacity_available: false,
                }),
            ),
            all_eligible(),
            |_, _, _| panic!("capacity-blocked ID15 never reaches spot callback"),
        )
        .unwrap();
    assert_eq!(draw.species_id, 15);
    assert_eq!(draw.spot, SpotDraw::BlockedBeforeSpot);
    assert_eq!(stream.shared_state(), before);
    assert_eq!(stream.next_queue_index(), 1);
}

#[test]
fn forced_goldie_over_total_limit_still_draws_spot() {
    let mut stream = OrdinaryStream::new(schedule(), &[0, 1, 2]).unwrap();
    stream.advance_clock(0).unwrap();
    let mut item = forced_context(2, Some(ForcedGoldieSpotGate::OverTotalLimit));
    item.runtime.active_total = 16;
    let draw = stream
        .draw_next(item, [RareCandidateState::Skip; 32], |id, _, _| {
            assert_eq!(id, 15);
            SpotRoute::Generic(1)
        })
        .unwrap();
    assert_eq!(
        draw.spot,
        SpotDraw::Generic {
            candidate_count: 1,
            index: Some(0)
        }
    );
}

#[test]
fn periodic_errors_leave_the_due_item_and_rng_untouched() {
    let mut stream = OrdinaryStream::new(schedule(), &[0, 1, 2]).unwrap();
    stream.advance_clock(0).unwrap();
    let before = stream.shared_state();
    let no_gate = forced_context(2, None);
    assert_eq!(
        stream.draw_next(no_gate, all_eligible(), |_, _, _| panic!("missing gate")),
        Err(OrdinaryStreamError::MissingForcedGoldieSpotGate)
    );
    let mut no_cycle = no_gate;
    no_cycle.periodic.event_none_cycle = None;
    assert_eq!(
        stream.draw_next(no_cycle, all_eligible(), |_, _, _| panic!("missing cycle")),
        Err(OrdinaryStreamError::Periodic(
            PeriodicGoldieError::MissingEventNoneCycle
        ))
    );
    let mut wrong_event = context();
    wrong_event.periodic.event = Some(5);
    assert_eq!(
        stream.draw_next(wrong_event, all_eligible(), |_, _, _| panic!("wrong event")),
        Err(OrdinaryStreamError::UnexpectedRuntimeEvent(5))
    );
    assert_eq!(stream.shared_state(), before);
    assert_eq!(stream.next_queue_index(), 0);

    let valid = stream
        .draw_next(context(), all_eligible(), |_, _, _| SpotRoute::Generic(3))
        .unwrap();
    assert_eq!(valid.species_id, 6);
    assert_eq!(valid.queue_index, 0);
}

#[test]
fn null_event_pointer_needs_no_cycle_and_uses_normal_species_scan() {
    let mut stream = OrdinaryStream::new(schedule(), &[0, 1, 2]).unwrap();
    stream.advance_clock(0).unwrap();
    let mut input = context();
    input.periodic.event = None;
    input.periodic.global_cycle_flag = true;
    let draw = stream
        .draw_next(input, all_eligible(), |_, _, _| {
            SpotRoute::HandledByCallback
        })
        .unwrap();
    assert_eq!(draw.selection, RareSpeciesSelection::Normal(6));
}

#[test]
fn next_item_can_see_an_early_counter_increment_from_previous_item() {
    let mut stream = OrdinaryStream::new(schedule(), &[0, 1, 2]).unwrap();
    assert_eq!(stream.advance_clock(0).unwrap().due_count, 2);
    let first = stream
        .draw_next(forced_context(1, None), all_eligible(), |_, _, _| {
            SpotRoute::HandledByCallback
        })
        .unwrap();
    assert_eq!(first.selection, RareSpeciesSelection::Normal(6));
    // The caller models an early +0x4d4 increment after the first item's
    // periodic check and supplies the new value for the next same-call item.
    let second = stream
        .draw_next(
            forced_context(
                2,
                Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
                    id_fifteen_capacity_available: false,
                }),
            ),
            all_eligible(),
            |_, _, _| panic!("second item is capacity blocked before spot"),
        )
        .unwrap();
    assert_eq!(second.species_id, 15);
    assert_eq!(second.spot, SpotDraw::BlockedBeforeSpot);
}
