use salmon_wave_lookahead::{
    SeedWords,
    direction::{
        DirectionAdvance, DirectionError, advance_direction_for_elapsed, choose_direction,
        direction_interval, predict_initial_directions,
    },
    rng::derive,
};

const SEED: [u32; 4] = [1, 2, 3, 4];
const DIRECTION_DOMAIN: u32 = 0x8d56_161f;

#[test]
fn initial_and_queue_tail_directions_use_previous_and_shared_rng() {
    let result = predict_initial_directions(SeedWords::from_scene(SEED), &[0, 1, 2])
        .expect("stage keys are valid");
    assert_eq!(result.wave_start, Some(2));
    assert_eq!(result.after_queue_build, Some(0));
    assert_eq!(
        result.director_rng_38c.as_array(),
        [0xf917_1386, 0x442f_e07b, 0x215a_da25, 0x5a98_dd9d]
    );
    assert_eq!(
        result.shared_state.as_array(),
        [3422436164, 3417165730, 4265732950, 1776012269]
    );
}

#[test]
fn director_hidden_advance_occurs_even_without_direction_candidates() {
    let result = predict_initial_directions(SeedWords::from_scene(SEED), &[]).unwrap();
    assert_eq!(result.wave_start, None);
    assert_eq!(result.after_queue_build, None);
    assert_eq!(
        result.director_rng_38c.as_array(),
        [0xf917_1386, 0x442f_e07b, 0x215a_da25, 0x5a98_dd9d]
    );
    assert_eq!(
        result.shared_state.as_array(),
        [3820670053, 1860123201, 3422436164, 3417165730]
    );
}

#[test]
fn missing_first_key_uses_swap_remove_order() {
    let mut rng = derive(SeedWords::from_scene(SEED), DIRECTION_DOMAIN);
    assert_eq!(choose_direction(&mut rng, &[1, 2], None), Ok(Some(1)));
    assert_eq!(
        rng.as_array(),
        [3820670053, 1860123201, 3422436164, 3417165730]
    );
}

#[test]
fn one_candidate_draws_but_no_candidates_do_not() {
    let mut rng = derive(SeedWords::from_scene(SEED), DIRECTION_DOMAIN);
    let before = rng.as_array();
    assert_eq!(choose_direction(&mut rng, &[2], None), Ok(Some(2)));
    assert_ne!(rng.as_array(), before);
    let after_one = rng.as_array();
    assert_eq!(choose_direction(&mut rng, &[2], Some(2)), Ok(Some(2)));
    assert_eq!(rng.as_array(), after_one);
    assert_eq!(choose_direction(&mut rng, &[], Some(1)), Ok(Some(1)));
    assert_eq!(rng.as_array(), after_one);
}

#[test]
fn invalid_stage_key_or_previous_direction_does_not_draw() {
    let mut rng = derive(SeedWords::from_scene(SEED), DIRECTION_DOMAIN);
    let before = rng.as_array();
    assert_eq!(
        choose_direction(&mut rng, &[9], None),
        Err(DirectionError::InvalidStageKey(9))
    );
    assert_eq!(
        choose_direction(&mut rng, &[0], Some(3)),
        Err(DirectionError::InvalidPrevious(3))
    );
    assert_eq!(rng.as_array(), before);
}

#[test]
fn queue_interval_matches_ordinary_wave_change_num() {
    assert_eq!(direction_interval(0), 0);
    assert_eq!(direction_interval(1), 0);
    assert_eq!(direction_interval(2), 4320);
    assert_eq!(direction_interval(3), 2160);
    assert_eq!(direction_interval(10), 480);
    assert_eq!(direction_interval(4322), 0);
}

#[test]
fn crossing_intervals_draws_once_before_due_items_even_when_skipping_many() {
    let initial = predict_initial_directions(SeedWords::from_scene(SEED), &[0, 1, 2]).unwrap();
    let mut rng = initial.shared_state;
    let before = rng.as_array();
    assert_eq!(
        advance_direction_for_elapsed(&mut rng, &[0, 1, 2], Some(1), -1, 0, 480),
        Ok(DirectionAdvance {
            direction: Some(1),
            branch_invoked: false,
        })
    );
    assert_eq!(rng.as_array(), before);
    assert_eq!(
        advance_direction_for_elapsed(&mut rng, &[0, 1, 2], Some(1), 479, 480, 480),
        Ok(DirectionAdvance {
            direction: Some(0),
            branch_invoked: true,
        })
    );
    assert_eq!(
        rng.as_array(),
        [3417165730, 4265732950, 1776012269, 1405483401]
    );
    assert_eq!(
        advance_direction_for_elapsed(&mut rng, &[0, 1, 2], Some(0), 480, 1920, 480),
        Ok(DirectionAdvance {
            direction: Some(1),
            branch_invoked: true,
        })
    );
    assert_eq!(
        rng.as_array(),
        [4265732950, 1776012269, 1405483401, 4138794124]
    );
}

#[test]
fn same_tick_backwards_zero_interval_and_no_candidate_keep_draw_rules() {
    let mut rng = derive(SeedWords::from_scene(SEED), DIRECTION_DOMAIN);
    let before = rng.as_array();
    for (old, new, interval) in [(10, 10, 480), (480, 479, 480), (0, 480, 0)] {
        assert_eq!(
            advance_direction_for_elapsed(&mut rng, &[0, 1, 2], Some(1), old, new, interval),
            Ok(DirectionAdvance {
                direction: Some(1),
                branch_invoked: false,
            })
        );
        assert_eq!(rng.as_array(), before);
    }
    assert_eq!(
        advance_direction_for_elapsed(&mut rng, &[], Some(1), 479, 480, 480),
        Ok(DirectionAdvance {
            direction: Some(1),
            branch_invoked: true,
        })
    );
    assert_eq!(rng.as_array(), before);
    assert_eq!(
        advance_direction_for_elapsed(&mut rng, &[2], None, 479, 480, 480),
        Ok(DirectionAdvance {
            direction: Some(2),
            branch_invoked: true,
        })
    );
    assert_ne!(rng.as_array(), before);
}

#[test]
fn signed_interval_and_branch_only_validation_follow_scheduler() {
    let mut rng = derive(SeedWords::from_scene(SEED), DIRECTION_DOMAIN);
    let before = rng.as_array();
    assert_eq!(
        advance_direction_for_elapsed(&mut rng, &[9], Some(3), 0, 480, 0),
        Ok(DirectionAdvance {
            direction: Some(3),
            branch_invoked: false,
        })
    );
    assert_eq!(rng.as_array(), before);
    assert_eq!(
        advance_direction_for_elapsed(&mut rng, &[9], Some(1), 479, 480, 480),
        Err(DirectionError::InvalidStageKey(9))
    );
    assert_eq!(rng.as_array(), before);
    assert!(
        advance_direction_for_elapsed(&mut rng, &[0, 1, 2], Some(1), 480, 0, -480)
            .unwrap()
            .branch_invoked
    );
}
