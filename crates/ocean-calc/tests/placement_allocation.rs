use ocean_calc::placement_allocation::{
    OrdinaryPlacementAllocation, OrdinaryPlacementInput, PlacementAllocationError,
    PlacementChannelSource, preallocate_ordinary_rare,
};
use ocean_calc::spawn_position::generic_rare_event_position;

fn input() -> OrdinaryPlacementInput {
    OrdinaryPlacementInput {
        rare_id: 4,
        requested_count: 3,
        primary_cap: 3,
        primary_active_before_create: 0,
        follower_cap: 5,
        follower_active_before_create: 1,
        follower_budget: 3,
        channel_source: PlacementChannelSource::OwnedRomfs1130,
    }
}

#[test]
fn owned_id_four_preallocation_feeds_generic_position_offset() {
    let allocation = preallocate_ordinary_rare(input()).unwrap();
    assert_eq!(
        allocation,
        OrdinaryPlacementAllocation {
            effective_count: 3,
            primary_reserved: true,
            follower_reserved: 2,
            channel_counts: [0, 2, 1],
            primary_channel: 2,
            follower_channel: 1,
        }
    );
    assert_eq!(
        generic_rare_event_position(
            [100.0, 200.0, 300.0],
            [1.0, 0.0, 0.0],
            allocation.primary_channel,
            [allocation.channel_counts[0], allocation.channel_counts[1]],
            0,
        ),
        Ok([97.5, 200.0, 300.0])
    );
}

#[test]
fn id_eleven_reserves_only_one_primary_when_free() {
    let mut value = input();
    value.rare_id = 11;
    let allocation = preallocate_ordinary_rare(value).unwrap();
    assert_eq!(allocation.effective_count, 1);
    assert!(allocation.primary_reserved);
    assert_eq!(allocation.follower_reserved, 0);
    assert_eq!(allocation.channel_counts, [1, 0, 0]);
}

#[test]
fn id_eleven_with_full_primary_can_reserve_one_follower() {
    let mut value = input();
    value.rare_id = 11;
    value.primary_active_before_create = value.primary_cap;
    let allocation = preallocate_ordinary_rare(value).unwrap();
    assert_eq!(allocation.effective_count, 1);
    assert!(!allocation.primary_reserved);
    assert_eq!(allocation.follower_reserved, 1);
    assert_eq!(allocation.channel_counts, [0, 1, 0]);
}

#[test]
fn full_primary_leaves_entire_request_for_followers() {
    let mut value = input();
    value.rare_id = 7;
    value.primary_active_before_create = value.primary_cap;
    let allocation = preallocate_ordinary_rare(value).unwrap();
    assert!(!allocation.primary_reserved);
    assert_eq!(allocation.follower_reserved, 3);
    assert_eq!(allocation.channel_counts, [0, 3, 0]);
}

#[test]
fn same_dynamic_channel_accumulates_primary_before_followers() {
    let mut value = input();
    value.rare_id = 7;
    value.channel_source = PlacementChannelSource::EffectiveInfoTypes {
        primary: Some(1),
        follower: Some(1),
    };
    let allocation = preallocate_ordinary_rare(value).unwrap();
    assert_eq!(
        (allocation.primary_channel, allocation.follower_channel),
        (0, 0)
    );
    assert_eq!(allocation.channel_counts, [3, 0, 0]);
}

#[test]
fn fixed_id_channel_ignores_contrary_effective_primary_type() {
    let mut value = input();
    value.channel_source = PlacementChannelSource::EffectiveInfoTypes {
        primary: Some(1),
        follower: Some(1),
    };
    let allocation = preallocate_ordinary_rare(value).unwrap();
    assert_eq!(
        (allocation.primary_channel, allocation.follower_channel),
        (2, 0)
    );
    assert_eq!(allocation.channel_counts, [2, 0, 1]);
}

#[test]
fn every_fixed_id_uses_the_elf_channel_table_before_enemy_info_type() {
    for (rare_id, expected_channel) in [(4, 2), (5, 0), (6, 0), (8, 2), (13, 0)] {
        let mut value = input();
        value.rare_id = rare_id;
        value.channel_source = PlacementChannelSource::EffectiveInfoTypes {
            primary: Some(0),
            follower: Some(0),
        };
        let allocation = preallocate_ordinary_rare(value).unwrap();
        assert_eq!(allocation.primary_channel, expected_channel, "ID {rare_id}");
    }
}

#[test]
fn dynamic_non_one_and_missing_lookups_take_channel_one() {
    let mut value = input();
    value.rare_id = 7;
    value.channel_source = PlacementChannelSource::EffectiveInfoTypes {
        primary: Some(0),
        follower: Some(1),
    };
    let allocation = preallocate_ordinary_rare(value).unwrap();
    assert_eq!(
        (allocation.primary_channel, allocation.follower_channel),
        (1, 0)
    );
    assert_eq!(allocation.channel_counts, [2, 1, 0]);

    value.channel_source = PlacementChannelSource::EffectiveInfoTypes {
        primary: None,
        follower: None,
    };
    let missing = preallocate_ordinary_rare(value).unwrap();
    assert_eq!((missing.primary_channel, missing.follower_channel), (1, 1));
    assert_eq!(missing.channel_counts, [0, 3, 0]);
}

#[test]
fn negative_live_budget_reduces_follower_channel_without_clamping() {
    let mut value = input();
    value.rare_id = 7;
    value.follower_budget = -1;
    let allocation = preallocate_ordinary_rare(value).unwrap();
    assert_eq!(allocation.follower_reserved, -1);
    assert_eq!(allocation.channel_counts, [1, -1, 0]);
}

#[test]
fn follower_capacity_uses_wrapping_signed_subtraction() {
    let mut value = input();
    value.follower_cap = i32::MIN;
    value.follower_active_before_create = 1;
    let allocation = preallocate_ordinary_rare(value).unwrap();
    assert_eq!(allocation.follower_reserved, 2);
    assert_eq!(allocation.channel_counts, [0, 2, 1]);
}

#[test]
fn invalid_ordinary_id_and_nonpositive_request_are_typed_errors() {
    let mut value = input();
    value.requested_count = 0;
    assert_eq!(
        preallocate_ordinary_rare(value),
        Err(PlacementAllocationError::InvalidRequestedCount(0))
    );
    value.requested_count = -2;
    assert_eq!(
        preallocate_ordinary_rare(value),
        Err(PlacementAllocationError::InvalidRequestedCount(-2))
    );
    value.requested_count = 3;
    value.rare_id = 3;
    assert_eq!(
        preallocate_ordinary_rare(value),
        Err(PlacementAllocationError::InvalidRareId(3))
    );
    value.rare_id = 15;
    assert_eq!(
        preallocate_ordinary_rare(value),
        Err(PlacementAllocationError::InvalidRareId(15))
    );
}
