use ocean_calc::{
    SeedWords,
    direction::predict_initial_directions,
    species::{choose_rare_boss, normal_first_rare_candidates},
    spot::choose_generic_spot_index,
};

fn after_first_boss() -> ocean_calc::rng::XorShift128 {
    let mut rng = predict_initial_directions(SeedWords::from_scene([1, 2, 3, 4]), &[0, 1, 2])
        .unwrap()
        .shared_state;
    assert_eq!(
        choose_rare_boss(&mut rng, &normal_first_rare_candidates(0x10100, [true; 11])),
        6
    );
    rng
}

#[test]
fn generic_spot_uses_one_shared_draw_and_multiply_high() {
    let mut rng = after_first_boss();
    assert_eq!(choose_generic_spot_index(&mut rng, 3), Some(2));
    assert_eq!(
        rng.as_array(),
        [3195445786, 3438216410, 3200453814, 3800794698]
    );
}

#[test]
fn singleton_spot_still_draws_and_empty_spot_does_not() {
    let mut rng = after_first_boss();
    let before = rng.as_array();
    assert_eq!(choose_generic_spot_index(&mut rng, 0), None);
    assert_eq!(rng.as_array(), before);
    assert_eq!(choose_generic_spot_index(&mut rng, 1), Some(0));
    assert_ne!(rng.as_array(), before);
}
