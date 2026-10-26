use salmon_wave_lookahead::{
    rng::XorShift128,
    scenario_seed::{
        Observation, ScenarioSeedError, ScenarioSeedFact, ScenarioSeedObservations,
        ScenarioSeedResolution, expand_scenario_seeds, global_rng_setting_words,
        resolve_scenario_seed,
    },
};

const ENTRIES: [u32; 4] = [0x11, 0x22, 0x33, 0x44];

fn confirmed() -> ScenarioSeedObservations {
    ScenarioSeedObservations {
        controller_entries_read: Observation::Confirmed,
        preserved_at_w28_gate: Observation::Confirmed,
        exact_controller_words: Observation::Confirmed,
        scene_copy_at_activation: Observation::Confirmed,
    }
}

fn with_observation(
    mut facts: ScenarioSeedObservations,
    fact: ScenarioSeedFact,
    observation: Observation,
) -> ScenarioSeedObservations {
    match fact {
        ScenarioSeedFact::ControllerEntriesRead => facts.controller_entries_read = observation,
        ScenarioSeedFact::PreservedAtW28Gate => facts.preserved_at_w28_gate = observation,
        ScenarioSeedFact::ExactControllerWords => facts.exact_controller_words = observation,
        ScenarioSeedFact::SceneCopyAtActivation => facts.scene_copy_at_activation = observation,
    }
    facts
}

#[test]
fn logical_entries_fill_missing_slots_from_the_first_entry() {
    assert_eq!(
        expand_scenario_seeds(&ENTRIES[..1]),
        Ok([0x11, 0x11, 0x11, 0x11])
    );
    assert_eq!(
        expand_scenario_seeds(&ENTRIES[..2]),
        Ok([0x11, 0x22, 0x11, 0x11])
    );
    assert_eq!(
        expand_scenario_seeds(&ENTRIES[..3]),
        Ok([0x11, 0x22, 0x33, 0x11])
    );
    assert_eq!(
        expand_scenario_seeds(&ENTRIES),
        Ok([0x11, 0x22, 0x33, 0x44])
    );
}

#[test]
fn empty_or_overlong_seed_arrays_are_rejected_differently() {
    assert_eq!(
        expand_scenario_seeds(&[]),
        Err(ScenarioSeedError::EmptyIndeterminate)
    );
    assert_eq!(
        expand_scenario_seeds(&[1, 2, 3, 4, 5]),
        Err(ScenarioSeedError::TooManyEntries(5))
    );
}

#[test]
fn confirmed_path_preserves_raw_zero_words_until_shared_rng_initialization() {
    let outcome = resolve_scenario_seed(&[0, 0, 0, 0], confirmed()).unwrap();
    assert_eq!(
        outcome,
        ScenarioSeedResolution::Confirmed {
            raw_scene_words: [0, 0, 0, 0],
            rng_seed_if_initialized: salmon_wave_lookahead::SeedWords::from_scene([0; 4]),
        }
    );
    let ScenarioSeedResolution::Confirmed {
        rng_seed_if_initialized,
        ..
    } = outcome
    else {
        panic!("all observations were confirmed");
    };
    assert_eq!(rng_seed_if_initialized.as_array(), [1, 0, 0, 0]);

    assert_eq!(
        resolve_scenario_seed(&ENTRIES, confirmed()).unwrap(),
        ScenarioSeedResolution::Confirmed {
            raw_scene_words: ENTRIES,
            rng_seed_if_initialized: salmon_wave_lookahead::SeedWords::from_scene(ENTRIES),
        }
    );
}

#[test]
fn every_unconfirmed_or_refuted_activation_fact_has_its_own_reason() {
    for fact in [
        ScenarioSeedFact::ControllerEntriesRead,
        ScenarioSeedFact::PreservedAtW28Gate,
        ScenarioSeedFact::ExactControllerWords,
        ScenarioSeedFact::SceneCopyAtActivation,
    ] {
        assert_eq!(
            resolve_scenario_seed(
                &ENTRIES,
                with_observation(confirmed(), fact, Observation::Unknown)
            )
            .unwrap(),
            ScenarioSeedResolution::Unresolved(fact)
        );
        assert_eq!(
            resolve_scenario_seed(
                &ENTRIES,
                with_observation(confirmed(), fact, Observation::Refuted)
            )
            .unwrap(),
            ScenarioSeedResolution::ScenarioPathRefuted(fact)
        );
    }
    // A proved counterexample takes priority even if another fact remains unknown.
    let facts = with_observation(
        with_observation(
            confirmed(),
            ScenarioSeedFact::ControllerEntriesRead,
            Observation::Unknown,
        ),
        ScenarioSeedFact::SceneCopyAtActivation,
        Observation::Refuted,
    );
    assert_eq!(
        resolve_scenario_seed(&ENTRIES, facts).unwrap(),
        ScenarioSeedResolution::ScenarioPathRefuted(ScenarioSeedFact::SceneCopyAtActivation)
    );
}

#[test]
fn separate_global_rng_branch_consumes_exactly_four_draws() {
    let incoming = XorShift128::from_words([1, 2, 3, 4]);
    let outcome = global_rng_setting_words(incoming);
    let expected = [0x80d, 0x181f, 0x4, 0x2020];
    assert_eq!(outcome.setting_words, expected);
    assert_eq!(outcome.advanced_global_state.as_array(), expected);
    assert_eq!(incoming.as_array(), [1, 2, 3, 4]);
}
