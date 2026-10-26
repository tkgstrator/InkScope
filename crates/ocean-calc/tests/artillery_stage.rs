use ocean_calc::{
    SeedWords,
    artillery_stage::{ArtilleryStageError, ArtilleryStageRoute, predict_first_artillery_stage},
    stage_data::stage_spawns,
};

#[test]
fn singleton_stage_group_identifies_source_actor_but_not_callback_success() {
    let stage = stage_spawns("Cop_Carousel").unwrap();
    let result = predict_first_artillery_stage(SeedWords::from_scene([0; 4]), stage, 0, 0).unwrap();
    assert!(result.initial_flag);
    assert_eq!(result.matching_records, 1);
    let ArtilleryStageRoute::ReservoirSelected {
        matching_ordinal,
        source_actor,
    } = result.route
    else {
        panic!("expected one selected source record");
    };
    assert_eq!(matching_ordinal, 0);
    assert_eq!(source_actor.unwrap().actor_index, 369);
    assert_eq!(
        result.object_rng_after.as_array(),
        [0xc844_2610, 0x89c8_3083, 0x6931_38b4, 0x3412_b8e9]
    );
}

#[test]
fn doubleton_group_keeps_matching_ordinal_but_withholds_actor_identity() {
    let stage = stage_spawns("Cop_Shakehighway").unwrap();
    let result = predict_first_artillery_stage(SeedWords::from_scene([0; 4]), stage, 0, 0).unwrap();
    assert!(result.initial_flag);
    assert_eq!(result.matching_records, 2);
    let ArtilleryStageRoute::ReservoirSelected {
        matching_ordinal,
        source_actor,
    } = result.route
    else {
        panic!("expected selected source record");
    };
    assert_eq!(matching_ordinal, 0);
    assert!(source_actor.is_none());
    assert_eq!(
        result.object_rng_after.as_array(),
        [0x89c8_3083, 0x6931_38b4, 0x3412_b8e9, 0xdd8f_6cdd]
    );
}

#[test]
fn candidate_veto_skips_reservoir_draws() {
    let stage = stage_spawns("Cop_Carousel").unwrap();
    let flag_clear =
        predict_first_artillery_stage(SeedWords::from_scene([1, 2, 3, 4]), stage, 0, 0).unwrap();
    assert!(!flag_clear.initial_flag);
    assert_eq!(flag_clear.route, ArtilleryStageRoute::CandidateVeto);
    assert_eq!(flag_clear.matching_records, 0);
    assert_eq!(
        flag_clear.object_rng_after.as_array(),
        [0xf643_3bb8, 0xa99a_a3ca, 0x8620_a5ec, 0xf17f_01ca]
    );

    let high_tide =
        predict_first_artillery_stage(SeedWords::from_scene([0; 4]), stage, 2, 0).unwrap();
    assert!(high_tide.initial_flag);
    assert_eq!(high_tide.route, ArtilleryStageRoute::CandidateVeto);
    assert_eq!(high_tide.matching_records, 0);
    assert_eq!(
        high_tide.object_rng_after.as_array(),
        [0x48e2_b388, 0xc844_2610, 0x89c8_3083, 0x6931_38b4]
    );
}

#[test]
fn empty_static_group_consumes_only_the_initial_flag_draw() {
    let mut stage = *stage_spawns("Cop_Carousel").unwrap();
    stage.artillery_points[0] = &[];
    let result =
        predict_first_artillery_stage(SeedWords::from_scene([0; 4]), &stage, 0, 0).unwrap();
    assert!(result.initial_flag);
    assert_eq!(result.route, ArtilleryStageRoute::NoTideRecords);
    assert_eq!(result.matching_records, 0);
    assert_eq!(
        result.object_rng_after.as_array(),
        [0x48e2_b388, 0xc844_2610, 0x89c8_3083, 0x6931_38b4]
    );
}

#[test]
fn invalid_stage_keys_are_explicit() {
    let stage = stage_spawns("Cop_Carousel").unwrap();
    let seed = SeedWords::from_scene([0; 4]);
    assert_eq!(
        predict_first_artillery_stage(seed, stage, 3, 0),
        Err(ArtilleryStageError::InvalidTide(3))
    );
    assert_eq!(
        predict_first_artillery_stage(seed, stage, 0, 3),
        Err(ArtilleryStageError::InvalidDirection(3))
    );
}
