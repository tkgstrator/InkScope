use ocean_calc::{
    SeedWords,
    first_rare::{
        FirstRareInput, FirstRareSpotDraw, FirstRareSpotError, FirstSpotRoute,
        predict_first_ordinary_rare_with_spot,
    },
    schedule::{ScheduleBranch, ScheduleInput},
    species::PeriodicGoldieInput,
    stage_data::{SpawnActor, StageSpawns, stage_spawns},
};

static ONE_ACTOR: [SpawnActor; 1] = [SpawnActor {
    actor_index: 1,
    hash: 1,
    translate: [0.0; 3],
    rotate: None,
}];

fn input(stage: &StageSpawns, tick: i32) -> FirstRareInput<'_> {
    FirstRareInput {
        schedule: ScheduleInput {
            seed: SeedWords::from_scene([1, 2, 3, 4]),
            event_type: 0,
            wave_change_num: 10,
            rare_appear_num: 12,
            assumed_branch: ScheduleBranch::Normal,
        },
        stage,
        tide: 0,
        global_word: 0x10100,
        first_scheduler_elapsed_tick: tick,
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
fn first_generic_spot_uses_current_tide_direction_group_and_advances_rng() {
    let stage = stage_spawns("Cop_Shakehighway").unwrap();
    let result = predict_first_ordinary_rare_with_spot(input(stage, 0), |_| {
        panic!("ID6's standard spot callback always declines")
    })
    .unwrap();
    assert_eq!(result.rare.species_id, 6);
    assert_eq!(result.rare.first_direction, 0);
    assert_eq!(
        result.spot,
        FirstRareSpotDraw::Generic {
            candidate_count: 6,
            index: Some(4)
        }
    );
    assert_eq!(
        result.shared_state_after_spot.as_array(),
        [2467946925, 3195445786, 3438216410, 3200453814]
    );
}

#[test]
fn fog_and_missile_nonforced_first_species_continue_to_generic_spot() {
    let stage = stage_spawns("Cop_Shakehighway").unwrap();
    for event in [5, 6] {
        let mut value = input(stage, 0);
        value.schedule.event_type = event;
        value.periodic.event = Some(event);
        value.periodic.fog_cycle = Some(3);
        let result = predict_first_ordinary_rare_with_spot(value, |_| {
            panic!("ID6's standard spot callback always declines")
        })
        .unwrap();
        assert_eq!(result.rare.species_id, 6);
        assert_eq!(
            result.spot,
            FirstRareSpotDraw::Generic {
                candidate_count: 6,
                index: Some(4),
            }
        );
        assert_eq!(
            result.shared_state_after_spot.as_array(),
            [2467946925, 3195445786, 3438216410, 3200453814]
        );
    }
}

#[test]
fn all_seven_always_declining_species_take_the_generic_route_automatically() {
    let stage = stage_spawns("Cop_Shakehighway").unwrap();
    for (species, first_seed_word) in [(4, 13), (6, 1), (7, 5), (9, 0), (10, 8), (12, 2), (14, 18)]
    {
        let mut value = input(stage, 0);
        value.schedule.seed = SeedWords::from_scene([first_seed_word, 2, 3, 4]);
        let result = predict_first_ordinary_rare_with_spot(value, |_| {
            panic!("a standard always-declining spot callback needs no runtime route")
        })
        .unwrap();
        assert_eq!(result.rare.species_id, species);
        assert!(matches!(result.spot, FirstRareSpotDraw::Generic { .. }));
    }
}

#[test]
fn special_spot_species_requires_an_explicit_route() {
    let stage = stage_spawns("Cop_Shakehighway").unwrap();
    let mut value = input(stage, 0);
    value.schedule.seed = SeedWords::from_scene([6, 2, 3, 4]);
    assert_eq!(
        predict_first_ordinary_rare_with_spot(value, |species| {
            assert_eq!(species, 8);
            FirstSpotRoute::Unknown
        }),
        Err(FirstRareSpotError::UnknownSpotRoute)
    );
}

#[test]
fn generic_spot_with_one_candidate_draws_and_zero_candidates_do_not() {
    let mut stage = *stage_spawns("Cop_Shakehighway").unwrap();
    stage.groups[0] = &ONE_ACTOR;
    let one = predict_first_ordinary_rare_with_spot(input(&stage, 0), |_| {
        FirstSpotRoute::GenericStageGroup
    })
    .unwrap();
    assert_eq!(
        one.spot,
        FirstRareSpotDraw::Generic {
            candidate_count: 1,
            index: Some(0)
        }
    );
    assert_eq!(
        one.shared_state_after_spot.as_array(),
        [2467946925, 3195445786, 3438216410, 3200453814]
    );

    stage.groups[0] = &[];
    // Removing key 0 changes the direction candidates; direction 2 is then
    // selected, so remove its low-tide spot group as well.
    stage.groups[6] = &[];
    let zero = predict_first_ordinary_rare_with_spot(input(&stage, 0), |_| {
        FirstSpotRoute::GenericStageGroup
    })
    .unwrap();
    assert_eq!(zero.rare.first_direction, 2);
    assert_eq!(
        zero.spot,
        FirstRareSpotDraw::Generic {
            candidate_count: 0,
            index: None
        }
    );
    assert_eq!(
        zero.shared_state_after_spot,
        zero.rare.shared_state_after_species
    );
}

#[test]
fn callback_handled_route_preserves_rng_and_unknown_route_is_rejected() {
    let stage = stage_spawns("Cop_Shakehighway").unwrap();
    let mut value = input(stage, 0);
    value.schedule.seed = SeedWords::from_scene([6, 2, 3, 4]);
    let handled = predict_first_ordinary_rare_with_spot(value, |species| {
        assert_eq!(species, 8);
        FirstSpotRoute::HandledWithoutSharedDraw
    })
    .unwrap();
    assert_eq!(handled.spot, FirstRareSpotDraw::HandledByCallback);
    assert_eq!(
        handled.shared_state_after_spot,
        handled.rare.shared_state_after_species
    );
    assert_eq!(
        predict_first_ordinary_rare_with_spot(value, |_| FirstSpotRoute::Unknown),
        Err(FirstRareSpotError::UnknownSpotRoute)
    );
}

#[test]
fn delayed_first_scheduler_tick_uses_updated_direction_spot_group() {
    let stage = stage_spawns("Cop_Shakehighway").unwrap();
    let result = predict_first_ordinary_rare_with_spot(input(stage, 960), |_| {
        FirstSpotRoute::GenericStageGroup
    })
    .unwrap();
    assert_eq!(result.rare.first_direction, 2);
    assert!(matches!(
        result.spot,
        FirstRareSpotDraw::Generic {
            candidate_count: 3,
            index: Some(2)
        }
    ));
}

#[test]
fn untouched_first_pillar_spot_is_handled_even_when_anchor_identity_is_ambiguous() {
    let stage = stage_spawns("Cop_Temple").unwrap();
    assert_eq!(stage.pillar_anchors[0].len(), 3);
    let mut value = input(stage, 0);
    value.schedule.seed = SeedWords::from_scene([11, 2, 3, 4]);
    let result = predict_first_ordinary_rare_with_spot(value, |_| {
        panic!("a populated untouched Pillar group has a known spot route")
    })
    .unwrap();
    assert_eq!(result.rare.species_id, 11);
    assert_eq!(result.rare.first_direction, 0);
    assert_eq!(result.spot, FirstRareSpotDraw::HandledByCallback);
    assert_eq!(
        result.shared_state_after_spot,
        result.rare.shared_state_after_species
    );
}

#[test]
fn untouched_pillar_spot_uses_the_selected_nonzero_direction_group() {
    let stage = stage_spawns("Cop_Shakehighway").unwrap();
    let mut value = input(stage, 0);
    value.schedule.seed = SeedWords::from_scene([10, 2, 3, 4]);
    let result = predict_first_ordinary_rare_with_spot(value, |_| {
        panic!("the requested direction-one Pillar group is populated")
    })
    .unwrap();
    assert_eq!(result.rare.species_id, 11);
    assert_eq!(result.rare.first_direction, 1);
    assert_eq!(result.spot, FirstRareSpotDraw::HandledByCallback);
    assert_eq!(
        result.shared_state_after_spot,
        result.rare.shared_state_after_species
    );
}

#[test]
fn missing_requested_pillar_group_keeps_the_spot_route_external() {
    let mut stage = *stage_spawns("Cop_Temple").unwrap();
    stage.pillar_anchors[0] = &[];
    let mut value = input(&stage, 0);
    value.schedule.seed = SeedWords::from_scene([11, 2, 3, 4]);
    let mut route_requested = false;
    let result = predict_first_ordinary_rare_with_spot(value, |species| {
        assert_eq!(species, 11);
        route_requested = true;
        FirstSpotRoute::GenericStageGroup
    })
    .unwrap();
    assert!(route_requested);
    assert_eq!(result.rare.species_id, 11);
    assert_eq!(result.rare.first_direction, 0);
    assert!(matches!(
        result.spot,
        FirstRareSpotDraw::Generic {
            candidate_count: 3,
            index: Some(_)
        }
    ));
    assert_ne!(
        result.shared_state_after_spot,
        result.rare.shared_state_after_species
    );
}
