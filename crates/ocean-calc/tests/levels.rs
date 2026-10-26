use ocean_calc::levels::{
    LevelError, QueueCounts, common_event_zero_rare_cycle, common_fog_rare_cycle,
    common_ordinary_item_request_count, common_rush_enemy_active_max,
    common_rush_enemy_appear_interval, common_tamaire_type_weights, common_weights, queue_counts,
};
use ocean_calc::species::{PeriodicGoldieInput, PeriodicGoldieSource, periodic_goldie_source};
use ocean_calc::tamaire::TamaireTypeWeights;

#[test]
fn exact_level_rows_select_the_expected_round_counts() {
    assert_eq!(
        queue_counts(0, 0),
        Ok(QueueCounts {
            wave_change_num: 3,
            rare_appear_num: 3
        })
    );
    assert_eq!(
        queue_counts(100, 0),
        Ok(QueueCounts {
            wave_change_num: 4,
            rare_appear_num: 5
        })
    );
    assert_eq!(
        queue_counts(1665, 2),
        Ok(QueueCounts {
            wave_change_num: 10,
            rare_appear_num: 35
        })
    );
}

#[test]
fn intermediate_difficulty_uses_single_precision_truncation() {
    assert_eq!(
        queue_counts(70, 0),
        Ok(QueueCounts {
            wave_change_num: 3,
            rare_appear_num: 4
        })
    );
    assert_eq!(
        queue_counts(70, 1),
        Ok(QueueCounts {
            wave_change_num: 3,
            rare_appear_num: 4
        })
    );
    assert_eq!(
        queue_counts(70, 2),
        Ok(QueueCounts {
            wave_change_num: 3,
            rare_appear_num: 5
        })
    );
    assert_eq!(
        queue_counts(250, 0),
        Ok(QueueCounts {
            wave_change_num: 4,
            rare_appear_num: 6
        })
    );
    assert_eq!(
        queue_counts(250, 1),
        Ok(QueueCounts {
            wave_change_num: 4,
            rare_appear_num: 8
        })
    );
    assert_eq!(
        queue_counts(250, 2),
        Ok(QueueCounts {
            wave_change_num: 4,
            rare_appear_num: 10
        })
    );
}

#[test]
fn difficulty_is_capped_and_nonexistent_round_is_rejected() {
    assert_eq!(queue_counts(2000, 2), queue_counts(1665, 2));
    assert_eq!(queue_counts(100, 3), Err(LevelError::InvalidRound(3)));
}

#[test]
fn common_weight_rows_and_interpolation_are_explicitly_source_values() {
    for difficulty in [0, 40] {
        let weights = common_weights(difficulty);
        assert_eq!(weights.water, [1, 3, 1]);
        assert_eq!(weights.event, [1, 0, 0, 0, 0, 0, 0, 0, 0]);
    }
    let between = common_weights(70);
    assert_eq!(between.water, [1, 3, 1]);
    assert_eq!(between.event, [12, 0, 0, 0, 0, 0, 0, 0, 0]);
    for difficulty in [100, 1665, 2000] {
        let weights = common_weights(difficulty);
        assert_eq!(weights.water, [1, 3, 1]);
        assert_eq!(weights.event, [24, 1, 1, 1, 1, 1, 1, 1, 1]);
    }
}

#[test]
fn ordinary_item_request_count_uses_the_effective_level_row_follower_field() {
    assert_eq!(common_ordinary_item_request_count(0), 2);
    assert_eq!(common_ordinary_item_request_count(40), 2);
    assert_eq!(common_ordinary_item_request_count(70), 2);
    assert_eq!(common_ordinary_item_request_count(100), 3);
    assert_eq!(common_ordinary_item_request_count(400), 3);
    assert_eq!(common_ordinary_item_request_count(425), 2);
    assert_eq!(common_ordinary_item_request_count(450), 2);
    assert_eq!(common_ordinary_item_request_count(451), 1);
    assert_eq!(common_ordinary_item_request_count(500), 1);
    assert_eq!(common_ordinary_item_request_count(1665), 1);
    assert_eq!(common_ordinary_item_request_count(2000), 1);
}

#[test]
fn event_zero_rare_cycle_uses_explicit_common_level_rows() {
    for (difficulty, expected) in [
        (0, 3),
        (40, 3),
        (100, 3),
        (200, 3),
        (300, 3),
        (400, 4),
        (500, 5),
        (1000, 7),
        (1665, 8),
    ] {
        assert_eq!(common_event_zero_rare_cycle(difficulty), expected);
    }
}

#[test]
fn event_zero_rare_cycle_truncates_between_rows_and_clamps() {
    for (difficulty, expected) in [
        (399, 3),
        (400, 4),
        (499, 4),
        (500, 5),
        (749, 5),
        (750, 6),
        (999, 6),
        (1000, 7),
        (1664, 7),
        (1665, 8),
        (2000, 8),
    ] {
        assert_eq!(
            common_event_zero_rare_cycle(difficulty),
            expected,
            "difficulty={difficulty}"
        );
    }
}

#[test]
fn common_cycle_can_feed_conditional_event_zero_goldie_predicate() {
    let input = PeriodicGoldieInput {
        event: Some(0),
        global_cycle_flag: true,
        accepted_rare_counter: 4,
        fog_cycle: None,
        event_none_cycle: Some(common_event_zero_rare_cycle(750)),
    };
    assert_eq!(periodic_goldie_source(input), Ok(None));
    assert_eq!(
        periodic_goldie_source(PeriodicGoldieInput {
            accepted_rare_counter: 5,
            ..input
        }),
        Ok(Some(PeriodicGoldieSource::EventNoneGlobalCycle))
    );
}

#[test]
fn fog_cycle_uses_separate_event_fog_source_rows_and_stock_interpolation() {
    for (difficulty, expected) in [
        (0, 3),
        (40, 3),
        (100, 3),
        (199, 3),
        (200, 4),
        (300, 4),
        (399, 4),
        (400, 5),
        (500, 5),
        (1000, 5),
        (1665, 5),
        (2000, 5),
    ] {
        assert_eq!(common_fog_rare_cycle(difficulty), expected);
    }
}

#[test]
fn fog_cycle_feeds_existing_optional_half_cycle_predicate_once() {
    for (difficulty, full_cycle, half_cycle) in [(0, 3, 2), (200, 4, 2), (400, 5, 3)] {
        assert_eq!(common_fog_rare_cycle(difficulty), full_cycle);
        let input = PeriodicGoldieInput {
            event: Some(5),
            global_cycle_flag: false,
            accepted_rare_counter: full_cycle - 1,
            fog_cycle: Some(common_fog_rare_cycle(difficulty)),
            event_none_cycle: None,
        };
        assert_eq!(
            periodic_goldie_source(input),
            Ok(Some(PeriodicGoldieSource::FogCycle))
        );
        assert_eq!(
            periodic_goldie_source(PeriodicGoldieInput {
                global_cycle_flag: true,
                accepted_rare_counter: half_cycle - 1,
                ..input
            }),
            Ok(Some(PeriodicGoldieSource::FogCycle))
        );
    }
}

#[test]
fn rush_enemy_interval_preserves_negative_source_and_signed_stock_interpolation() {
    for (difficulty, expected) in [
        (0, -1),
        (1, 0),
        (7, 0),
        (8, 1),
        (20, 4),
        (39, 9),
        (40, 10),
        (100, 10),
        (1665, 10),
        (2000, 10),
    ] {
        assert_eq!(
            common_rush_enemy_appear_interval(difficulty),
            expected,
            "difficulty={difficulty}"
        );
    }
}

#[test]
fn rush_active_max_preserves_sentinel_and_signed_stock_interpolation() {
    for (difficulty, expected) in [
        (0, -1),
        (40, -1),
        (41, 0),
        (47, 0),
        (48, 1),
        (99, 14),
        (100, 15),
        (200, 17),
        (300, 19),
        (350, 20),
        (400, 22),
        (500, 25),
        (1000, 32),
        (1665, 32),
        (2000, 32),
    ] {
        assert_eq!(
            common_rush_enemy_active_max(difficulty),
            expected,
            "difficulty={difficulty}"
        );
    }
}

#[test]
fn tamaire_stock_weights_preserve_constructor_defaults_and_source_rows() {
    for (difficulty, large, small, standard) in [
        (0, 1, 1, 8),
        (40, 1, 1, 8),
        (100, 1, 2, 7),
        (200, 2, 3, 10),
        (300, 2, 3, 7),
        (400, 3, 4, 7),
        (500, 4, 5, 6),
        (1000, 5, 6, 3),
        (1665, 5, 5, 1),
    ] {
        assert_eq!(
            common_tamaire_type_weights(difficulty),
            TamaireTypeWeights {
                large,
                small,
                standard,
            },
            "difficulty={difficulty}"
        );
    }
}

#[test]
fn tamaire_stock_weights_interpolate_signed_fields_and_cap_difficulty() {
    for (difficulty, large, small, standard) in [
        (70, 1, 1, 7),
        (150, 1, 2, 8),
        (250, 2, 3, 8),
        (750, 4, 5, 4),
        (2000, 5, 5, 1),
    ] {
        assert_eq!(
            common_tamaire_type_weights(difficulty),
            TamaireTypeWeights {
                large,
                small,
                standard,
            },
            "difficulty={difficulty}"
        );
    }
}
