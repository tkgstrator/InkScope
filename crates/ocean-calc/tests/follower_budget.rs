use ocean_calc::follower_budget::{
    FollowerBudgetScene, OWNED_1130_WATER_ENEMY_ACTIVE_MAX_COEF, follower_budget_after_update,
};

fn present(event_id: u32, cap: i32, event_coef: i32, water_coef: i32) -> FollowerBudgetScene {
    FollowerBudgetScene::Present {
        applied_event_id: event_id,
        event_enemy_active_max: cap,
        event_enemy_active_max_coef: event_coef,
        water_enemy_active_max_coef: water_coef,
    }
}

#[test]
fn default_event_cap_uses_director_cap_at_update() {
    assert_eq!(
        follower_budget_after_update(
            17,
            present(0, -1, 100, OWNED_1130_WATER_ENEMY_ACTIVE_MAX_COEF),
        ),
        17
    );
}

#[test]
fn nonnegative_event_cap_is_not_limited_by_director_cap_for_other_events() {
    assert_eq!(
        follower_budget_after_update(17, present(0, 25, 100, 100)),
        25
    );
    assert_eq!(
        follower_budget_after_update(17, present(260, 25, 100, 100)),
        25
    );
}

#[test]
fn event_four_caps_at_signed_director_cap() {
    assert_eq!(
        follower_budget_after_update(17, present(4, 25, 100, 100)),
        17
    );
    assert_eq!(
        follower_budget_after_update(-3, present(4, 25, 100, 100)),
        -3
    );
}

#[test]
fn lower_coefficient_wins_regardless_of_its_source() {
    assert_eq!(follower_budget_after_update(1, present(0, 20, 80, 50)), 10);
    assert_eq!(follower_budget_after_update(1, present(0, 20, 50, 80)), 10);
}

#[test]
fn signed_negative_coefficient_is_preserved_and_division_truncates_toward_zero() {
    assert_eq!(follower_budget_after_update(1, present(0, 3, -50, 100)), -1);
    assert_eq!(
        follower_budget_after_update(-3, present(0, -1, 50, 100)),
        -1
    );
}

#[test]
fn product_wraps_at_32_bits_before_signed_division() {
    assert_eq!(
        follower_budget_after_update(0, present(0, i32::MAX, 100, 100)),
        -1
    );
}

#[test]
fn absent_scene_copies_director_cap_without_event_inputs() {
    assert_eq!(
        follower_budget_after_update(i32::MIN, FollowerBudgetScene::Absent),
        i32::MIN
    );
}
