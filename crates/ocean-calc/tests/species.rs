use ocean_calc::{
    SeedWords,
    direction::predict_initial_directions,
    species::{
        PeriodicGoldieError, PeriodicGoldieInput, PeriodicGoldieSource, RareCandidateState,
        RarePopulation, RareSpeciesSelection, choose_event_rare_species, choose_rare_boss,
        normal_first_rare_candidates, periodic_goldie_source, rare_candidate_states,
    },
};

fn shared_state() -> ocean_calc::rng::XorShift128 {
    predict_initial_directions(SeedWords::from_scene([1, 2, 3, 4]), &[0, 1, 2])
        .unwrap()
        .shared_state
}

#[test]
fn first_normal_candidates_use_rare_ids_global_gate_and_callback_results() {
    let all = normal_first_rare_candidates(0x10100, [true; 11]);
    for (id, state) in all.iter().enumerate() {
        assert_eq!(
            *state,
            if (4..=14).contains(&id) {
                RareCandidateState::Eligible
            } else {
                RareCandidateState::Skip
            }
        );
    }
    let mut callbacks = [true; 11];
    callbacks[11 - 4] = false;
    let gated = normal_first_rare_candidates(0x100ff, callbacks);
    assert_eq!(gated[11], RareCandidateState::Skip);
    assert_eq!(gated[14], RareCandidateState::Skip);
    assert_eq!(gated[10], RareCandidateState::Eligible);
}

#[test]
fn eligible_bosses_use_reservoir_draws_in_id_order() {
    let mut rng = shared_state();
    let candidates = normal_first_rare_candidates(0x10100, [true; 11]);
    assert_eq!(choose_rare_boss(&mut rng, &candidates), 6);
    assert_eq!(
        rng.as_array(),
        [2467946925, 3195445786, 3438216410, 3200453814]
    );
}

#[test]
fn at_limit_consumes_one_draw_without_joining_candidate_count() {
    let mut rng = shared_state();
    let mut candidates = [RareCandidateState::Skip; 32];
    candidates[4] = RareCandidateState::Eligible;
    candidates[5] = RareCandidateState::AtLimit;
    candidates[6] = RareCandidateState::Eligible;
    assert_eq!(choose_rare_boss(&mut rng, &candidates), 6);
    assert_eq!(
        rng.as_array(),
        [1776012269, 1405483401, 4138794124, 123380311]
    );
}

#[test]
fn skipped_candidates_do_not_consume_shared_randomness() {
    let mut rng = shared_state();
    let before = rng.as_array();
    assert_eq!(
        choose_rare_boss(&mut rng, &[RareCandidateState::Skip; 32]),
        0x20
    );
    assert_eq!(rng.as_array(), before);
}

#[test]
fn population_gate_runs_before_callbacks_and_uses_signed_sum() {
    let mut population = [RarePopulation {
        active: 0,
        reserved: 0,
    }; 11];
    let mut callbacks = [true; 11];
    population[4 - 4] = RarePopulation {
        active: 3,
        reserved: 0,
    };
    callbacks[4 - 4] = false;
    population[5 - 4] = RarePopulation {
        active: 3,
        reserved: 1,
    };
    population[6 - 4] = RarePopulation {
        active: 2,
        reserved: -1,
    };
    population[7 - 4] = RarePopulation {
        active: 0,
        reserved: i32::MAX,
    };
    population[9 - 4] = RarePopulation {
        active: i32::MIN,
        reserved: 0,
    };
    let states = rare_candidate_states(Some(0), 0x10100, population, callbacks);
    assert_eq!(states[4], RareCandidateState::AtLimit);
    assert_eq!(states[5], RareCandidateState::Eligible);
    assert_eq!(states[6], RareCandidateState::AtLimit);
    assert_eq!(states[7], RareCandidateState::Eligible);
    assert_eq!(states[9], RareCandidateState::AtLimit);
}

#[test]
fn event_seven_gate_and_global_id_fourteen_gate_precede_count_draws() {
    let population = [RarePopulation {
        active: 3,
        reserved: 0,
    }; 11];
    let all = [true; 11];
    let seven = rare_candidate_states(Some(7), 0x10100, population, all);
    for (id, state) in seven.iter().enumerate() {
        assert_eq!(
            *state,
            if id == 8 || id == 11 {
                RareCandidateState::AtLimit
            } else {
                RareCandidateState::Skip
            },
            "ID {id}"
        );
    }
    let null_context = rare_candidate_states(None, 0x100ff, population, all);
    assert_eq!(null_context[8], RareCandidateState::AtLimit);
    assert_eq!(null_context[14], RareCandidateState::Skip);
}

fn periodic_input(event: Option<u8>, counter: i32) -> PeriodicGoldieInput {
    PeriodicGoldieInput {
        event,
        global_cycle_flag: false,
        accepted_rare_counter: counter,
        fog_cycle: None,
        event_none_cycle: None,
    }
}

#[test]
fn fog_forces_id_fifteen_on_its_periodic_counter_only() {
    for (counter, forced) in [(0, false), (1, false), (2, true), (3, false), (5, true)] {
        let input = PeriodicGoldieInput {
            fog_cycle: Some(3),
            ..periodic_input(Some(5), counter)
        };
        assert_eq!(
            periodic_goldie_source(input).unwrap(),
            forced.then_some(PeriodicGoldieSource::FogCycle)
        );
    }
    let zero_cycle = PeriodicGoldieInput {
        fog_cycle: Some(0),
        ..periodic_input(Some(5), 0)
    };
    assert_eq!(periodic_goldie_source(zero_cycle), Ok(None));
}

#[test]
fn global_flag_halves_fog_cycle_with_game_float_rounding() {
    for (counter, forced) in [(0, false), (1, true), (2, false), (3, true)] {
        let input = PeriodicGoldieInput {
            global_cycle_flag: true,
            fog_cycle: Some(3),
            ..periodic_input(Some(5), counter)
        };
        assert_eq!(
            periodic_goldie_source(input).unwrap(),
            forced.then_some(PeriodicGoldieSource::FogCycle)
        );
    }
    // f32 rounds 16,777,217 to 16,777,216 before the divide by two.
    let large = PeriodicGoldieInput {
        global_cycle_flag: true,
        fog_cycle: Some(16_777_217),
        ..periodic_input(Some(5), 8_388_607)
    };
    assert_eq!(
        periodic_goldie_source(large),
        Ok(Some(PeriodicGoldieSource::FogCycle))
    );
}

#[test]
fn second_cycle_is_only_for_event_zero_with_global_flag() {
    let second = PeriodicGoldieInput {
        global_cycle_flag: true,
        event_none_cycle: Some(3),
        ..periodic_input(Some(0), 2)
    };
    assert_eq!(
        periodic_goldie_source(second),
        Ok(Some(PeriodicGoldieSource::EventNoneGlobalCycle))
    );
    assert_eq!(
        periodic_goldie_source(PeriodicGoldieInput {
            global_cycle_flag: false,
            ..second
        }),
        Ok(None)
    );
    assert_eq!(
        periodic_goldie_source(PeriodicGoldieInput {
            event: Some(6),
            ..second
        }),
        Ok(None)
    );
    assert_eq!(
        periodic_goldie_source(PeriodicGoldieInput {
            event: Some(5),
            accepted_rare_counter: 1,
            fog_cycle: Some(3),
            ..second
        }),
        Ok(Some(PeriodicGoldieSource::FogCycle))
    );
    assert_eq!(
        periodic_goldie_source(PeriodicGoldieInput {
            event: None,
            ..second
        }),
        Ok(None)
    );
}

#[test]
fn missing_required_runtime_cycles_are_unknown_not_false() {
    assert_eq!(
        periodic_goldie_source(periodic_input(Some(5), 2)),
        Err(PeriodicGoldieError::MissingFogCycle)
    );
    assert_eq!(
        periodic_goldie_source(PeriodicGoldieInput {
            global_cycle_flag: true,
            ..periodic_input(Some(0), 2)
        }),
        Err(PeriodicGoldieError::MissingEventNoneCycle)
    );
    assert_eq!(
        periodic_goldie_source(periodic_input(Some(9), 2)),
        Err(PeriodicGoldieError::InvalidEvent(9))
    );
}

#[test]
fn forced_goldie_bypasses_species_rng_while_fallback_uses_ordinary_scan() {
    let states = normal_first_rare_candidates(0x10100, [true; 11]);
    let mut rng = shared_state();
    let before = rng;
    let forced = choose_event_rare_species(
        &mut rng,
        PeriodicGoldieInput {
            fog_cycle: Some(3),
            ..periodic_input(Some(5), 2)
        },
        &states,
    )
    .unwrap();
    assert_eq!(
        forced,
        RareSpeciesSelection::ForcedGoldie(PeriodicGoldieSource::FogCycle)
    );
    assert_eq!(rng, before);

    let normal = choose_event_rare_species(
        &mut rng,
        PeriodicGoldieInput {
            fog_cycle: Some(3),
            ..periodic_input(Some(5), 0)
        },
        &states,
    )
    .unwrap();
    assert_eq!(normal, RareSpeciesSelection::Normal(6));
    assert_eq!(
        rng.as_array(),
        [2467946925, 3195445786, 3438216410, 3200453814]
    );

    let after_normal = rng;
    let error = choose_event_rare_species(&mut rng, periodic_input(Some(5), 2), &states);
    assert_eq!(error, Err(PeriodicGoldieError::MissingFogCycle));
    assert_eq!(rng, after_normal);
}

#[test]
fn normal_event_species_selector_rejects_impossible_candidate_ids() {
    let mut states = [RareCandidateState::Skip; 32];
    states[15] = RareCandidateState::Eligible;
    let mut rng = shared_state();
    let before = rng;
    assert_eq!(
        choose_event_rare_species(&mut rng, periodic_input(Some(6), 0), &states),
        Err(PeriodicGoldieError::InvalidCandidateId(15))
    );
    assert_eq!(rng, before);
}
