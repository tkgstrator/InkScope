use ocean_calc::follower_budget::{
    DirectorCapAssignmentInput, FollowerBudgetScene, director_cap_after_scheduler_assignment,
    follower_budget_after_update,
};
use ocean_calc::levels::{LevelError, RoundPopulationLimits, common_round_population_limits};

#[test]
fn owned_common_round_caps_match_all_source_rows() {
    // Independently decoded from 11.3.0 SingletonParam.pack.zs,
    // Gyml/Singleton/spl__CoopLevelsConfig.spl__CoopLevelsConfig.bgyml.
    const ROWS: &[(u32, [i32; 3], [i32; 3])] = &[
        (0, [3, 3, 4], [5, 5, 7]),
        (40, [3, 4, 5], [5, 6, 8]),
        (100, [7, 8, 9], [11, 12, 13]),
        (200, [8, 9, 10], [12, 14, 16]),
        (300, [9, 10, 12], [14, 16, 18]),
        (400, [10, 12, 14], [16, 18, 20]),
        (500, [12, 14, 16], [18, 20, 22]),
        (1000, [22, 24, 26], [28, 30, 32]),
        (1665, [24, 25, 26], [30, 31, 32]),
    ];
    for &(difficulty, first, second) in ROWS {
        for round in 0..3 {
            assert_eq!(
                common_round_population_limits(difficulty, round as u8),
                Ok(RoundPopulationLimits {
                    first_half: first[round],
                    second_half: second[round],
                }),
                "difficulty={difficulty}, source_round={round}"
            );
        }
    }
}

#[test]
fn common_round_caps_interpolate_and_validate_source_round() {
    assert_eq!(
        common_round_population_limits(70, 2),
        Ok(RoundPopulationLimits {
            first_half: 7,
            second_half: 10,
        })
    );
    assert_eq!(
        common_round_population_limits(750, 0),
        Ok(RoundPopulationLimits {
            first_half: 17,
            second_half: 23,
        })
    );
    assert_eq!(
        common_round_population_limits(9999, 2),
        common_round_population_limits(1665, 2)
    );
    assert_eq!(
        common_round_population_limits(100, 3),
        Err(LevelError::InvalidRound(3))
    );
}

fn cap_input() -> DirectorCapAssignmentInput {
    DirectorCapAssignmentInput {
        previous_cap: 19,
        effective_config_available: true,
        event_enemy_active_max: -1,
        director_time: 49,
        half_time: 50,
        round: RoundPopulationLimits {
            first_half: 7,
            second_half: 11,
        },
    }
}

#[test]
fn unavailable_config_skips_only_the_guarded_assignment() {
    let mut input = cap_input();
    input.effective_config_available = false;
    input.previous_cap = -7;
    assert_eq!(director_cap_after_scheduler_assignment(input), -7);
}

#[test]
fn nonnegative_event_cap_bypasses_round_and_time() {
    let mut input = cap_input();
    input.event_enemy_active_max = 0;
    assert_eq!(director_cap_after_scheduler_assignment(input), 0);
    input.event_enemy_active_max = 24;
    input.director_time = i32::MAX;
    assert_eq!(director_cap_after_scheduler_assignment(input), 24);
}

#[test]
fn signed_halfway_boundary_selects_round_cap() {
    let mut input = cap_input();
    assert_eq!(director_cap_after_scheduler_assignment(input), 7);
    input.director_time = 50;
    assert_eq!(director_cap_after_scheduler_assignment(input), 11);
    input.director_time = -2;
    input.half_time = -1;
    assert_eq!(director_cap_after_scheduler_assignment(input), 7);
    input.director_time = -1;
    assert_eq!(director_cap_after_scheduler_assignment(input), 11);
}

#[test]
fn scheduler_cap_and_follower_budget_are_separate_updates() {
    let cap = director_cap_after_scheduler_assignment(cap_input());
    assert_eq!(cap, 7);
    assert_eq!(
        follower_budget_after_update(
            cap,
            FollowerBudgetScene::Present {
                applied_event_id: 0,
                event_enemy_active_max: 5,
                event_enemy_active_max_coef: 50,
                water_enemy_active_max_coef: 100,
            },
        ),
        2
    );
}
