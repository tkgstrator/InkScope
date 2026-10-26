use salmon_wave_lookahead::{
    SeedWords,
    direction::DirectionError,
    rush_direction::{RushDirectionError, trace_rush_directions},
};

const SEED: [u32; 4] = [1, 2, 3, 4];
const KEYS: [u8; 3] = [0, 1, 2];

#[test]
fn traces_queue_ready_direction_then_one_update_per_crossing_call() {
    // This is the complete sequence of scheduler calls in this fixture.
    let trace = trace_rush_directions(
        SeedWords::from_scene(SEED),
        &KEYS,
        10,
        &[0, 479, 480, 480, 960],
    )
    .unwrap();
    assert_eq!(trace.wave_start, Some(2));
    assert_eq!(trace.after_queue_build, Some(0));
    assert_eq!(
        trace
            .calls
            .iter()
            .map(|call| call.direction)
            .collect::<Vec<_>>(),
        [Some(0), Some(0), Some(2), Some(2), Some(1)]
    );
    assert_eq!(
        trace
            .calls
            .iter()
            .map(|call| call.branch_invoked)
            .collect::<Vec<_>>(),
        [false, false, true, false, true]
    );
    assert_eq!(trace.calls[2].call_index, 2);
    assert_eq!(trace.calls[2].elapsed_tick, 480);
    assert_eq!(
        trace.calls[2].shared_state_after,
        [3417165730, 4265732950, 1776012269, 1405483401]
    );
    assert_eq!(
        trace.calls[4].shared_state_after,
        [4265732950, 1776012269, 1405483401, 4138794124]
    );
}

#[test]
fn the_complete_call_timeline_controls_the_number_of_direction_draws() {
    let two_calls =
        trace_rush_directions(SeedWords::from_scene(SEED), &KEYS, 10, &[0, 960]).unwrap();
    let three_calls =
        trace_rush_directions(SeedWords::from_scene(SEED), &KEYS, 10, &[0, 480, 960]).unwrap();
    assert_eq!(
        two_calls
            .calls
            .iter()
            .map(|call| call.direction)
            .collect::<Vec<_>>(),
        [Some(0), Some(2)]
    );
    assert_eq!(
        three_calls
            .calls
            .iter()
            .map(|call| call.direction)
            .collect::<Vec<_>>(),
        [Some(0), Some(2), Some(1)]
    );
    assert_eq!(
        two_calls.calls[1].shared_state_after,
        three_calls.calls[1].shared_state_after
    );
    assert_ne!(
        two_calls.calls[1].shared_state_after,
        three_calls.calls[2].shared_state_after
    );
}

#[test]
fn one_queued_item_and_empty_spawn_groups_consume_no_periodic_direction_draws() {
    let single =
        trace_rush_directions(SeedWords::from_scene(SEED), &KEYS, 1, &[0, 480, 960]).unwrap();
    assert!(single.calls.iter().all(|call| call.direction == Some(0)));
    assert!(single.calls.iter().all(|call| !call.branch_invoked));
    assert!(
        single
            .calls
            .windows(2)
            .all(|pair| { pair[0].shared_state_after == pair[1].shared_state_after })
    );

    let empty = trace_rush_directions(SeedWords::from_scene(SEED), &[], 10, &[0, 480]).unwrap();
    assert_eq!(empty.wave_start, None);
    assert_eq!(empty.after_queue_build, None);
    assert_eq!(empty.calls[0].direction, None);
    assert_eq!(empty.calls[1].direction, None);
    assert!(empty.calls[1].branch_invoked);
    assert_eq!(
        empty.calls[0].shared_state_after,
        empty.calls[1].shared_state_after
    );
}

#[test]
fn interval_one_updates_on_tick_zero_but_zero_interval_does_not() {
    // Signed -1 / 1 is -1, so the first call crosses a quotient boundary.
    let one = trace_rush_directions(SeedWords::from_scene(SEED), &KEYS, 4321, &[0]).unwrap();
    assert_eq!(one.after_queue_build, Some(0));
    assert!(one.calls[0].branch_invoked);
    assert_eq!(one.calls[0].direction, Some(2));

    let zero = trace_rush_directions(SeedWords::from_scene(SEED), &KEYS, 4322, &[0]).unwrap();
    assert!(!zero.calls[0].branch_invoked);
    assert_eq!(zero.calls[0].direction, Some(0));
}

#[test]
fn rejects_missing_queue_build_invalid_ticks_and_stage_keys() {
    for n in [0, -1] {
        assert_eq!(
            trace_rush_directions(SeedWords::from_scene(SEED), &KEYS, n, &[0]),
            Err(RushDirectionError::InvalidWaveChangeNum(n))
        );
    }
    assert_eq!(
        trace_rush_directions(SeedWords::from_scene(SEED), &KEYS, 10, &[-1]),
        Err(RushDirectionError::InvalidElapsedTick {
            call_index: 0,
            previous_tick: -1,
            elapsed_tick: -1,
        })
    );
    assert_eq!(
        trace_rush_directions(SeedWords::from_scene(SEED), &KEYS, 10, &[0, 480, 479]),
        Err(RushDirectionError::InvalidElapsedTick {
            call_index: 2,
            previous_tick: 480,
            elapsed_tick: 479,
        })
    );
    assert_eq!(
        trace_rush_directions(SeedWords::from_scene(SEED), &[9], 10, &[]),
        Err(RushDirectionError::Direction(
            DirectionError::InvalidStageKey(9)
        ))
    );
}
