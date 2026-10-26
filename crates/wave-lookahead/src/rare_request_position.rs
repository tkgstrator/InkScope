//! Conditional creation-request XYZ for the primary ordinary Rare enemy.
//!
//! Inputs must describe the same due item: the selected spawn actor in its
//! runtime candidate order and allocation counts immediately before creation.
//! A position here is a request to creation, not a registered actor location.

use crate::{
    event_zero_counter::CounterPhase,
    ordinary_stream::{CounterTracedDue, PlannedDue, SpotDraw},
    placement_allocation::{
        OrdinaryPlacementAllocation, OrdinaryPlacementInput, PlacementAllocationError,
        preallocate_ordinary_rare,
    },
    spawn_attempts::{AttemptBlock, AttemptPlan},
    spawn_position::{GenericRarePositionError, generic_rare_event_position},
    species::{RareSpeciesSelection, UNSPECIFIED_BOSS_ID},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceBasis {
    ObservedRuntime,
    AssumedIdealized,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RuntimeGenericSpot {
    pub candidate_count: u32,
    pub selected_index: u32,
    pub origin: [f32; 3],
    pub direction: [f32; 3],
    pub basis: EvidenceBasis,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlacementObservation {
    pub input: OrdinaryPlacementInput,
    pub basis: EvidenceBasis,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrimaryRequestPosition {
    pub species_id: u8,
    pub spot_index: u32,
    pub channel: u8,
    pub position: [f32; 3],
    pub allocation: OrdinaryPlacementAllocation,
    pub spot_basis: EvidenceBasis,
    pub allocation_basis: EvidenceBasis,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PrimaryRequestOutcome {
    NoSpecies,
    ForcedGoldie,
    Skipped(AttemptBlock),
    NoPrimaryCall,
    NeedsCallbackPosition,
    Conditional(PrimaryRequestPosition),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectionError {
    InconsistentTrace,
    MissingRuntimeSpot,
    RuntimeSpotMismatch,
    MissingPlacement,
    PlacementSpeciesMismatch,
    PlacementRequestCountMismatch,
    PlacementBudgetMismatch,
    PrimaryReservationMismatch,
    Placement(PlacementAllocationError),
    Position(GenericRarePositionError),
}

/// Resolve ordinal-zero XYZ only after the stateful trace reaches its
/// selected-ID creation call. The caller must establish that both observations
/// belong to this due item; this function checks their exposed consistency.
pub fn project_primary_rare_request(
    traced: CounterTracedDue,
    spot: Option<RuntimeGenericSpot>,
    placement: Option<PlacementObservation>,
) -> Result<PrimaryRequestOutcome, ProjectionError> {
    let expected_after = match traced.counter.phase {
        CounterPhase::Unchanged => traced.counter.before,
        CounterPhase::EarlyRareLimit | CounterPhase::LatePostSpot => {
            traced.counter.before.wrapping_add(1)
        },
    };
    if traced.counter.after != expected_after {
        return Err(ProjectionError::InconsistentTrace);
    }
    let (draw, plan) = match traced.due {
        PlannedDue::NoSpecies(draw) => {
            if draw.selection.species_id() != draw.species_id
                || draw.species_id != UNSPECIFIED_BOSS_ID
                || draw.spot != SpotDraw::NoSpecies
                || traced.counter.phase == CounterPhase::LatePostSpot
            {
                return Err(ProjectionError::InconsistentTrace);
            }
            return Ok(PrimaryRequestOutcome::NoSpecies);
        },
        PlannedDue::ForcedGoldie { draw, plan } => {
            if draw.species_id != 15
                || !matches!(draw.selection, RareSpeciesSelection::ForcedGoldie(_))
                || match plan {
                    crate::spawn_attempts::GoldieAttemptPlan::Skipped(_) => {
                        traced.counter.phase != CounterPhase::Unchanged
                    },
                    crate::spawn_attempts::GoldieAttemptPlan::Calls { .. } => {
                        traced.counter.phase != CounterPhase::LatePostSpot
                    },
                }
            {
                return Err(ProjectionError::InconsistentTrace);
            }
            return Ok(PrimaryRequestOutcome::ForcedGoldie);
        },
        PlannedDue::Selected { draw, plan } => (draw, plan),
    };
    if draw.selection.species_id() != draw.species_id
        || !(4..=14).contains(&draw.species_id)
        || !valid_spot_draw(draw.spot)
    {
        return Err(ProjectionError::InconsistentTrace);
    }
    match plan {
        AttemptPlan::Skipped(reason) => {
            let expected_phase = if reason == AttemptBlock::RareGlobalLimit {
                CounterPhase::EarlyRareLimit
            } else {
                CounterPhase::Unchanged
            };
            if traced.counter.phase != expected_phase
                || !valid_selected_skip(draw.spot, reason, draw.requested_count)
            {
                return Err(ProjectionError::InconsistentTrace);
            }
            return Ok(PrimaryRequestOutcome::Skipped(reason));
        },
        AttemptPlan::Calls {
            effective_budget,
            rare_create_call,
            ..
        } => {
            let expected_budget = if draw.species_id == 11 {
                1
            } else {
                draw.requested_count
            };
            if draw.requested_count < 1
                || u32::try_from(expected_budget).ok() != Some(effective_budget)
                || traced.counter.phase != CounterPhase::LatePostSpot
                || !successful_spot_draw(draw.spot)
            {
                return Err(ProjectionError::InconsistentTrace);
            }
            if !rare_create_call {
                return Ok(PrimaryRequestOutcome::NoPrimaryCall);
            }
        },
    }

    let (candidate_count, selected_index) = match draw.spot {
        SpotDraw::HandledByCallback => return Ok(PrimaryRequestOutcome::NeedsCallbackPosition),
        SpotDraw::Generic {
            candidate_count,
            index: Some(index),
        } if candidate_count > 0 && index < candidate_count => (candidate_count, index),
        _ => return Err(ProjectionError::InconsistentTrace),
    };
    let spot = spot.ok_or(ProjectionError::MissingRuntimeSpot)?;
    if spot.candidate_count != candidate_count || spot.selected_index != selected_index {
        return Err(ProjectionError::RuntimeSpotMismatch);
    }
    let placement = placement.ok_or(ProjectionError::MissingPlacement)?;
    if placement.input.rare_id != draw.species_id {
        return Err(ProjectionError::PlacementSpeciesMismatch);
    }
    if placement.input.requested_count != draw.requested_count {
        return Err(ProjectionError::PlacementRequestCountMismatch);
    }
    let allocation =
        preallocate_ordinary_rare(placement.input).map_err(ProjectionError::Placement)?;
    let expected_budget = if draw.species_id == 11 {
        1
    } else {
        draw.requested_count
    };
    if allocation.effective_count != expected_budget {
        return Err(ProjectionError::PlacementBudgetMismatch);
    }
    if !allocation.primary_reserved {
        return Err(ProjectionError::PrimaryReservationMismatch);
    }
    let position = generic_rare_event_position(
        spot.origin,
        spot.direction,
        allocation.primary_channel,
        [allocation.channel_counts[0], allocation.channel_counts[1]],
        0,
    )
    .map_err(ProjectionError::Position)?;
    Ok(PrimaryRequestOutcome::Conditional(PrimaryRequestPosition {
        species_id: draw.species_id,
        spot_index: selected_index,
        channel: allocation.primary_channel,
        position,
        allocation,
        spot_basis: spot.basis,
        allocation_basis: placement.basis,
    }))
}

fn valid_spot_draw(spot: SpotDraw) -> bool {
    match spot {
        SpotDraw::Generic {
            candidate_count,
            index,
        } => match index {
            Some(index) => index < candidate_count,
            None => candidate_count == 0,
        },
        _ => true,
    }
}

fn successful_spot_draw(spot: SpotDraw) -> bool {
    matches!(
        spot,
        SpotDraw::Generic { index: Some(_), .. } | SpotDraw::HandledByCallback
    )
}

fn valid_selected_skip(spot: SpotDraw, reason: AttemptBlock, requested_count: i32) -> bool {
    match reason {
        AttemptBlock::RareGlobalLimit | AttemptBlock::TotalGlobalLimit => {
            matches!(spot, SpotDraw::Generic { .. } | SpotDraw::HandledByCallback)
        },
        AttemptBlock::RequestCount => {
            requested_count < 1 && spot == SpotDraw::BlockedByRequestCount
        },
        AttemptBlock::SpotFailed => {
            matches!(spot, SpotDraw::Generic { .. } | SpotDraw::HandledByCallback)
        },
        AttemptBlock::PostSpotVeto => successful_spot_draw(spot),
        AttemptBlock::GoldiePreSpotCapacity => false,
    }
}
