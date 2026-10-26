use ocean_calc::rng::SeedWords;
use ocean_calc::rush_location::{
    RushLocationCallInput, RushLocationError, RushLocationInput, RushSelectorInput, RushTimerGates,
    trace_rush_locations,
};
use ocean_calc::zako_spot::{RuntimeZakoSpot, ZakoSpotError, ZakoSpotOutcome, ZakoSpotSource};

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

fn timer(post_reset_allows: bool) -> RushTimerGates {
    RushTimerGates {
        pre_countdown_allows: true,
        population_gate_allows: Some(true),
        reset_interval: Some(10),
        post_reset_allows: Some(post_reset_allows),
    }
}

fn trace<'a>(
    calls: &'a [RushLocationCallInput<'a>],
) -> Result<ocean_calc::rush_location::RushLocationTrace, RushLocationError> {
    trace_rush_locations(RushLocationInput {
        seed: SeedWords::from_scene([1, 2, 3, 4]),
        stage_keys: &[0, 1, 2],
        wave_change_num: 4,
        initial_timer: 0,
        calls,
    })
}

#[test]
fn three_rush_locations_carry_the_request_draw_into_the_next_spot() {
    let group = [Some(A), Some(B), Some(C), Some(D)];
    let calls: Vec<_> = (0..=20)
        .map(|index| RushLocationCallInput {
            elapsed_tick: index,
            timer: timer(true),
            selector: (index % 10 == 0).then(|| selector(&group)),
        })
        .collect();
    let trace = trace(&calls).unwrap();
    assert_eq!(trace.after_queue_build, Some(0));
    assert_eq!(trace.calls.len(), 21);
    assert_eq!(trace.calls[1].timer.timer_after, 9);
    assert!(trace.calls[1].spot.is_none());

    for (index, expected_source, expected_point, expected_draw) in [
        (0, 1, B, 0xc3f8_c1e5),
        (10, 0, A, 0x3e4e_8589),
        (20, 3, D, 0xa4a0_8e23),
    ] {
        let call = &trace.calls[index];
        assert_eq!(call.direction.direction, Some(0));
        assert!(call.timer.spawn_path_entered);
        assert_eq!(call.request_draw, Some(expected_draw));
        assert_eq!(
            call.spot.unwrap().outcome,
            ZakoSpotOutcome::SelectedPoint {
                combined_index: expected_source,
                source: ZakoSpotSource::Group {
                    source_index: expected_source,
                },
                spot: expected_point,
            }
        );
    }
    assert_eq!(
        trace.calls[0].rng_after_direct_path,
        [0x2045_6942, 0xd739_149d, 0x4bd1_4fd2, 0xc3f8_c1e5]
    );
    assert_eq!(
        trace.final_rng_after_direct_path,
        [0x8cc5_c22d, 0x8a2e_077f, 0xc006_b97e, 0xa4a0_8e23]
    );
}

#[test]
fn post_reset_veto_delays_the_first_spot_without_advancing_its_rng() {
    let group = [Some(A), Some(B), Some(C), Some(D)];
    let calls: Vec<_> = (0..=10)
        .map(|index| RushLocationCallInput {
            elapsed_tick: index,
            timer: timer(index != 0),
            selector: (index == 10).then(|| selector(&group)),
        })
        .collect();
    let trace = trace(&calls).unwrap();
    assert!(trace.calls[0].timer.timer_reset);
    assert!(!trace.calls[0].timer.spawn_path_entered);
    assert_eq!(trace.calls[0].timer.timer_after, 10);
    assert!(trace.calls[0].spot.is_none());
    assert_eq!(
        trace.calls[10].spot.unwrap().outcome,
        ZakoSpotOutcome::SelectedPoint {
            combined_index: 1,
            source: ZakoSpotSource::Group { source_index: 1 },
            spot: B,
        }
    );
}

#[test]
fn a_zero_visit_selector_failure_consumes_no_spot_or_request_draw() {
    let group = [Some(A), Some(B), Some(C), Some(D)];
    let mut failed = selector(&group);
    failed.eligible_visit_count = 0;
    failed.selection_succeeded = false;
    let calls = [RushLocationCallInput {
        elapsed_tick: 0,
        timer: timer(true),
        selector: Some(failed),
    }];
    let trace = trace(&calls).unwrap();
    assert_eq!(
        trace.calls[0].spot.unwrap().outcome,
        ZakoSpotOutcome::SpeciesSelectionFailed
    );
    assert_eq!(trace.calls[0].request_draw, None);
    assert_eq!(
        trace.final_rng_after_direct_path,
        [0xf917_1386, 0x442f_e07b, 0x215a_da25, 0x5a98_dd9d]
    );
}

#[test]
fn selector_cannot_succeed_without_an_eligible_visit() {
    let group = [Some(A)];
    let mut impossible = selector(&group);
    impossible.eligible_visit_count = 0;
    let calls = [RushLocationCallInput {
        elapsed_tick: 0,
        timer: timer(true),
        selector: Some(impossible),
    }];
    assert_eq!(
        trace(&calls),
        Err(RushLocationError::SelectorSuccessWithoutEligibleVisit { call_index: 0 })
    );
}

#[test]
fn branch_inputs_and_observed_rng_must_match_the_reached_path() {
    let group = [Some(A), Some(B), Some(C), Some(D)];
    let due_without_selector = [RushLocationCallInput {
        elapsed_tick: 0,
        timer: timer(true),
        selector: None,
    }];
    assert_eq!(
        trace(&due_without_selector),
        Err(RushLocationError::MissingSelector { call_index: 0 })
    );

    let blocked_with_selector = [RushLocationCallInput {
        elapsed_tick: 0,
        timer: RushTimerGates {
            pre_countdown_allows: false,
            ..timer(true)
        },
        selector: Some(selector(&group)),
    }];
    assert_eq!(
        trace(&blocked_with_selector),
        Err(RushLocationError::UnexpectedSelector { call_index: 0 })
    );

    let mut wrong = selector(&group);
    wrong.observed_rng_before_selector = Some([0; 4]);
    let mismatch = [RushLocationCallInput {
        elapsed_tick: 0,
        timer: timer(true),
        selector: Some(wrong),
    }];
    assert!(matches!(
        trace(&mismatch),
        Err(RushLocationError::ObservedRngMismatch { call_index: 0, .. })
    ));
}

#[test]
fn selected_null_consumes_spot_draw_but_skips_request_draw() {
    let group = [Some(A), None, Some(C), Some(D)];
    let calls = [RushLocationCallInput {
        elapsed_tick: 0,
        timer: timer(true),
        selector: Some(selector(&group)),
    }];
    let trace = trace(&calls).unwrap();
    assert_eq!(
        trace.calls[0].spot.unwrap().outcome,
        ZakoSpotOutcome::SelectedNull {
            combined_index: 1,
            source: ZakoSpotSource::Group { source_index: 1 },
        }
    );
    assert_eq!(trace.calls[0].request_draw, None);
    assert_eq!(
        trace.final_rng_after_direct_path,
        [0x1b52_7e6b, 0x2045_6942, 0xd739_149d, 0x4bd1_4fd2]
    );
}

#[test]
fn absent_and_empty_main_groups_do_not_consume_a_spot_or_request_draw() {
    let group = [Some(A)];
    let mut absent = selector(&group);
    absent.runtime_groups = [None; 9];
    let missing_calls = [RushLocationCallInput {
        elapsed_tick: 0,
        timer: timer(true),
        selector: Some(absent),
    }];
    let missing = trace(&missing_calls).unwrap();
    assert_eq!(
        missing.calls[0].spot.unwrap().outcome,
        ZakoSpotOutcome::MissingGroup
    );
    assert_eq!(missing.calls[0].request_draw, None);
    assert_eq!(
        missing.final_rng_after_direct_path,
        [0x5a98_dd9d, 0x1b52_7e6b, 0x2045_6942, 0xd739_149d]
    );

    let mut empty = selector(&group);
    empty.runtime_groups[0] = Some(&[]);
    let empty_calls = [RushLocationCallInput {
        elapsed_tick: 0,
        timer: timer(true),
        selector: Some(empty),
    }];
    let empty = trace(&empty_calls).unwrap();
    assert_eq!(
        empty.calls[0].spot.unwrap().outcome,
        ZakoSpotOutcome::NoCandidates
    );
    assert_eq!(empty.calls[0].request_draw, None);
    assert_eq!(
        empty.final_rng_after_direct_path,
        missing.final_rng_after_direct_path
    );
}

#[test]
fn a_later_attempt_uses_its_own_runtime_group_snapshot() {
    let first_group = [Some(A), Some(B), Some(C), Some(D)];
    let second_group = [Some(D), Some(C), Some(B), Some(A)];
    let calls: Vec<_> = (0..=10)
        .map(|index| RushLocationCallInput {
            elapsed_tick: index,
            timer: timer(true),
            selector: match index {
                0 => Some(selector(&first_group)),
                10 => Some(selector(&second_group)),
                _ => None,
            },
        })
        .collect();
    let trace = trace(&calls).unwrap();
    assert_eq!(
        trace.calls[0].spot.unwrap().outcome,
        ZakoSpotOutcome::SelectedPoint {
            combined_index: 1,
            source: ZakoSpotSource::Group { source_index: 1 },
            spot: B,
        }
    );
    assert_eq!(
        trace.calls[10].spot.unwrap().outcome,
        ZakoSpotOutcome::SelectedPoint {
            combined_index: 0,
            source: ZakoSpotSource::Group { source_index: 0 },
            spot: D,
        }
    );
}

#[test]
fn interval_one_updates_the_direction_only_on_the_first_of_two_tick_zero_calls() {
    let group = [Some(A), Some(B), Some(C), Some(D)];
    let mut runtime_groups: [Option<&[Option<RuntimeZakoSpot>]>; 9] = [None; 9];
    runtime_groups[6] = Some(&group);
    let mut snapshot = selector(&group);
    snapshot.runtime_groups = runtime_groups;
    let calls = [
        RushLocationCallInput {
            elapsed_tick: 0,
            timer: RushTimerGates {
                reset_interval: Some(0),
                ..timer(true)
            },
            selector: Some(snapshot),
        },
        RushLocationCallInput {
            elapsed_tick: 0,
            timer: RushTimerGates {
                reset_interval: Some(0),
                ..timer(true)
            },
            selector: Some(snapshot),
        },
    ];
    let trace = trace_rush_locations(RushLocationInput {
        seed: SeedWords::from_scene([1, 2, 3, 4]),
        stage_keys: &[0, 1, 2],
        wave_change_num: 4321,
        initial_timer: 0,
        calls: &calls,
    })
    .unwrap();
    assert_eq!(trace.after_queue_build, Some(0));
    assert_eq!(trace.calls[0].direction.direction, Some(2));
    assert!(trace.calls[0].direction.branch_invoked);
    assert_eq!(trace.calls[1].direction.direction, Some(2));
    assert!(!trace.calls[1].direction.branch_invoked);
    assert_eq!(trace.calls[0].spot.unwrap().group_key, Some(6));
    assert_eq!(trace.calls[1].spot.unwrap().group_key, Some(6));
    assert!(matches!(
        trace.calls[0].spot.unwrap().outcome,
        ZakoSpotOutcome::SelectedPoint {
            combined_index: 1,
            spot: B,
            ..
        }
    ));
    assert!(matches!(
        trace.calls[1].spot.unwrap().outcome,
        ZakoSpotOutcome::SelectedPoint {
            combined_index: 0,
            spot: A,
            ..
        }
    ));
}

#[test]
fn invalid_reached_tide_and_missing_direction_are_errors() {
    let group = [Some(A)];
    let mut bad_tide = selector(&group);
    bad_tide.tide = 3;
    let calls = [RushLocationCallInput {
        elapsed_tick: 0,
        timer: timer(true),
        selector: Some(bad_tide),
    }];
    assert_eq!(
        trace(&calls),
        Err(RushLocationError::Spot {
            call_index: 0,
            cause: ZakoSpotError::InvalidTide(3),
        })
    );

    let no_direction = trace_rush_locations(RushLocationInput {
        seed: SeedWords::from_scene([1, 2, 3, 4]),
        stage_keys: &[],
        wave_change_num: 4,
        initial_timer: 0,
        calls: &[RushLocationCallInput {
            elapsed_tick: 0,
            timer: timer(true),
            selector: Some(selector(&group)),
        }],
    });
    assert_eq!(
        no_direction,
        Err(RushLocationError::Spot {
            call_index: 0,
            cause: ZakoSpotError::InvalidDirection(-1),
        })
    );
}
