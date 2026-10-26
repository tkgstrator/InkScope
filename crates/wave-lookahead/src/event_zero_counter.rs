//! Event 0/5/6 ordinary due-item `director+0x4d4` transition.
//!
//! The periodic predicate reads the pre-item counter. A later early or late
//! increment is visible to the next item, not to that predicate.

use crate::{
    ordinary_stream::{DueDraw, EventZeroDueContext, ForcedGoldieSpotGate, SpotDraw},
    species::{RareSpeciesSelection, UNSPECIFIED_BOSS_ID, periodic_goldie_source},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CounterPhase {
    EarlyRareLimit,
    LatePostSpot,
    Unchanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CounterAdvance {
    pub before: i32,
    pub after: i32,
    pub phase: CounterPhase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostSpotOutcome {
    SpotFailed,
    Vetoed,
    Allowed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CounterProjectionError {
    InconsistentSpeciesId,
    InconsistentDrawBranch,
    UnexpectedPostSpotOutcome,
    MissingPostSpotOutcome,
    ImpossibleSpotSuccess,
}

/// Project the counter after one already-drawn ordinary Rare queue item.
///
/// `post_spot` is needed only on the creation-capable path after a spot draw.
/// Early Rare-total and over-total paths still draw a spot but ignore its
/// return value. This function does not infer registration or death outcomes.
/// It validates this item's event branch only; a stream additionally checks
/// that a nonnull runtime event matches its configured queue event.
pub fn project_event_zero_accepted_counter(
    context: EventZeroDueContext,
    draw: DueDraw,
    post_spot: Option<PostSpotOutcome>,
) -> Result<CounterAdvance, CounterProjectionError> {
    if draw.selection.species_id() != draw.species_id {
        return Err(CounterProjectionError::InconsistentSpeciesId);
    }
    if !draw_branch_matches(context, draw) {
        return Err(CounterProjectionError::InconsistentDrawBranch);
    }
    let before = context.periodic.accepted_rare_counter;
    let unchanged = CounterAdvance {
        before,
        after: before,
        phase: CounterPhase::Unchanged,
    };
    let forced = matches!(draw.selection, RareSpeciesSelection::ForcedGoldie(_));
    if !forced && context.runtime.active_rare_total >= 15 {
        if post_spot.is_some() {
            return Err(CounterProjectionError::UnexpectedPostSpotOutcome);
        }
        return Ok(CounterAdvance {
            before,
            after: before.wrapping_add(1),
            phase: CounterPhase::EarlyRareLimit,
        });
    }
    if context.runtime.active_total > context.runtime.item_total_limit
        || draw.species_id == UNSPECIFIED_BOSS_ID
        || matches!(
            draw.spot,
            SpotDraw::BlockedBeforeSpot | SpotDraw::BlockedByRequestCount
        )
    {
        if post_spot.is_some() {
            return Err(CounterProjectionError::UnexpectedPostSpotOutcome);
        }
        return Ok(unchanged);
    }
    let post_spot = post_spot.ok_or(CounterProjectionError::MissingPostSpotOutcome)?;
    if matches!(draw.spot, SpotDraw::Generic { index: None, .. })
        && post_spot != PostSpotOutcome::SpotFailed
    {
        return Err(CounterProjectionError::ImpossibleSpotSuccess);
    }
    if post_spot == PostSpotOutcome::Allowed {
        Ok(CounterAdvance {
            before,
            after: before.wrapping_add(1),
            phase: CounterPhase::LatePostSpot,
        })
    } else {
        Ok(unchanged)
    }
}

fn draw_branch_matches(context: EventZeroDueContext, draw: DueDraw) -> bool {
    if !matches!(context.periodic.event, Some(0 | 5 | 6) | None) {
        return false;
    }
    let expected_force = match periodic_goldie_source(context.periodic) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let forced = match draw.selection {
        RareSpeciesSelection::ForcedGoldie(source) => {
            if expected_force != Some(source) {
                return false;
            }
            true
        },
        RareSpeciesSelection::Normal(id) => {
            if expected_force.is_some() || (id != UNSPECIFIED_BOSS_ID && !(4..=14).contains(&id)) {
                return false;
            }
            false
        },
    };
    let over_total = context.runtime.active_total > context.runtime.item_total_limit;
    let early_rare_limit = !forced && context.runtime.active_rare_total >= 15;
    let capacity_blocked = if forced {
        match (over_total, context.forced_goldie_spot) {
            (true, None | Some(ForcedGoldieSpotGate::OverTotalLimit)) => false,
            (
                false,
                Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
                    id_fifteen_capacity_available,
                }),
            ) => !id_fifteen_capacity_available,
            _ => return false,
        }
    } else {
        false
    };
    if capacity_blocked {
        return draw.spot == SpotDraw::BlockedBeforeSpot;
    }
    if draw.species_id == UNSPECIFIED_BOSS_ID {
        return draw.spot == SpotDraw::NoSpecies;
    }
    if !early_rare_limit && !over_total && context.runtime.requested_count < 1 {
        return draw.spot == SpotDraw::BlockedByRequestCount;
    }
    matches!(
        draw.spot,
        SpotDraw::Generic { .. } | SpotDraw::HandledByCallback
    )
}
