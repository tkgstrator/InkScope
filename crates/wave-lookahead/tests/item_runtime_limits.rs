use salmon_wave_lookahead::{
    levels::{OrdinaryItemLimitInput, OrdinaryItemLimits, ordinary_item_runtime_limits},
    ordinary_stream::EventZeroItemRuntime,
};

fn input() -> OrdinaryItemLimitInput {
    OrdinaryItemLimitInput {
        effective_rare_follower_num: 2,
        scene_mode: 0,
        queue_item_flag: false,
        deadline_marker: i32::MIN,
        director_time: 100,
    }
}

#[test]
fn normal_marker_uses_thirty_one_minus_follower_count() {
    for (followers, expected_request, expected_limit) in [(0, 1, 31), (1, 2, 30), (2, 3, 29)] {
        assert_eq!(
            ordinary_item_runtime_limits(OrdinaryItemLimitInput {
                effective_rare_follower_num: followers,
                ..input()
            }),
            OrdinaryItemLimits {
                requested_count: expected_request,
                item_total_limit: expected_limit,
            }
        );
    }
}

#[test]
fn deadline_marker_boundary_selects_the_correct_base() {
    for (marker, limit) in [(99, 28), (100, 29), (101, 29), (i32::MIN, 29)] {
        assert_eq!(
            ordinary_item_runtime_limits(OrdinaryItemLimitInput {
                deadline_marker: marker,
                ..input()
            }),
            OrdinaryItemLimits {
                requested_count: 3,
                item_total_limit: limit,
            }
        );
    }
}

#[test]
fn queue_item_flag_overrides_scene_mode_and_level_followers() {
    let scene_override = ordinary_item_runtime_limits(OrdinaryItemLimitInput {
        effective_rare_follower_num: 1,
        scene_mode: 7,
        ..input()
    });
    assert_eq!(
        scene_override,
        OrdinaryItemLimits {
            requested_count: 3,
            item_total_limit: 29,
        }
    );
    let item_override = ordinary_item_runtime_limits(OrdinaryItemLimitInput {
        effective_rare_follower_num: 1,
        scene_mode: 7,
        queue_item_flag: true,
        ..input()
    });
    assert_eq!(
        item_override,
        OrdinaryItemLimits {
            requested_count: 1,
            item_total_limit: 31,
        }
    );
}

#[test]
fn word_arithmetic_wraps_and_runtime_context_accepts_the_values() {
    let wrapped = ordinary_item_runtime_limits(OrdinaryItemLimitInput {
        effective_rare_follower_num: i32::MIN,
        ..input()
    });
    assert_eq!(wrapped.requested_count, -2_147_483_647);
    assert_eq!(wrapped.item_total_limit, -2_147_483_617);

    let ordinary = ordinary_item_runtime_limits(input());
    let runtime = EventZeroItemRuntime {
        active_rare_total: 4,
        active_total: 7,
        requested_count: ordinary.requested_count,
        item_total_limit: ordinary.item_total_limit,
    };
    assert_eq!(runtime.requested_count, 3);
    assert_eq!(runtime.item_total_limit, 29);
}
