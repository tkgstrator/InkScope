//! Conditional Rush direction and common-Zako request-point stream.
//!
//! The caller supplies every scheduler call and the live gates and spot
//! candidates reached in that call. This module counts directly observed
//! `director+0x38c` draws without calculating a Zako species. Continuity from
//! the seed assumes no intervening or indirect draws from that same state.
//! A selected point is a creation-request transform, not a registered enemy.

use crate::{
    direction::predict_initial_directions,
    rng::{SeedWords, XorShift128},
    rush_direction::{RushDirectionCall, RushDirectionError, trace_rush_directions},
    zako_spot::{
        ModeZeroZakoSpotInput, RuntimeZakoSpot, ZakoSpotError, ZakoSpotOutcome, ZakoSpotTrace,
        advance_zako_selector_inline_draws, predict_mode_zero_zako_spot,
    },
    zako_timer::{ZakoTimerError, ZakoTimerInput, ZakoTimerStep, step_common_zako_timer},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RushTimerGates {
    pub pre_countdown_allows: bool,
    pub population_gate_allows: Option<bool>,
    pub reset_interval: Option<i32>,
    pub post_reset_allows: Option<bool>,
}

/// Per-call runtime values reached only after the common timer allows it.
#[derive(Clone, Copy, Debug)]
pub struct RushSelectorInput<'a> {
    /// Optional live check at the boundary before selector draws.
    pub observed_rng_before_selector: Option<[u32; 4]>,
    /// Number of eligible iterator visits, without identifying the chosen ID.
    /// Validate this against a bounded live iterator snapshot before calling.
    pub eligible_visit_count: u32,
    pub selection_succeeded: bool,
    pub default_callback_route_confirmed: bool,
    pub applied_mode: i32,
    pub tide: i32,
    /// The nine runtime groups, retaining missing-group versus empty-group.
    pub runtime_groups: [Option<&'a [Option<RuntimeZakoSpot>]>; 9],
    pub tree_30: &'a [Option<RuntimeZakoSpot>],
    pub tree_50: &'a [Option<RuntimeZakoSpot>],
    pub scratch_capacity: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct RushLocationCallInput<'a> {
    pub elapsed_tick: i32,
    pub timer: RushTimerGates,
    /// Required iff this call's timer enters the selector path.
    pub selector: Option<RushSelectorInput<'a>>,
}

pub struct RushLocationInput<'a> {
    pub seed: SeedWords,
    pub stage_keys: &'a [u8],
    pub wave_change_num: i32,
    /// Queue `+0x44` before the first supplied scheduler call.
    pub initial_timer: i32,
    /// Complete, ordered scheduler-call snapshots from this WAVE's start.
    pub calls: &'a [RushLocationCallInput<'a>],
}

#[derive(Clone, Debug, PartialEq)]
pub struct RushLocationCallTrace {
    pub direction: RushDirectionCall,
    pub timer: ZakoTimerStep,
    /// State after the selector's directly observed eligible-visit draws.
    pub rng_after_selector: Option<[u32; 4]>,
    /// Present only when the timer enters the selector path.
    pub spot: Option<ZakoSpotTrace>,
    /// The direct creation-request draw after a nonnull selected point.
    pub request_draw: Option<u32>,
    /// State after directly observed work in this call, not after unknown
    /// indirect callbacks or actor registration.
    pub rng_after_direct_path: [u32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct RushLocationTrace {
    pub wave_start: Option<u8>,
    pub after_queue_build: Option<u8>,
    pub calls: Vec<RushLocationCallTrace>,
    pub final_timer: i32,
    pub final_rng_after_direct_path: [u32; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RushLocationError {
    Direction(RushDirectionError),
    Timer {
        call_index: usize,
        cause: ZakoTimerError,
    },
    MissingSelector {
        call_index: usize,
    },
    SelectorSuccessWithoutEligibleVisit {
        call_index: usize,
    },
    UnexpectedSelector {
        call_index: usize,
    },
    ObservedRngMismatch {
        call_index: usize,
        predicted: [u32; 4],
        observed: [u32; 4],
    },
    Spot {
        call_index: usize,
        cause: ZakoSpotError,
    },
}

/// Trace Event 1 Rush through direction, timer, and common-Zako location draws.
///
/// The returned location is a selected runtime point record. Correctness of
/// later calls requires no unmodeled changes to the shared direction stream,
/// the `+0x38c` spot stream, timer, or runtime candidate snapshots between
/// supplied calls. An observed pre-selector state can detect a mismatch at
/// that boundary. The caller supplies actual per-call gate and selector
/// outcomes and confirms that scene mode remains Event 1 through the request;
/// no Zako species identity or registration is predicted here.
pub fn trace_rush_locations(
    input: RushLocationInput<'_>,
) -> Result<RushLocationTrace, RushLocationError> {
    let ticks: Vec<i32> = input.calls.iter().map(|call| call.elapsed_tick).collect();
    let directions =
        trace_rush_directions(input.seed, input.stage_keys, input.wave_change_num, &ticks)
            .map_err(RushLocationError::Direction)?;
    let initial = predict_initial_directions(input.seed, input.stage_keys)
        .map_err(|error| RushLocationError::Direction(RushDirectionError::Direction(error)))?;
    let mut rng = initial.director_rng_38c.as_array();
    let mut timer = input.initial_timer;
    let mut calls = Vec::with_capacity(input.calls.len());

    for (call_index, (call, direction)) in input.calls.iter().zip(directions.calls).enumerate() {
        let timer_step = step_common_zako_timer(ZakoTimerInput {
            timer_before: timer,
            pre_countdown_allows: call.timer.pre_countdown_allows,
            population_gate_allows: call.timer.population_gate_allows,
            reset_interval: call.timer.reset_interval,
            post_reset_allows: call.timer.post_reset_allows,
        })
        .map_err(|cause| RushLocationError::Timer { call_index, cause })?;
        timer = timer_step.timer_after;

        let mut rng_after_selector = None;
        let mut spot = None;
        let mut request_draw = None;
        if timer_step.spawn_path_entered {
            let snapshot = call
                .selector
                .ok_or(RushLocationError::MissingSelector { call_index })?;
            if snapshot.selection_succeeded && snapshot.eligible_visit_count == 0 {
                return Err(RushLocationError::SelectorSuccessWithoutEligibleVisit { call_index });
            }
            if let Some(observed) = snapshot.observed_rng_before_selector
                && observed != rng
            {
                return Err(RushLocationError::ObservedRngMismatch {
                    call_index,
                    predicted: rng,
                    observed,
                });
            }
            let after_selector =
                advance_zako_selector_inline_draws(rng, snapshot.eligible_visit_count);
            rng_after_selector = Some(after_selector);
            let selected = predict_mode_zero_zako_spot(ModeZeroZakoSpotInput {
                timer_step,
                species_selection_succeeded: snapshot.selection_succeeded,
                default_callback_route_confirmed: snapshot.default_callback_route_confirmed,
                applied_event: 1,
                applied_mode: snapshot.applied_mode,
                direction: direction.direction.map(i32::from).unwrap_or(-1),
                tide: snapshot.tide,
                rng_after_species: after_selector,
                runtime_groups: snapshot.runtime_groups,
                tree_30: snapshot.tree_30,
                tree_50: snapshot.tree_50,
                scratch_capacity: snapshot.scratch_capacity,
            })
            .map_err(|cause| RushLocationError::Spot { call_index, cause })?;
            rng = selected.rng_after_spot;
            if matches!(selected.outcome, ZakoSpotOutcome::SelectedPoint { .. }) {
                let mut request_rng = XorShift128::from_words(rng);
                request_draw = Some(request_rng.next32());
                rng = request_rng.as_array();
            }
            spot = Some(selected);
        } else if call.selector.is_some() {
            return Err(RushLocationError::UnexpectedSelector { call_index });
        }
        calls.push(RushLocationCallTrace {
            direction,
            timer: timer_step,
            rng_after_selector,
            spot,
            request_draw,
            rng_after_direct_path: rng,
        });
    }

    Ok(RushLocationTrace {
        wave_start: directions.wave_start,
        after_queue_build: directions.after_queue_build,
        calls,
        final_timer: timer,
        final_rng_after_direct_path: rng,
    })
}
