use salmon_wave_lookahead::{
    SeedWords,
    first_rare::{
        FirstEventRarePrediction, FirstRareError, FirstRareInput, predict_first_event_rare_species,
        predict_first_ordinary_rare,
    },
    schedule::{ScheduleBranch, ScheduleError, ScheduleInput},
    species::{PeriodicGoldieError, PeriodicGoldieInput, PeriodicGoldieSource, RareCandidateState},
    stage_data::{PillarAnchor, PillarStaticGate, SpawnActor, StageSpawns, stage_spawns},
};

static SYNTHETIC_ARRIVAL: [SpawnActor; 1] = [SpawnActor {
    actor_index: 1,
    hash: 1,
    translate: [0.0; 3],
    rotate: None,
}];
static MIXED_ANCHORS: [PillarAnchor; 2] = [
    PillarAnchor {
        actor_index: 2,
        hash: 2,
        translate: [0.0; 3],
        rotate: None,
        arrivals: &[],
        dangling_destinations: &[],
    },
    PillarAnchor {
        actor_index: 3,
        hash: 3,
        translate: [0.0; 3],
        rotate: None,
        arrivals: &SYNTHETIC_ARRIVAL,
        dangling_destinations: &[],
    },
];

fn input(seed: [u32; 4], tide: u8, first_tick: i32) -> FirstRareInput<'static> {
    FirstRareInput {
        schedule: ScheduleInput {
            seed: SeedWords::from_scene(seed),
            event_type: 0,
            wave_change_num: 10,
            rare_appear_num: 12,
            assumed_branch: ScheduleBranch::Normal,
        },
        stage: stage_spawns("Cop_Shakehighway").unwrap(),
        tide,
        global_word: 0x10100,
        first_scheduler_elapsed_tick: first_tick,
        periodic: PeriodicGoldieInput {
            event: Some(0),
            global_cycle_flag: false,
            accepted_rare_counter: 0,
            fog_cycle: None,
            event_none_cycle: None,
        },
    }
}

#[test]
fn first_item_combines_stage_pillar_and_seeded_artillery_gates() {
    let result = predict_first_ordinary_rare(input([1, 2, 3, 4], 0, 0)).unwrap();
    assert_eq!(result.nominal_first_entry.tick, 0);
    assert_eq!(result.wave_start_direction, Some(2));
    assert_eq!(result.after_queue_build_direction, Some(0));
    assert_eq!(result.first_direction, 0);
    assert!(!result.direction_branch_invoked);
    assert_eq!(result.candidate_states[11], RareCandidateState::Eligible);
    assert_eq!(result.candidate_states[13], RareCandidateState::Skip);
    assert_eq!(result.candidate_states[14], RareCandidateState::Eligible);
    assert_eq!(result.species_id, 6);
    assert_eq!(
        result.shared_state_after_species.as_array(),
        [1873002430, 2467946925, 3195445786, 3438216410]
    );
}

#[test]
fn normalized_zero_seed_can_enable_artillery_at_low_but_not_high_tide() {
    let low = predict_first_ordinary_rare(input([0; 4], 0, 0)).unwrap();
    assert_eq!(low.candidate_states[13], RareCandidateState::Eligible);
    assert_eq!(low.species_id, 5);
    assert_eq!(
        low.shared_state_after_species.as_array(),
        [2242081508, 762596970, 2525687459, 1644441704]
    );
    let high = predict_first_ordinary_rare(input([0; 4], 2, 0)).unwrap();
    assert_eq!(high.candidate_states[13], RareCandidateState::Skip);
    assert_eq!(high.species_id, 5);
    assert_eq!(
        high.shared_state_after_species.as_array(),
        [2328876716, 2242081508, 762596970, 2525687459]
    );
    assert_ne!(
        high.shared_state_after_species,
        low.shared_state_after_species
    );
}

#[test]
fn delayed_first_scheduler_call_updates_direction_before_species() {
    let initial = predict_first_ordinary_rare(input([1, 2, 3, 4], 0, 0)).unwrap();
    let delayed = predict_first_ordinary_rare(input([1, 2, 3, 4], 0, 960)).unwrap();
    assert!(delayed.direction_branch_invoked);
    assert_eq!(delayed.first_direction, 2);
    assert_eq!(delayed.species_id, 5);
    assert_eq!(
        delayed.shared_state_after_species.as_array(),
        [2467946925, 3195445786, 3438216410, 3200453814]
    );
    assert_ne!(
        delayed.shared_state_after_species,
        initial.shared_state_after_species
    );
}

#[test]
fn global_gate_excludes_id_fourteen_before_the_species_draw() {
    let mut value = input([1, 2, 3, 4], 0, 0);
    value.global_word = 0x100ff;
    let result = predict_first_ordinary_rare(value).unwrap();
    assert_eq!(result.candidate_states[11], RareCandidateState::Eligible);
    assert_eq!(result.candidate_states[14], RareCandidateState::Skip);
}

#[test]
fn invalid_tide_tick_and_missing_stage_direction_are_rejected() {
    assert_eq!(
        predict_first_ordinary_rare(input([1, 2, 3, 4], 3, 0)),
        Err(FirstRareError::InvalidTide(3))
    );
    assert_eq!(
        predict_first_ordinary_rare(input([1, 2, 3, 4], 0, -1)),
        Err(FirstRareError::NegativeElapsedTick(-1))
    );
    let empty = StageSpawns {
        name: "Empty",
        pack_sha256: "",
        banc_path: "",
        water_override: None,
        ban_event_mask: 0,
        groups: [&[]; 9],
        artillery_points: [&[]; 9],
        tower_arrivals: [&[]; 9],
        tamaire_geysers: &[],
        ikura_banks: &[],
        ikura_bank_bases: &[],
        carrier_box_locators: [&[]; 3],
        pillar_anchors: [&[]; 9],
    };
    let mut value = input([1, 2, 3, 4], 0, 0);
    value.stage = &empty;
    assert_eq!(
        predict_first_ordinary_rare(value),
        Err(FirstRareError::MissingDirection)
    );
}

#[test]
fn ambiguous_pillar_anchor_blocks_first_prediction() {
    let mut stage = StageSpawns {
        name: "Synthetic",
        pack_sha256: "",
        banc_path: "",
        water_override: None,
        ban_event_mask: 0,
        groups: [&SYNTHETIC_ARRIVAL; 9],
        artillery_points: [&[]; 9],
        tower_arrivals: [&[]; 9],
        tamaire_geysers: &[],
        ikura_banks: &[],
        ikura_bank_bases: &[],
        carrier_box_locators: [&[]; 3],
        pillar_anchors: [&[]; 9],
    };
    stage.pillar_anchors[0] = &MIXED_ANCHORS;
    assert_eq!(
        stage.pillar_initial_static_gate(0, 0),
        Some(PillarStaticGate::DependsOnRuntimeOrder)
    );
    let value = FirstRareInput {
        stage: &stage,
        ..input([1, 2, 3, 4], 0, 0)
    };
    assert_eq!(
        predict_first_ordinary_rare(value),
        Err(FirstRareError::PillarDependsOnRuntimeOrder)
    );
}

#[test]
fn absent_static_pillar_nodes_and_invalid_schedule_are_distinguished() {
    let no_pillar = StageSpawns {
        name: "NoPillar",
        pack_sha256: "",
        banc_path: "",
        water_override: None,
        ban_event_mask: 0,
        groups: [&SYNTHETIC_ARRIVAL; 9],
        artillery_points: [&[]; 9],
        tower_arrivals: [&[]; 9],
        tamaire_geysers: &[],
        ikura_banks: &[],
        ikura_bank_bases: &[],
        carrier_box_locators: [&[]; 3],
        pillar_anchors: [&[]; 9],
    };
    assert_eq!(
        no_pillar.pillar_initial_static_gate(0, 1),
        Some(PillarStaticGate::Unavailable)
    );
    let value = FirstRareInput {
        stage: &no_pillar,
        ..input([1, 2, 3, 4], 0, 0)
    };
    assert_eq!(
        predict_first_ordinary_rare(value).unwrap().candidate_states[11],
        RareCandidateState::Skip
    );

    let mut invalid_event = input([1, 2, 3, 4], 0, 0);
    invalid_event.schedule.event_type = 1;
    assert_eq!(
        predict_first_ordinary_rare(invalid_event),
        Err(FirstRareError::Schedule(ScheduleError::UnsupportedEvent(1)))
    );
    let mut invalid_counts = input([1, 2, 3, 4], 0, 0);
    invalid_counts.schedule.wave_change_num = 1;
    assert_eq!(
        predict_first_ordinary_rare(invalid_counts),
        Err(FirstRareError::Schedule(ScheduleError::InvalidCounts))
    );
}

#[test]
fn first_rare_rejects_forced_goldie_before_irrelevant_pillar_callback() {
    let mut stage = StageSpawns {
        name: "Synthetic",
        pack_sha256: "",
        banc_path: "",
        water_override: None,
        ban_event_mask: 0,
        groups: [&SYNTHETIC_ARRIVAL; 9],
        artillery_points: [&[]; 9],
        tower_arrivals: [&[]; 9],
        tamaire_geysers: &[],
        ikura_banks: &[],
        ikura_bank_bases: &[],
        carrier_box_locators: [&[]; 3],
        pillar_anchors: [&[]; 9],
    };
    stage.pillar_anchors[0] = &MIXED_ANCHORS;
    let mut value = FirstRareInput {
        stage: &stage,
        ..input([1, 2, 3, 4], 0, 0)
    };
    value.periodic.global_cycle_flag = true;
    value.periodic.event_none_cycle = Some(1);
    assert_eq!(
        predict_first_ordinary_rare(value),
        Err(FirstRareError::ForcedGoldieRequiresStream)
    );
}

#[test]
fn first_rare_rejects_missing_periodic_cycle_and_wrong_runtime_event() {
    let mut value = input([1, 2, 3, 4], 0, 0);
    value.periodic.global_cycle_flag = true;
    assert_eq!(
        predict_first_ordinary_rare(value),
        Err(FirstRareError::Periodic(
            PeriodicGoldieError::MissingEventNoneCycle
        ))
    );
    value.periodic.event = Some(5);
    assert_eq!(
        predict_first_ordinary_rare(value),
        Err(FirstRareError::UnexpectedRuntimeEvent(5))
    );
}

#[test]
fn fog_nonforced_and_missile_first_normal_species_match_fixed_event_zero_vector() {
    for event in [5, 6] {
        let mut value = input([1, 2, 3, 4], 0, 0);
        value.schedule.event_type = event;
        value.periodic.event = Some(event);
        value.periodic.fog_cycle = Some(3);
        value.periodic.global_cycle_flag = true;
        let normal = predict_first_ordinary_rare(value).unwrap();
        assert_eq!(normal.nominal_first_entry.tick, 0);
        assert_eq!(normal.first_direction, 0);
        assert_eq!(normal.species_id, 6);
        assert_eq!(
            normal.shared_state_after_species.as_array(),
            [1873002430, 2467946925, 3195445786, 3438216410]
        );
        assert_eq!(
            predict_first_event_rare_species(value),
            Ok(FirstEventRarePrediction::Normal(normal))
        );
    }
}

#[test]
fn fog_cycle_forces_first_goldie_without_needing_normal_callbacks() {
    let mut stage = StageSpawns {
        name: "Synthetic",
        pack_sha256: "",
        banc_path: "",
        water_override: None,
        ban_event_mask: 0,
        groups: [&SYNTHETIC_ARRIVAL; 9],
        artillery_points: [&[]; 9],
        tower_arrivals: [&[]; 9],
        tamaire_geysers: &[],
        ikura_banks: &[],
        ikura_bank_bases: &[],
        carrier_box_locators: [&[]; 3],
        pillar_anchors: [&[]; 9],
    };
    stage.pillar_anchors[0] = &MIXED_ANCHORS;
    let mut value = input([1, 2, 3, 4], 0, 0);
    value.stage = &stage;
    value.schedule.event_type = 5;
    value.periodic.event = Some(5);
    value.periodic.fog_cycle = Some(3);
    value.periodic.accepted_rare_counter = 2;
    assert_eq!(
        predict_first_ordinary_rare(value),
        Err(FirstRareError::ForcedGoldieRequiresStream)
    );
    let result = predict_first_event_rare_species(value).unwrap();
    assert_eq!(result.species_id(), 15);
    assert_eq!(
        result,
        FirstEventRarePrediction::ForcedGoldie {
            nominal_first_entry: salmon_wave_lookahead::schedule::NominalEntry {
                tick: 0,
                kind: salmon_wave_lookahead::schedule::EntryKind::Base,
            },
            source: PeriodicGoldieSource::FogCycle,
        }
    );
}

#[test]
fn event_zero_force_and_null_runtime_event_are_distinct() {
    let mut value = input([1, 2, 3, 4], 0, 0);
    value.periodic.global_cycle_flag = true;
    value.periodic.event_none_cycle = Some(1);
    assert_eq!(
        predict_first_event_rare_species(value),
        Ok(FirstEventRarePrediction::ForcedGoldie {
            nominal_first_entry: salmon_wave_lookahead::schedule::NominalEntry {
                tick: 0,
                kind: salmon_wave_lookahead::schedule::EntryKind::Base,
            },
            source: PeriodicGoldieSource::EventNoneGlobalCycle,
        })
    );
    value.periodic.event = None;
    value.periodic.event_none_cycle = None;
    assert!(matches!(
        predict_first_event_rare_species(value),
        Ok(FirstEventRarePrediction::Normal(_))
    ));
}

#[test]
fn event_six_does_not_force_goldie_and_new_api_validates_inputs() {
    let mut value = input([1, 2, 3, 4], 0, 0);
    value.schedule.event_type = 6;
    value.periodic.event = Some(6);
    value.periodic.global_cycle_flag = true;
    value.periodic.accepted_rare_counter = 2;
    assert!(matches!(
        predict_first_event_rare_species(value),
        Ok(FirstEventRarePrediction::Normal(_))
    ));

    value.periodic.event = Some(5);
    assert_eq!(
        predict_first_event_rare_species(value),
        Err(FirstRareError::UnexpectedRuntimeEvent(5))
    );
    value.schedule.event_type = 5;
    assert_eq!(
        predict_first_event_rare_species(value),
        Err(FirstRareError::Periodic(
            PeriodicGoldieError::MissingFogCycle
        ))
    );
    value.periodic.fog_cycle = Some(3);
    value.tide = 3;
    assert_eq!(
        predict_first_event_rare_species(value),
        Err(FirstRareError::InvalidTide(3))
    );
    value.tide = 0;
    value.first_scheduler_elapsed_tick = -1;
    assert_eq!(
        predict_first_event_rare_species(value),
        Err(FirstRareError::NegativeElapsedTick(-1))
    );
    value.first_scheduler_elapsed_tick = 0;
    value.schedule.wave_change_num = 1;
    assert_eq!(
        predict_first_event_rare_species(value),
        Err(FirstRareError::Schedule(ScheduleError::InvalidCounts))
    );
}
