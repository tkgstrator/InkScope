use ocean_calc::{
    SeedWords,
    enemy_object::{
        EnemyObjectError, artillery_first_reservoir, artillery_initial_callback_allows,
        artillery_initial_flag, artillery_reservoir_from_state, initial_enemy_object_rng,
    },
};

#[test]
fn id_thirteen_object_state_and_flag_match_independent_static_vector() {
    let seed = SeedWords::from_scene([1, 2, 3, 4]);
    let mut rng = initial_enemy_object_rng(seed, 13).unwrap();
    assert_eq!(
        rng.as_array(),
        [0x2c4b6c7e, 0xf6433bb8, 0xa99aa3ca, 0x8620a5ec]
    );
    assert_eq!(rng.next32(), 0xf17f01ca);
    assert!(!artillery_initial_flag(seed));
    assert_eq!(artillery_initial_callback_allows(seed, 0), Ok(false));
    assert_eq!(artillery_initial_callback_allows(seed, 1), Ok(false));
    assert_eq!(artillery_initial_callback_allows(seed, 2), Ok(false));
}

#[test]
fn low_enemy_ids_advance_the_base_rng_id_plus_one_times() {
    let seed = SeedWords::from_scene([1, 2, 3, 4]);
    assert_eq!(
        initial_enemy_object_rng(seed, 0).unwrap().as_array(),
        [0xccea8e66, 0xdf813adb, 0x1261cf3b, 0x271a554b]
    );
    assert_eq!(
        initial_enemy_object_rng(seed, 1).unwrap().as_array(),
        [0x32b67581, 0x496564e7, 0x0249e4c1, 0x1fd98929]
    );
}

#[test]
fn normalized_zero_scene_seed_uses_the_game_global_initialization_path() {
    let seed = SeedWords::from_scene([0; 4]);
    let mut rng = initial_enemy_object_rng(seed, 13).unwrap();
    assert_eq!(
        rng.as_array(),
        [0x2b99700e, 0x48e2b388, 0xc8442610, 0x89c83083]
    );
    assert_eq!(rng.next32(), 0x693138b4);
    assert!(artillery_initial_flag(seed));
    assert_eq!(artillery_initial_callback_allows(seed, 0), Ok(true));
    assert_eq!(artillery_initial_callback_allows(seed, 1), Ok(true));
    assert_eq!(artillery_initial_callback_allows(seed, 2), Ok(false));
}

#[test]
fn invalid_enemy_id_and_tide_are_rejected() {
    let seed = SeedWords::from_scene([1, 2, 3, 4]);
    assert_eq!(
        initial_enemy_object_rng(seed, 32),
        Err(EnemyObjectError::InvalidEnemyId(32))
    );
    assert_eq!(
        artillery_initial_callback_allows(seed, 3),
        Err(EnemyObjectError::InvalidTide(3))
    );
}

#[test]
fn first_artillery_reservoir_uses_one_local_draw_per_matching_record() {
    let seed = SeedWords::from_scene([0; 4]);
    // This is the loaded direction-record order, not Banc Actors order.
    let result = artillery_first_reservoir(seed, 0, &[0, 2, 0, 1, 0]).unwrap();
    assert!(result.initial_flag);
    assert_eq!(result.matching_records, 3);
    assert_eq!(result.selected_record_index, Some(4));
    assert_eq!(
        result.object_rng_after.as_array(),
        [0x693138b4, 0x3412b8e9, 0xdd8f6cdd, 0x150b13c7]
    );
}

#[test]
fn first_artillery_reservoir_without_matching_records_consumes_only_flag_draw() {
    let seed = SeedWords::from_scene([1, 2, 3, 4]);
    let result = artillery_first_reservoir(seed, 0, &[1, 2, 1]).unwrap();
    assert_eq!(result.matching_records, 0);
    assert_eq!(result.selected_record_index, None);
    assert_eq!(
        result.object_rng_after.as_array(),
        [0xf6433bb8, 0xa99aa3ca, 0x8620a5ec, 0xf17f01ca]
    );
    assert_eq!(
        artillery_first_reservoir(seed, 3, &[0]),
        Err(EnemyObjectError::InvalidTide(3))
    );
}

#[test]
fn later_null_cached_point_uses_the_continued_artillery_local_state() {
    let seed = SeedWords::from_scene([0; 4]);
    let records = [0, 2, 0, 1, 0];
    let first = artillery_first_reservoir(seed, 0, &records).unwrap();
    let later = artillery_reservoir_from_state(first.object_rng_after, 0, &records).unwrap();
    assert_eq!(later.matching_records, 3);
    assert_eq!(later.selected_record_index, Some(2));
    assert_eq!(
        later.object_rng_after.as_array(),
        [0x150b13c7, 0xf51f7d4a, 0x546b46f0, 0xf2242124]
    );
}
