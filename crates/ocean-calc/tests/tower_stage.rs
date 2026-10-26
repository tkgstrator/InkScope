use ocean_calc::{
    SeedWords,
    stage_data::stage_spawns,
    tower_stage::{TowerFirstError, predict_first_tower_arrival},
};

#[test]
fn first_tower_arrival_draw_uses_tide_matching_static_count() {
    let seed = SeedWords::from_scene([0; 4]);
    let factory = stage_spawns("Cop_Factory").unwrap();
    let three = predict_first_tower_arrival(seed, factory, 0, 0).unwrap();
    assert_eq!(three.candidate_count, 3);
    assert_eq!(three.runtime_candidate_index, Some(2));
    assert_eq!(
        three.object_rng_after.as_array(),
        [0x8a8b_7097, 0x3856_27cc, 0x7bdd_df80, 0xd448_1d7f]
    );

    let highway = stage_spawns("Cop_Shakehighway").unwrap();
    let six = predict_first_tower_arrival(seed, highway, 0, 0).unwrap();
    assert_eq!(six.candidate_count, 6);
    assert_eq!(six.runtime_candidate_index, Some(4));
    assert_eq!(six.object_rng_after, three.object_rng_after);
}

#[test]
fn empty_synthetic_tower_group_does_not_draw() {
    let seed = SeedWords::from_scene([0; 4]);
    let mut stage = *stage_spawns("Cop_Factory").unwrap();
    stage.tower_arrivals[0] = &[];
    let result = predict_first_tower_arrival(seed, &stage, 0, 0).unwrap();
    assert_eq!(result.candidate_count, 0);
    assert_eq!(result.runtime_candidate_index, None);
    assert_eq!(
        result.object_rng_after.as_array(),
        [0xa441_6f73, 0x8a8b_7097, 0x3856_27cc, 0x7bdd_df80]
    );
}

#[test]
fn invalid_tower_group_key_is_rejected() {
    let seed = SeedWords::from_scene([0; 4]);
    let stage = stage_spawns("Cop_Factory").unwrap();
    assert_eq!(
        predict_first_tower_arrival(seed, stage, 3, 0),
        Err(TowerFirstError::InvalidTide(3))
    );
    assert_eq!(
        predict_first_tower_arrival(seed, stage, 0, 3),
        Err(TowerFirstError::InvalidDirection(3))
    );
}
