use ocean_calc::zako_timer::{
    RushNominalError, ZakoTimerError, ZakoTimerInput, ZakoTimerStep,
    common_zako_population_gate_allows, nominal_rush_attempt_calls, step_common_zako_timer,
};

fn input(timer_before: i32) -> ZakoTimerInput {
    ZakoTimerInput {
        timer_before,
        pre_countdown_allows: true,
        population_gate_allows: None,
        reset_interval: None,
        post_reset_allows: None,
    }
}

#[test]
fn early_mode_gate_preserves_timer_without_other_inputs() {
    assert_eq!(
        step_common_zako_timer(ZakoTimerInput {
            pre_countdown_allows: false,
            ..input(30)
        }),
        Ok(ZakoTimerStep {
            timer_after: 30,
            timer_reset: false,
            spawn_path_entered: false,
        })
    );
}

#[test]
fn countdown_only_reads_counter_until_it_becomes_due() {
    for (before, after) in [(30, 29), (2, 1)] {
        assert_eq!(
            step_common_zako_timer(input(before)),
            Ok(ZakoTimerStep {
                timer_after: after,
                timer_reset: false,
                spawn_path_entered: false,
            })
        );
    }
}

#[test]
fn due_timer_stays_zero_when_population_gate_blocks() {
    for before in [1, 0, -1] {
        assert_eq!(
            step_common_zako_timer(ZakoTimerInput {
                population_gate_allows: Some(false),
                ..input(before)
            }),
            Ok(ZakoTimerStep {
                timer_after: 0,
                timer_reset: false,
                spawn_path_entered: false,
            })
        );
    }
}

#[test]
fn allowed_timer_resets_before_later_spawn_outcome() {
    for (before, interval) in [(0, 30), (1, 20), (-1, 0), (-1, -3)] {
        assert_eq!(
            step_common_zako_timer(ZakoTimerInput {
                population_gate_allows: Some(true),
                reset_interval: Some(interval),
                post_reset_allows: Some(true),
                ..input(before)
            }),
            Ok(ZakoTimerStep {
                timer_after: interval,
                timer_reset: true,
                spawn_path_entered: true,
            })
        );
    }
}

#[test]
fn post_reset_veto_does_not_undo_timer_reset() {
    assert_eq!(
        step_common_zako_timer(ZakoTimerInput {
            population_gate_allows: Some(true),
            reset_interval: Some(30),
            post_reset_allows: Some(false),
            ..input(0)
        }),
        Ok(ZakoTimerStep {
            timer_after: 30,
            timer_reset: true,
            spawn_path_entered: false,
        })
    );
}

#[test]
fn missing_inputs_are_requested_only_after_their_branch_is_reached() {
    assert_eq!(
        step_common_zako_timer(input(0)),
        Err(ZakoTimerError::MissingPopulationGate)
    );
    assert_eq!(
        step_common_zako_timer(ZakoTimerInput {
            population_gate_allows: Some(true),
            ..input(0)
        }),
        Err(ZakoTimerError::MissingResetInterval)
    );
    assert_eq!(
        step_common_zako_timer(ZakoTimerInput {
            population_gate_allows: Some(true),
            reset_interval: Some(30),
            ..input(0)
        }),
        Err(ZakoTimerError::MissingPostResetOutcome)
    );
}

#[test]
fn population_gate_uses_signed_marker_boundary_and_strict_budget() {
    assert!(common_zako_population_gate_allows(3, 4, i32::MIN, 100));
    assert!(!common_zako_population_gate_allows(3, 4, 99, 100));
    assert!(common_zako_population_gate_allows(3, 4, 100, 100));
    assert!(common_zako_population_gate_allows(3, 4, 101, 100));
    assert!(!common_zako_population_gate_allows(4, 4, i32::MIN, 100));
}

#[test]
fn population_gate_wraps_the_conditional_increment() {
    assert!(common_zako_population_gate_allows(i32::MAX, 0, 99, 100,));
    assert!(!common_zako_population_gate_allows(
        i32::MAX,
        0,
        i32::MIN,
        100,
    ));
}

#[test]
fn ideal_rush_timer_enters_zako_path_on_first_then_every_tenth_scheduler_call() {
    assert_eq!(nominal_rush_attempt_calls(10, 32), Ok(vec![0, 10, 20, 30]));
}

#[test]
fn ideal_rush_zero_interval_enters_each_call_and_zero_calls_stay_empty() {
    assert_eq!(nominal_rush_attempt_calls(0, 4), Ok(vec![0, 1, 2, 3]));
    assert_eq!(nominal_rush_attempt_calls(10, 0), Ok(vec![]));
}

#[test]
fn ideal_rush_rejects_unresolved_negative_reset_even_without_calls() {
    assert_eq!(
        nominal_rush_attempt_calls(-1, 32),
        Err(RushNominalError::UnresolvedEffectiveInterval(-1))
    );
    assert_eq!(
        nominal_rush_attempt_calls(-1, 0),
        Err(RushNominalError::UnresolvedEffectiveInterval(-1))
    );
}
