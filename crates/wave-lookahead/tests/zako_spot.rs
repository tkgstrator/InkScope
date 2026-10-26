use salmon_wave_lookahead::direction::predict_initial_directions;
use salmon_wave_lookahead::rng::SeedWords;
use salmon_wave_lookahead::stage_data::SpawnActor;
use salmon_wave_lookahead::zako_spot::{
    ModeZeroZakoSpotInput, RuntimeZakoSpot, ZakoSpotError, ZakoSpotOutcome, ZakoSpotSource,
    advance_zako_selector_inline_draws, predict_mode_zero_zako_spot, stock_main_banc_spot,
};
use salmon_wave_lookahead::zako_timer::ZakoTimerStep;

const A: RuntimeZakoSpot = RuntimeZakoSpot {
    origin: [1.0, 2.0, 3.0],
    forward: [0.0, 0.0, 1.0],
};
const B: RuntimeZakoSpot = RuntimeZakoSpot {
    origin: [4.0, 5.0, 6.0],
    forward: [0.0, 1.0, 0.0],
};
const C: RuntimeZakoSpot = RuntimeZakoSpot {
    origin: [7.0, 8.0, 9.0],
    forward: [1.0, 0.0, 0.0],
};
const D: RuntimeZakoSpot = RuntimeZakoSpot {
    origin: [10.0, 11.0, 12.0],
    forward: [-1.0, 0.0, 0.0],
};
const INITIAL_RNG: [u32; 4] = [1, 2, 3, 0xf000_0000];
const AFTER_ONE_DRAW: [u32; 4] = [2, 3, 0xf000_0000, 0xf000_1609];

fn input<'a>(
    groups: [Option<&'a [Option<RuntimeZakoSpot>]>; 9],
    tree_30: &'a [Option<RuntimeZakoSpot>],
    tree_50: &'a [Option<RuntimeZakoSpot>],
) -> ModeZeroZakoSpotInput<'a> {
    ModeZeroZakoSpotInput {
        timer_step: ZakoTimerStep {
            timer_after: 8,
            timer_reset: true,
            spawn_path_entered: true,
        },
        species_selection_succeeded: true,
        default_callback_route_confirmed: true,
        applied_event: 0,
        applied_mode: 0,
        direction: 0,
        tide: 0,
        rng_after_species: INITIAL_RNG,
        runtime_groups: groups,
        tree_30,
        tree_50,
        scratch_capacity: 4,
    }
}

#[test]
fn concatenates_group_then_both_trees_and_copies_selected_transform() {
    let group = [Some(A), Some(B)];
    let tree_30 = [Some(C)];
    let tree_50 = [Some(D)];
    let mut groups: [Option<&[Option<RuntimeZakoSpot>]>; 9] = [None; 9];
    groups[0] = Some(&group);
    let result = predict_mode_zero_zako_spot(input(groups, &tree_30, &tree_50)).unwrap();
    assert_eq!(result.group_key, Some(0));
    assert_eq!(result.candidate_count, 4);
    assert_eq!(result.rng_after_spot, AFTER_ONE_DRAW);
    assert_eq!(
        result.outcome,
        ZakoSpotOutcome::SelectedPoint {
            combined_index: 3,
            source: ZakoSpotSource::Tree50 { source_index: 0 },
            spot: D,
        }
    );

    let mut limited = input(groups, &tree_30, &tree_50);
    limited.scratch_capacity = 3;
    let result = predict_mode_zero_zako_spot(limited).unwrap();
    assert_eq!(result.candidate_count, 3);
    assert_eq!(result.rng_after_spot, AFTER_ONE_DRAW);
    assert_eq!(
        result.outcome,
        ZakoSpotOutcome::SelectedPoint {
            combined_index: 2,
            source: ZakoSpotSource::Tree30 { source_index: 0 },
            spot: C,
        }
    );
}

#[test]
fn uses_the_live_direction_and_tide_group_key() {
    let group = [Some(A)];
    let mut groups: [Option<&[Option<RuntimeZakoSpot>]>; 9] = [None; 9];
    groups[7] = Some(&group); // tide 1 + 3 * direction 2
    let mut call = input(groups, &[], &[]);
    call.direction = 2;
    call.tide = 1;
    let result = predict_mode_zero_zako_spot(call).unwrap();
    assert_eq!(result.group_key, Some(7));
    assert_eq!(
        result.outcome,
        ZakoSpotOutcome::SelectedPoint {
            combined_index: 0,
            source: ZakoSpotSource::Group { source_index: 0 },
            spot: A,
        }
    );
    assert_eq!(result.rng_after_spot, AFTER_ONE_DRAW);
}

#[test]
fn absent_group_does_not_fall_through_to_tree_but_present_empty_group_does() {
    let tree = [Some(C)];
    let missing = predict_mode_zero_zako_spot(input([None; 9], &tree, &[])).unwrap();
    assert_eq!(missing.outcome, ZakoSpotOutcome::MissingGroup);
    assert_eq!(missing.candidate_count, 0);
    assert_eq!(missing.rng_after_spot, INITIAL_RNG);

    let mut groups: [Option<&[Option<RuntimeZakoSpot>]>; 9] = [None; 9];
    groups[0] = Some(&[]);
    let present = predict_mode_zero_zako_spot(input(groups, &tree, &[])).unwrap();
    assert_eq!(present.candidate_count, 1);
    assert_eq!(present.rng_after_spot, AFTER_ONE_DRAW);
    assert_eq!(
        present.outcome,
        ZakoSpotOutcome::SelectedPoint {
            combined_index: 0,
            source: ZakoSpotSource::Tree30 { source_index: 0 },
            spot: C,
        }
    );
}

#[test]
fn empty_or_nonpositive_capacity_consumes_no_spot_draw() {
    let group = [Some(A)];
    let mut groups: [Option<&[Option<RuntimeZakoSpot>]>; 9] = [None; 9];
    groups[0] = Some(&group);
    for capacity in [0, -1] {
        let mut call = input(groups, &[], &[]);
        call.scratch_capacity = capacity;
        let result = predict_mode_zero_zako_spot(call).unwrap();
        assert_eq!(result.outcome, ZakoSpotOutcome::NoCandidates);
        assert_eq!(result.rng_after_spot, INITIAL_RNG);
    }
    groups[0] = Some(&[]);
    let result = predict_mode_zero_zako_spot(input(groups, &[], &[])).unwrap();
    assert_eq!(result.outcome, ZakoSpotOutcome::NoCandidates);
    assert_eq!(result.rng_after_spot, INITIAL_RNG);
}

#[test]
fn selected_null_slot_is_a_failure_after_the_one_draw() {
    let group = [Some(A), Some(B), Some(C), None];
    let mut groups: [Option<&[Option<RuntimeZakoSpot>]>; 9] = [None; 9];
    groups[0] = Some(&group);
    let result = predict_mode_zero_zako_spot(input(groups, &[], &[])).unwrap();
    assert_eq!(result.rng_after_spot, AFTER_ONE_DRAW);
    assert_eq!(
        result.outcome,
        ZakoSpotOutcome::SelectedNull {
            combined_index: 3,
            source: ZakoSpotSource::Group { source_index: 3 },
        }
    );
}

#[test]
fn timer_and_species_veto_before_validating_spot_inputs() {
    let mut blocked = input([None; 9], &[], &[]);
    blocked.timer_step.spawn_path_entered = false;
    blocked.applied_event = 7;
    blocked.applied_mode = 5;
    blocked.direction = -4;
    blocked.tide = 9;
    blocked.default_callback_route_confirmed = false;
    assert_eq!(
        predict_mode_zero_zako_spot(blocked).unwrap().outcome,
        ZakoSpotOutcome::TimerBlocked
    );
    assert_eq!(
        predict_mode_zero_zako_spot(blocked).unwrap().rng_after_spot,
        INITIAL_RNG
    );

    let mut rejected = blocked;
    rejected.timer_step.spawn_path_entered = true;
    rejected.species_selection_succeeded = false;
    assert_eq!(
        predict_mode_zero_zako_spot(rejected).unwrap().outcome,
        ZakoSpotOutcome::SpeciesSelectionFailed
    );
    assert_eq!(
        predict_mode_zero_zako_spot(rejected)
            .unwrap()
            .rng_after_spot,
        INITIAL_RNG
    );
}

#[test]
fn unsupported_event_mode_direction_tide_or_callback_are_errors_without_draws() {
    for event in [2, 7] {
        let mut call = input([None; 9], &[], &[]);
        call.applied_event = event;
        assert_eq!(
            predict_mode_zero_zako_spot(call),
            Err(ZakoSpotError::UnsupportedEvent(event))
        );
    }
    let mut call = input([None; 9], &[], &[]);
    call.default_callback_route_confirmed = false;
    assert_eq!(
        predict_mode_zero_zako_spot(call),
        Err(ZakoSpotError::UnconfirmedDefaultCallbackRoute)
    );
    let mut call = input([None; 9], &[], &[]);
    call.applied_mode = 2;
    assert_eq!(
        predict_mode_zero_zako_spot(call),
        Err(ZakoSpotError::UnsupportedMode(2))
    );
    let mut call = input([None; 9], &[], &[]);
    call.direction = 3;
    assert_eq!(
        predict_mode_zero_zako_spot(call),
        Err(ZakoSpotError::InvalidDirection(3))
    );
    let mut call = input([None; 9], &[], &[]);
    call.tide = -1;
    assert_eq!(
        predict_mode_zero_zako_spot(call),
        Err(ZakoSpotError::InvalidTide(-1))
    );
}

#[test]
fn tick_zero_uses_queue_ready_direction_not_wave_start_direction() {
    let directions =
        predict_initial_directions(SeedWords::from_scene([1, 2, 3, 4]), &[0, 1, 2]).unwrap();
    assert_eq!(directions.wave_start, Some(2));
    assert_eq!(directions.after_queue_build, Some(0));
    let group = [Some(A)];
    let mut groups: [Option<&[Option<RuntimeZakoSpot>]>; 9] = [None; 9];
    groups[0] = Some(&group);
    let mut call = input(groups, &[], &[]);
    call.direction = i32::from(directions.after_queue_build.unwrap());
    let result = predict_mode_zero_zako_spot(call).unwrap();
    assert_eq!(result.group_key, Some(0));
    assert!(matches!(
        result.outcome,
        ZakoSpotOutcome::SelectedPoint { .. }
    ));
}

#[test]
fn selector_inline_draws_depend_on_eligible_visits_not_selected_species() {
    let initial =
        predict_initial_directions(SeedWords::from_scene([1, 2, 3, 4]), &[0, 1, 2]).unwrap();
    let before = initial.director_rng_38c.as_array();
    assert_eq!(before, [0xf917_1386, 0x442f_e07b, 0x215a_da25, 0x5a98_dd9d]);
    let expected = [
        before,
        [0x442f_e07b, 0x215a_da25, 0x5a98_dd9d, 0x1b52_7e6b],
        [0x215a_da25, 0x5a98_dd9d, 0x1b52_7e6b, 0x2045_6942],
        [0x5a98_dd9d, 0x1b52_7e6b, 0x2045_6942, 0xd739_149d],
        [0x1b52_7e6b, 0x2045_6942, 0xd739_149d, 0x4bd1_4fd2],
    ];
    for (visits, expected_state) in expected.into_iter().enumerate() {
        assert_eq!(
            advance_zako_selector_inline_draws(before, visits as u32),
            expected_state,
            "eligible visits {visits}"
        );
    }
}

#[test]
fn first_rush_spot_composes_seed_and_inline_draws_only_under_explicit_assumptions() {
    let initial =
        predict_initial_directions(SeedWords::from_scene([1, 2, 3, 4]), &[0, 1, 2]).unwrap();
    assert_eq!(initial.after_queue_build, Some(0));
    let points = [Some(A), Some(B), Some(C), Some(D)];
    let mut groups: [Option<&[Option<RuntimeZakoSpot>]>; 9] = [None; 9];
    groups[0] = Some(&points);
    let mut call = input(groups, &[], &[]);
    call.applied_event = 1;
    call.direction = i32::from(initial.after_queue_build.unwrap());
    // Conditional fixture: the queue builder ran, the first scheduler tick is
    // zero, and no earlier or indirect callback advanced director +0x38c.
    // The caller established three eligible iterator visits, selector success,
    // and the stock default spot callback route without predicting species.
    call.rng_after_species =
        advance_zako_selector_inline_draws(initial.director_rng_38c.as_array(), 3);
    let result = predict_mode_zero_zako_spot(call).unwrap();
    assert_eq!(result.group_key, Some(0));
    assert_eq!(result.candidate_count, 4);
    assert_eq!(
        result.rng_after_spot,
        [0x1b52_7e6b, 0x2045_6942, 0xd739_149d, 0x4bd1_4fd2]
    );
    assert_eq!(
        result.outcome,
        ZakoSpotOutcome::SelectedPoint {
            combined_index: 1,
            source: ZakoSpotSource::Group { source_index: 1 },
            spot: B,
        }
    );
}

#[test]
fn stock_main_banc_spot_handles_omitted_and_mixed_rotations() {
    let actor = SpawnActor {
        actor_index: 0,
        hash: 0,
        translate: [3.0, -4.0, 5.0],
        rotate: None,
    };
    let check = |rotate: Option<[f32; 3]>, expected: [f32; 3]| {
        let spot = stock_main_banc_spot(&SpawnActor { rotate, ..actor });
        assert_eq!(spot.origin, actor.translate);
        for (axis, (&actual, &expected)) in spot.forward.iter().zip(expected.iter()).enumerate() {
            assert!(
                (actual - expected).abs() < 0.000002,
                "axis {axis}: actual {actual} expected {expected}"
            );
        }
    };
    check(None, [0.0, 0.0, 1.0]);
    check(Some([0.0; 3]), [0.0, 0.0, 1.0]);
    let half_pi = std::f32::consts::FRAC_PI_2;
    check(Some([0.0, half_pi, 0.0]), [1.0, 0.0, 0.0]);
    check(Some([half_pi, 0.0, 0.0]), [0.0, -1.0, 0.0]);
    let pi = std::f32::consts::PI;
    check(
        Some([pi / 6.0, pi / 4.0, pi / 3.0]),
        [0.739_198_9, 0.280_330_1, 0.61237244],
    );
}
