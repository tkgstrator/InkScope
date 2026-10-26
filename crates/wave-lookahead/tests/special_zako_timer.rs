use salmon_wave_lookahead::zako_timer::{
    SpecialZakoEvent, SpecialZakoTimerError, SpecialZakoTimerInput, ZakoTimerError, ZakoTimerInput,
    ZakoTimerStep, common_zako_population_gate_allows, step_relay_or_tamaire_zako_timer,
};

fn input(event: SpecialZakoEvent, timer_before: i32) -> SpecialZakoTimerInput {
    SpecialZakoTimerInput {
        event,
        common: ZakoTimerInput {
            timer_before,
            pre_countdown_allows: true,
            population_gate_allows: None,
            reset_interval: None,
            post_reset_allows: None,
        },
        active_total: None,
        runtime_candidate_count: None,
    }
}

fn idle(timer_after: i32) -> ZakoTimerStep {
    ZakoTimerStep {
        timer_after,
        timer_reset: false,
        spawn_path_entered: false,
    }
}

#[test]
fn pre_countdown_gate_preserves_timer_without_event_live_inputs() {
    for event in [SpecialZakoEvent::Relay7, SpecialZakoEvent::Tamaire8] {
        let mut input = input(event, 1);
        input.common.pre_countdown_allows = false;
        assert_eq!(step_relay_or_tamaire_zako_timer(input), Ok(idle(1)));
    }
}

#[test]
fn relay_active_cap_precedes_countdown_and_candidate_read() {
    let capped = SpecialZakoTimerInput {
        active_total: Some(29),
        ..input(SpecialZakoEvent::Relay7, 2)
    };
    assert_eq!(step_relay_or_tamaire_zako_timer(capped), Ok(idle(2)));

    let at_cap = SpecialZakoTimerInput {
        active_total: Some(28),
        ..input(SpecialZakoEvent::Relay7, 2)
    };
    assert_eq!(step_relay_or_tamaire_zako_timer(at_cap), Ok(idle(1)));

    let capped_and_due = SpecialZakoTimerInput {
        active_total: Some(29),
        ..input(SpecialZakoEvent::Relay7, 1)
    };
    assert_eq!(
        step_relay_or_tamaire_zako_timer(capped_and_due),
        Ok(idle(1))
    );
}

#[test]
fn relay_requires_active_total_after_mode_gate_even_before_due() {
    assert_eq!(
        step_relay_or_tamaire_zako_timer(input(SpecialZakoEvent::Relay7, 2)),
        Err(SpecialZakoTimerError::MissingActiveTotal)
    );
}

#[test]
fn relay_zero_or_negative_candidates_clamp_due_timer_without_population_inputs() {
    for candidate_count in [0, -1] {
        let relay = SpecialZakoTimerInput {
            active_total: Some(28),
            runtime_candidate_count: Some(candidate_count),
            ..input(SpecialZakoEvent::Relay7, 1)
        };
        assert_eq!(step_relay_or_tamaire_zako_timer(relay), Ok(idle(0)));
    }
}

#[test]
fn tamaire_countdown_needs_no_candidate_or_active_total() {
    assert_eq!(
        step_relay_or_tamaire_zako_timer(input(SpecialZakoEvent::Tamaire8, 2)),
        Ok(idle(1))
    );
}

#[test]
fn tamaire_zero_or_negative_candidates_clamp_due_timer() {
    for candidate_count in [0, -1] {
        let tamaire = SpecialZakoTimerInput {
            runtime_candidate_count: Some(candidate_count),
            ..input(SpecialZakoEvent::Tamaire8, 1)
        };
        assert_eq!(step_relay_or_tamaire_zako_timer(tamaire), Ok(idle(0)));
    }
}

#[test]
fn due_timer_requires_the_selected_events_candidate_count() {
    for (event, active_total) in [
        (SpecialZakoEvent::Relay7, Some(28)),
        (SpecialZakoEvent::Tamaire8, None),
    ] {
        let due = SpecialZakoTimerInput {
            active_total,
            ..input(event, 1)
        };
        assert_eq!(
            step_relay_or_tamaire_zako_timer(due),
            Err(SpecialZakoTimerError::MissingCandidateCount)
        );
    }
}

#[test]
fn positive_candidates_delegate_population_reset_and_post_reset_gate() {
    for (event, active_total, post_reset_allows) in [
        (SpecialZakoEvent::Relay7, Some(28), true),
        (SpecialZakoEvent::Tamaire8, None, false),
    ] {
        let due = SpecialZakoTimerInput {
            active_total,
            runtime_candidate_count: Some(1),
            common: ZakoTimerInput {
                population_gate_allows: Some(true),
                reset_interval: Some(30),
                post_reset_allows: Some(post_reset_allows),
                ..input(event, 1).common
            },
            ..input(event, 1)
        };
        assert_eq!(
            step_relay_or_tamaire_zako_timer(due),
            Ok(ZakoTimerStep {
                timer_after: 30,
                timer_reset: true,
                spawn_path_entered: post_reset_allows,
            })
        );
    }
}

#[test]
fn relay_population_gate_can_use_the_same_active_total_snapshot() {
    let active_total = 28;
    let due = SpecialZakoTimerInput {
        active_total: Some(active_total),
        runtime_candidate_count: Some(1),
        common: ZakoTimerInput {
            population_gate_allows: Some(common_zako_population_gate_allows(
                active_total,
                28,
                i32::MIN,
                0,
            )),
            ..input(SpecialZakoEvent::Relay7, 1).common
        },
        ..input(SpecialZakoEvent::Relay7, 1)
    };
    assert_eq!(step_relay_or_tamaire_zako_timer(due), Ok(idle(0)));
}

#[test]
fn positive_candidates_propagate_common_missing_input_errors() {
    let due = SpecialZakoTimerInput {
        runtime_candidate_count: Some(1),
        ..input(SpecialZakoEvent::Tamaire8, 1)
    };
    assert_eq!(
        step_relay_or_tamaire_zako_timer(due),
        Err(SpecialZakoTimerError::Common(
            ZakoTimerError::MissingPopulationGate
        ))
    );
    let with_population = SpecialZakoTimerInput {
        common: ZakoTimerInput {
            population_gate_allows: Some(true),
            ..due.common
        },
        ..due
    };
    assert_eq!(
        step_relay_or_tamaire_zako_timer(with_population),
        Err(SpecialZakoTimerError::Common(
            ZakoTimerError::MissingResetInterval
        ))
    );
    let with_interval = SpecialZakoTimerInput {
        common: ZakoTimerInput {
            reset_interval: Some(30),
            ..with_population.common
        },
        ..with_population
    };
    assert_eq!(
        step_relay_or_tamaire_zako_timer(with_interval),
        Err(SpecialZakoTimerError::Common(
            ZakoTimerError::MissingPostResetOutcome
        ))
    );
}
