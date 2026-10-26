//! Follower budget written by a completed 11.3.0 director update.
//!
//! The caller supplies `director+0x444` at that update and the applied event
//! and tide-selected effective water configuration. This calculation does not
//! establish whether the update ran before a particular queue item.

use crate::levels::RoundPopulationLimits;

/// Coefficient in the owned 11.3.0 common and present stage Water resources.
/// Runtime replacement or override resources may have a different value.
pub const OWNED_1130_WATER_ENEMY_ACTIVE_MAX_COEF: i32 = 100;

/// Inputs to the scheduler's guarded assignment to director `+0x444`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectorCapAssignmentInput {
    /// Value immediately before this assignment, after any earlier side effects.
    pub previous_cap: i32,
    /// Both the effective Round and event configuration pointers are present.
    pub effective_config_available: bool,
    pub event_enemy_active_max: i32,
    /// Signed elapsed time at director `+0x30`.
    pub director_time: i32,
    /// Signed halfway threshold at director `+0x38`.
    pub half_time: i32,
    /// Effective Round caps, potentially different from the owned common row.
    pub round: RoundPopulationLimits,
}

/// Value of director `+0x444` after its guarded scheduler assignment.
///
/// The scheduler may have earlier side effects, and `+0x448` is updated by a
/// separate director function. Neither their call order nor the first tick0
/// item's budget is determined by this pure calculation.
pub fn director_cap_after_scheduler_assignment(input: DirectorCapAssignmentInput) -> i32 {
    if !input.effective_config_available {
        return input.previous_cap;
    }
    if input.event_enemy_active_max >= 0 {
        return input.event_enemy_active_max;
    }
    if input.director_time < input.half_time {
        input.round.first_half
    } else {
        input.round.second_half
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FollowerBudgetScene {
    Absent,
    Present {
        /// Raw 32-bit applied scene event ID; only 4 uses the director cap.
        applied_event_id: u32,
        event_enemy_active_max: i32,
        event_enemy_active_max_coef: i32,
        water_enemy_active_max_coef: i32,
    },
}

/// Calculate the value written to `director+0x448` by this update.
pub fn follower_budget_after_update(director_cap_444: i32, scene: FollowerBudgetScene) -> i32 {
    let FollowerBudgetScene::Present {
        applied_event_id,
        event_enemy_active_max,
        event_enemy_active_max_coef,
        water_enemy_active_max_coef,
    } = scene
    else {
        return director_cap_444;
    };

    let event_cap = if event_enemy_active_max < 0 {
        director_cap_444
    } else {
        event_enemy_active_max
    };
    let cap = if applied_event_id == 4 {
        event_cap.min(director_cap_444)
    } else {
        event_cap
    };
    let coefficient = event_enemy_active_max_coef.min(water_enemy_active_max_coef);
    cap.wrapping_mul(coefficient) / 100
}
