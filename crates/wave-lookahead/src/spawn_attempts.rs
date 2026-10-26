//! Conditional creation-call counts after an Event 0 ordinary queue item.
//!
//! This models calls to 11.3.0 `0x375d2a0`, not successful actor registration.
//! Species and spot selection have already advanced the shared RNG. The caller
//! supplies live counts, caps, gate results, and the follower count after the
//! optional selected-ID creation call. Successful registration updates active counts
//! synchronously; the same scheduler call can process another item before a
//! full recount.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttemptInput {
    /// The level-row request `1 + RareFollowerNum` for an ordinary item.
    pub requested_count: i32,
    pub selected_rare_id: u8,
    /// Director `+0x450`, before the item.
    pub active_rare_total: i32,
    /// Director `+0x44c`, before the item.
    pub active_total: i32,
    /// The item-specific global active limit at item `+0xc`.
    pub item_total_limit: i32,
    /// Return value of the species-specific or generic spot selection.
    pub spot_succeeded: bool,
    /// Result of the conditional virtual gate after spot selection.
    pub post_spot_allows: bool,
    pub selected_rare_cap: i32,
    pub selected_rare_active: i32,
    pub follower_cap: i32,
    /// Fresh ID 0/1 count read after the possible Rare creation call.
    pub follower_active_after_rare: i32,
    /// Director `+0x448`, read after the possible Rare creation call.
    pub follower_budget: i32,
    /// Current scene event/mode field `+0x5a4`; value 7 selects follower ID 1.
    pub scene_mode: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttemptBlock {
    RareGlobalLimit,
    TotalGlobalLimit,
    GoldiePreSpotCapacity,
    RequestCount,
    SpotFailed,
    PostSpotVeto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttemptPlan {
    Skipped(AttemptBlock),
    Calls {
        /// ID11 Pillar forces this to one; all other Rare IDs retain N.
        effective_budget: u32,
        rare_create_call: bool,
        follower_id: u8,
        follower_create_calls: u32,
    },
}

/// Calls reachable from the forced Event 0 ID 15 branch. ID 15 is EventRare,
/// not a member of the ordinary ID 4–14 Rare species scan. Followers are ID 0.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoldieAttemptPlan {
    Skipped(AttemptBlock),
    Calls {
        requested_count: u32,
        goldie_create_call: bool,
        follower_id: u8,
        follower_create_calls: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttemptPlanError {
    InvalidRequestedCount(i32),
    InvalidRareId(u8),
    InvalidSceneMode(u32),
}

/// Plan calls after the caller has established a forced Event 0/5 ID 15 path.
///
/// The pre-spot ID 15 capacity gate is applied by the stream before this
/// function. `selected_rare_*` refer to ID 15; `follower_*` refer to ID 0,
/// including the fresh active count after a possible ID 15 creation call.
/// A true `goldie_create_call` consumes one request slot even if creation or
/// registration fails. Successful ID 15 registration does not increase the
/// ordinary Rare-total count because ID 15 is EventRare category 2.
pub fn plan_event_zero_forced_goldie_attempts(
    input: AttemptInput,
) -> Result<GoldieAttemptPlan, AttemptPlanError> {
    if input.requested_count < 1 {
        return Err(AttemptPlanError::InvalidRequestedCount(
            input.requested_count,
        ));
    }
    if input.selected_rare_id != 15 {
        return Err(AttemptPlanError::InvalidRareId(input.selected_rare_id));
    }
    if input.active_total > input.item_total_limit {
        return Ok(GoldieAttemptPlan::Skipped(AttemptBlock::TotalGlobalLimit));
    }
    if !input.spot_succeeded {
        return Ok(GoldieAttemptPlan::Skipped(AttemptBlock::SpotFailed));
    }
    if !input.post_spot_allows {
        return Ok(GoldieAttemptPlan::Skipped(AttemptBlock::PostSpotVeto));
    }
    if !matches!(input.scene_mode, 0 | 5) {
        return Err(AttemptPlanError::InvalidSceneMode(input.scene_mode));
    }

    let goldie_create_call = input.selected_rare_cap > input.selected_rare_active;
    let remaining_budget = input.requested_count - i32::from(goldie_create_call);
    let follower_available = input
        .follower_cap
        .wrapping_sub(input.follower_active_after_rare)
        .max(0);
    let follower_create_calls = remaining_budget
        .min(input.follower_budget)
        .min(follower_available)
        .max(0);
    Ok(GoldieAttemptPlan::Calls {
        requested_count: input.requested_count as u32,
        goldie_create_call,
        follower_id: 0,
        follower_create_calls: follower_create_calls as u32,
    })
}

/// Plan `0x375d2a0` calls for one already-drawn ordinary Rare queue item.
///
/// A creation call can fail before actor registration. A failed Rare call
/// still consumes one request slot. This result does not update active counts
/// or decide whether a later same-tick item sees a kill/death callback.
pub fn plan_ordinary_item_attempts(input: AttemptInput) -> Result<AttemptPlan, AttemptPlanError> {
    if input.requested_count < 1 {
        return Err(AttemptPlanError::InvalidRequestedCount(
            input.requested_count,
        ));
    }
    if !(4..=14).contains(&input.selected_rare_id) {
        return Err(AttemptPlanError::InvalidRareId(input.selected_rare_id));
    }
    if input.active_rare_total >= 15 {
        return Ok(AttemptPlan::Skipped(AttemptBlock::RareGlobalLimit));
    }
    if input.active_total > input.item_total_limit {
        return Ok(AttemptPlan::Skipped(AttemptBlock::TotalGlobalLimit));
    }
    if !input.spot_succeeded {
        return Ok(AttemptPlan::Skipped(AttemptBlock::SpotFailed));
    }
    if !input.post_spot_allows {
        return Ok(AttemptPlan::Skipped(AttemptBlock::PostSpotVeto));
    }

    let effective_budget = if input.selected_rare_id == 11 {
        1
    } else {
        input.requested_count
    };
    let rare_create_call = input.selected_rare_cap > input.selected_rare_active;
    let remaining_budget = effective_budget - i32::from(rare_create_call);
    let follower_available = input
        .follower_cap
        .wrapping_sub(input.follower_active_after_rare)
        .max(0);
    let follower_create_calls = remaining_budget
        .min(input.follower_budget)
        .min(follower_available)
        .max(0);
    Ok(AttemptPlan::Calls {
        effective_budget: effective_budget as u32,
        rare_create_call,
        follower_id: u8::from(input.scene_mode == 7),
        follower_create_calls: follower_create_calls as u32,
    })
}
