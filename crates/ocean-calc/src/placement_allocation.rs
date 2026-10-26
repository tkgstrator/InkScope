//! Local placement-channel reservations before ordinary Rare creation calls.
//!
//! The caller has already accepted the item's spot and passed or bypassed its
//! optional post-spot gate. These counts are assigned before creation; they
//! are neither creation-call counts nor successfully registered enemies.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlacementChannelSource {
    /// Enemy-info types from the owned 11.3.0 RomFS: Rare=1, follower=0.
    OwnedRomfs1130,
    /// Caller-observed live types. `None` means the lookup was absent.
    EffectiveInfoTypes {
        primary: Option<u32>,
        follower: Option<u32>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrdinaryPlacementInput {
    pub rare_id: u8,
    pub requested_count: i32,
    pub primary_cap: i32,
    pub primary_active_before_create: i32,
    pub follower_cap: i32,
    /// The active follower count before any primary creation call.
    pub follower_active_before_create: i32,
    /// Director `+0x448`, read before creation.
    pub follower_budget: i32,
    pub channel_source: PlacementChannelSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrdinaryPlacementAllocation {
    pub effective_count: i32,
    pub primary_reserved: bool,
    pub follower_reserved: i32,
    /// Local `C[0..3]`; the first two counts feed generic position math.
    pub channel_counts: [i32; 3],
    pub primary_channel: u8,
    pub follower_channel: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlacementAllocationError {
    InvalidRareId(u8),
    InvalidRequestedCount(i32),
}

/// Reserve the 11.3.0 local channel counts for an accepted ordinary Rare item.
///
/// The fixed primary-ID table takes precedence over `primary` effective type.
/// Dynamic primary IDs and the follower use channel 0 only for type 1; every
/// other type, including a missing lookup, uses channel 1. Signed minima and
/// wrapping 32-bit operations follow `0x375ea98..0x375ec48`.
pub fn preallocate_ordinary_rare(
    input: OrdinaryPlacementInput,
) -> Result<OrdinaryPlacementAllocation, PlacementAllocationError> {
    if input.requested_count < 1 {
        return Err(PlacementAllocationError::InvalidRequestedCount(
            input.requested_count,
        ));
    }
    if !(4..=14).contains(&input.rare_id) {
        return Err(PlacementAllocationError::InvalidRareId(input.rare_id));
    }

    let (primary_type, follower_type) = match input.channel_source {
        PlacementChannelSource::OwnedRomfs1130 => (Some(1), Some(0)),
        PlacementChannelSource::EffectiveInfoTypes { primary, follower } => (primary, follower),
    };
    let primary_channel = match input.rare_id {
        4 | 8 => 2,
        5 | 6 | 13 => 0,
        _ => channel_for_type(primary_type),
    };
    let follower_channel = channel_for_type(follower_type);

    let effective_count = if input.rare_id == 11 {
        1
    } else {
        input.requested_count
    };
    let primary_reserved = input.primary_cap > input.primary_active_before_create;
    let primary_count = i32::from(primary_reserved);
    let mut channel_counts = [0_i32; 3];
    channel_counts[usize::from(primary_channel)] =
        channel_counts[usize::from(primary_channel)].wrapping_add(primary_count);

    let remaining = effective_count - primary_count;
    let follower_reserved = if remaining > 0 {
        let available = input
            .follower_cap
            .wrapping_sub(input.follower_active_before_create)
            .max(0);
        remaining.min(input.follower_budget).min(available)
    } else {
        0
    };
    channel_counts[usize::from(follower_channel)] =
        channel_counts[usize::from(follower_channel)].wrapping_add(follower_reserved);

    Ok(OrdinaryPlacementAllocation {
        effective_count,
        primary_reserved,
        follower_reserved,
        channel_counts,
        primary_channel,
        follower_channel,
    })
}

fn channel_for_type(info_type: Option<u32>) -> u8 {
    u8::from(info_type != Some(1))
}
