//! Conditional Event 0 direction and common-Zako request-point stream.
//!
//! One scheduler call updates the held direction, drains all due Rare items,
//! then enters the Zako timer. Rare creation requests and common-Zako spots
//! use director `+0x38c`, separate from the shared Rare/direction `+0x37c`.
//! This module counts the directly observed draws without computing a Zako
//! species. Runtime gates, candidate order, callback behavior, and population
//! remain explicit inputs. A point is a creation request, not a registered
//! enemy or final actor coordinate.

use crate::{
    direction::{DirectionError, predict_initial_directions},
    event_zero_counter::PostSpotOutcome,
    event_zero_runner::{
        CreationReceipt, EventZeroPopulation, EventZeroRunner, EventZeroTraceItem,
        LateAttemptState, RunnerStateError, RunnerTraceError,
    },
    ordinary_stream::{
        ClockAdvance, CounterTracedDue, OrdinaryStream, OrdinaryStreamError, PlannedDue, SpotRoute,
    },
    rng::XorShift128,
    rush_location::{RushSelectorInput, RushTimerGates},
    schedule::ScheduleInput,
    spawn_attempts::{AttemptPlan, GoldieAttemptPlan},
    zako_spot::{
        ModeZeroZakoSpotInput, ZakoSpotError, ZakoSpotOutcome, ZakoSpotTrace,
        advance_zako_selector_inline_draws, predict_mode_zero_zako_spot,
    },
    zako_timer::{ZakoTimerError, ZakoTimerInput, ZakoTimerStep, step_common_zako_timer},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PopulationBasis {
    Observed,
    Idealized,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PopulationSnapshot {
    pub population: EventZeroPopulation,
    pub basis: PopulationBasis,
}

/// Provenance for treating planned creation calls as actual entries to the
/// Event 0 request initializer. Indirect callback draws remain unmodeled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RareRequestPathBasis {
    /// Assume caps and relevant state remain stable across every planned call.
    AssumedStableCaps,
    /// A live trace confirmed each planned call reached request initialization.
    ObservedMatchingPlan,
}

pub struct EventZeroLocationStart<'a> {
    pub schedule: ScheduleInput,
    pub stage_keys: &'a [u8],
    pub accepted_counter: i32,
    pub population: EventZeroPopulation,
    pub population_basis: PopulationBasis,
    pub initial_timer: i32,
    pub rare_request_path_basis: RareRequestPathBasis,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectorRngBoundary {
    /// Continue from direct draws calculated from the scene seed.
    Predicted,
    /// Reject a mismatch while retaining all local state for a corrected
    /// observation or an explicit observed-state rebase.
    Verify([u32; 4]),
    /// Use a live `director+0x38c` snapshot after Rare work. This changes
    /// provenance for this and later calls to `ObservedRebased`.
    UseObserved([u32; 4]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectorRngOrigin {
    /// Seed-derived direct draws, conditional on no unmodeled indirect draws.
    SeedDerivedConditional,
    /// A caller-supplied live `+0x38c` observation established the base.
    ObservedRebased,
}

pub struct EventZeroFinishInput<'a> {
    pub timer: RushTimerGates,
    /// Required iff the timer reaches the Zako selector.
    pub selector: Option<RushSelectorInput<'a>>,
    pub rng_boundary: DirectorRngBoundary,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventZeroLocationCallTrace {
    pub call_index: usize,
    pub elapsed_tick: i32,
    /// Held game group number, not a compass bearing or confirmed spawn.
    pub held_direction: Option<u8>,
    pub direction_branch_invoked: bool,
    pub due_rare_items: usize,
    pub rare_request_draws: u64,
    pub rare_request_path_basis: RareRequestPathBasis,
    pub population_basis_before: PopulationBasis,
    pub population_basis_after_rare: PopulationBasis,
    pub shared_state_after_rare: [u32; 4],
    /// Calculated direct state before an optional observed-state rebase.
    pub rng_after_rare: [u32; 4],
    pub rng_origin: DirectorRngOrigin,
    pub timer: ZakoTimerStep,
    pub rng_after_selector: Option<[u32; 4]>,
    pub spot: Option<ZakoSpotTrace>,
    /// Direct request-initializer output after a nonnull selected point.
    pub request_draw: Option<u32>,
    pub rng_after_direct_path: [u32; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventZeroLocationError {
    UnsupportedEvent(u8),
    Stream(OrdinaryStreamError),
    Direction(DirectionError),
    Runner(RunnerStateError),
    CallAlreadyActive,
    NoActiveCall,
    QueuePathNotConfirmed,
    AuxiliaryPathNotConfirmed,
    DecreasingElapsedTick {
        previous: i32,
        current: i32,
    },
    NeedsPostZakoPopulation,
    PopulationDuringCall,
    NoRarePopulationDivergence,
    PendingRareItems(usize),
    PendingRareReceipt,
    NeedsRarePopulationReconciliation,
    ObservedRngMismatch {
        predicted: [u32; 4],
        observed: [u32; 4],
    },
    Timer(ZakoTimerError),
    MissingSelector,
    UnexpectedSelector,
    SelectorSuccessWithoutEligibleVisit,
    Spot(ZakoSpotError),
}

#[derive(Clone, Debug, PartialEq)]
pub enum EventZeroLocationTraceError<E> {
    NoActiveCall,
    Rare(RunnerTraceError<E>),
}

#[derive(Clone, Copy, Debug)]
struct ActiveCall {
    elapsed_tick: i32,
    clock: ClockAdvance,
    rare_request_draws: u64,
    population_basis_before: PopulationBasis,
}

pub struct EventZeroLocationRunner {
    rare: EventZeroRunner,
    director_rng: XorShift128,
    rng_origin: DirectorRngOrigin,
    timer: i32,
    rare_request_path_basis: RareRequestPathBasis,
    population_basis: PopulationBasis,
    active: Option<ActiveCall>,
    needs_post_zako_population: bool,
    next_call_index: usize,
    last_elapsed_tick: Option<i32>,
}

impl EventZeroLocationRunner {
    /// Construct the ordinary Event 0 path from one confirmed scene seed.
    pub fn new(input: EventZeroLocationStart<'_>) -> Result<Self, EventZeroLocationError> {
        if input.schedule.event_type != 0 {
            return Err(EventZeroLocationError::UnsupportedEvent(
                input.schedule.event_type,
            ));
        }
        let stream = OrdinaryStream::new(input.schedule, input.stage_keys)
            .map_err(EventZeroLocationError::Stream)?;
        let initial = predict_initial_directions(input.schedule.seed, input.stage_keys)
            .map_err(EventZeroLocationError::Direction)?;
        Ok(Self {
            rare: EventZeroRunner::new(stream, input.accepted_counter, input.population),
            director_rng: initial.director_rng_38c,
            rng_origin: DirectorRngOrigin::SeedDerivedConditional,
            timer: input.initial_timer,
            rare_request_path_basis: input.rare_request_path_basis,
            population_basis: input.population_basis,
            active: None,
            needs_post_zako_population: false,
            next_call_index: 0,
            last_elapsed_tick: None,
        })
    }

    pub fn rare(&self) -> &EventZeroRunner {
        &self.rare
    }

    pub fn director_rng_words(&self) -> [u32; 4] {
        self.director_rng.as_array()
    }

    pub fn timer(&self) -> i32 {
        self.timer
    }

    /// Start one actual scheduler call. The caller confirms its normal Rare
    /// queue path was not bypassed by runtime mode or another scheduler gate.
    /// The auxiliary step before the Rare queue must also be confirmed inert;
    /// it can otherwise create requests and advance director `+0x38c`.
    pub fn advance_clock(
        &mut self,
        elapsed_tick: i32,
        queue_path_confirmed: bool,
        auxiliary_path_inactive_confirmed: bool,
    ) -> Result<ClockAdvance, EventZeroLocationError> {
        if self.active.is_some() {
            return Err(EventZeroLocationError::CallAlreadyActive);
        }
        if self.needs_post_zako_population {
            return Err(EventZeroLocationError::NeedsPostZakoPopulation);
        }
        if !queue_path_confirmed {
            return Err(EventZeroLocationError::QueuePathNotConfirmed);
        }
        if !auxiliary_path_inactive_confirmed {
            return Err(EventZeroLocationError::AuxiliaryPathNotConfirmed);
        }
        if let Some(previous) = self.last_elapsed_tick
            && elapsed_tick < previous
        {
            return Err(EventZeroLocationError::DecreasingElapsedTick {
                previous,
                current: elapsed_tick,
            });
        }
        let clock = self
            .rare
            .advance_clock(elapsed_tick)
            .map_err(EventZeroLocationError::Runner)?;
        self.active = Some(ActiveCall {
            elapsed_tick,
            clock,
            rare_request_draws: 0,
            population_basis_before: self.population_basis,
        });
        self.last_elapsed_tick = Some(elapsed_tick);
        Ok(clock)
    }

    /// Trace one due Rare item. Callback providers must be read-only; their
    /// external side effects cannot be rolled back if this method errors.
    /// A caller that performed such side effects must discard this runner.
    pub fn trace_next<E, F, G, H>(
        &mut self,
        item: EventZeroTraceItem,
        normal_callbacks: Result<[bool; 11], E>,
        spot_route: F,
        post_spot: G,
        late_attempt: H,
    ) -> Result<CounterTracedDue, EventZeroLocationTraceError<E>>
    where
        F: FnOnce(u8, Option<u8>, XorShift128) -> Result<SpotRoute, E>,
        G: FnOnce(crate::ordinary_stream::DueDraw) -> Result<Option<PostSpotOutcome>, E>,
        H: FnOnce(crate::ordinary_stream::DueDraw) -> Result<LateAttemptState, E>,
    {
        let active = self
            .active
            .as_mut()
            .ok_or(EventZeroLocationTraceError::NoActiveCall)?;
        let traced = self
            .rare
            .trace_next(item, normal_callbacks, spot_route, post_spot, late_attempt)
            .map_err(EventZeroLocationTraceError::Rare)?;
        let direct_calls = request_init_calls(traced.due);
        for _ in 0..direct_calls {
            self.director_rng.next32();
        }
        active.rare_request_draws += direct_calls;
        Ok(traced)
    }

    pub fn submit_receipt(
        &mut self,
        receipt: CreationReceipt,
    ) -> Result<(), EventZeroLocationError> {
        if self.active.is_none() {
            return Err(EventZeroLocationError::NoActiveCall);
        }
        self.rare
            .submit_receipt(receipt)
            .map_err(EventZeroLocationError::Runner)
    }

    /// Only an exceptional observed mid-call divergence may be reconciled
    /// before all due Rare items complete. Ordinary deaths belong between
    /// scheduler calls, after `finish_call`.
    pub fn reconcile_rare_observed(
        &mut self,
        population: EventZeroPopulation,
    ) -> Result<(), EventZeroLocationError> {
        if self.active.is_none() {
            return Err(EventZeroLocationError::NoActiveCall);
        }
        if !self.rare.needs_reconciliation() {
            return Err(EventZeroLocationError::NoRarePopulationDivergence);
        }
        self.rare
            .reconcile_population(population)
            .map_err(EventZeroLocationError::Runner)?;
        self.population_basis = PopulationBasis::Observed;
        Ok(())
    }

    /// Install a full observed or explicitly idealized snapshot between calls.
    /// Required after a nonnull Zako request point before the next clock step.
    /// The accepted Rare counter is intentionally left unchanged.
    pub fn reconcile_between_calls(
        &mut self,
        snapshot: PopulationSnapshot,
    ) -> Result<(), EventZeroLocationError> {
        if self.active.is_some() {
            return Err(EventZeroLocationError::PopulationDuringCall);
        }
        self.rare
            .reconcile_population(snapshot.population)
            .map_err(EventZeroLocationError::Runner)?;
        self.population_basis = snapshot.basis;
        self.needs_post_zako_population = false;
        Ok(())
    }

    /// Finish after all due Rare items, using their held direction and their
    /// direct request-init draws before the common Zako timer. An observed RNG
    /// rebase is an explicit boundary input; verification mismatches do not
    /// commit timer, spot RNG, or call phase.
    pub fn finish_call(
        &mut self,
        input: EventZeroFinishInput<'_>,
    ) -> Result<EventZeroLocationCallTrace, EventZeroLocationError> {
        let active = self.active.ok_or(EventZeroLocationError::NoActiveCall)?;
        if self.rare.pending_receipt() {
            return Err(EventZeroLocationError::PendingRareReceipt);
        }
        let remaining = self.rare.stream().remaining_due_count();
        if remaining != 0 {
            return Err(EventZeroLocationError::PendingRareItems(remaining));
        }
        if self.rare.needs_reconciliation() {
            return Err(EventZeroLocationError::NeedsRarePopulationReconciliation);
        }

        let predicted_rng_after_rare = self.director_rng.as_array();
        let mut rng = self.director_rng;
        let mut rng_origin = self.rng_origin;
        match input.rng_boundary {
            DirectorRngBoundary::Predicted => {},
            DirectorRngBoundary::Verify(observed) => {
                if observed != predicted_rng_after_rare {
                    return Err(EventZeroLocationError::ObservedRngMismatch {
                        predicted: predicted_rng_after_rare,
                        observed,
                    });
                }
            },
            DirectorRngBoundary::UseObserved(observed) => {
                rng = XorShift128::from_words(observed);
                rng_origin = DirectorRngOrigin::ObservedRebased;
            },
        }
        let timer = step_common_zako_timer(ZakoTimerInput {
            timer_before: self.timer,
            pre_countdown_allows: input.timer.pre_countdown_allows,
            population_gate_allows: input.timer.population_gate_allows,
            reset_interval: input.timer.reset_interval,
            post_reset_allows: input.timer.post_reset_allows,
        })
        .map_err(EventZeroLocationError::Timer)?;
        let mut rng_after_selector = None;
        let mut spot = None;
        let mut request_draw = None;
        let mut needs_post_zako_population = false;
        if timer.spawn_path_entered {
            let selector = input
                .selector
                .ok_or(EventZeroLocationError::MissingSelector)?;
            if selector.selection_succeeded && selector.eligible_visit_count == 0 {
                return Err(EventZeroLocationError::SelectorSuccessWithoutEligibleVisit);
            }
            if let Some(observed) = selector.observed_rng_before_selector
                && observed != rng.as_array()
            {
                return Err(EventZeroLocationError::ObservedRngMismatch {
                    predicted: rng.as_array(),
                    observed,
                });
            }
            let after_selector =
                advance_zako_selector_inline_draws(rng.as_array(), selector.eligible_visit_count);
            rng_after_selector = Some(after_selector);
            let selected = predict_mode_zero_zako_spot(ModeZeroZakoSpotInput {
                timer_step: timer,
                species_selection_succeeded: selector.selection_succeeded,
                default_callback_route_confirmed: selector.default_callback_route_confirmed,
                applied_event: 0,
                applied_mode: selector.applied_mode,
                direction: active.clock.direction.map(i32::from).unwrap_or(-1),
                tide: selector.tide,
                rng_after_species: after_selector,
                runtime_groups: selector.runtime_groups,
                tree_30: selector.tree_30,
                tree_50: selector.tree_50,
                scratch_capacity: selector.scratch_capacity,
            })
            .map_err(EventZeroLocationError::Spot)?;
            rng = XorShift128::from_words(selected.rng_after_spot);
            if matches!(selected.outcome, ZakoSpotOutcome::SelectedPoint { .. }) {
                request_draw = Some(rng.next32());
                needs_post_zako_population = true;
            }
            spot = Some(selected);
        } else if input.selector.is_some() {
            return Err(EventZeroLocationError::UnexpectedSelector);
        }

        self.director_rng = rng;
        self.rng_origin = rng_origin;
        self.timer = timer.timer_after;
        self.needs_post_zako_population = needs_post_zako_population;
        self.active = None;
        let trace = EventZeroLocationCallTrace {
            call_index: self.next_call_index,
            elapsed_tick: active.elapsed_tick,
            held_direction: active.clock.direction,
            direction_branch_invoked: active.clock.direction_branch_invoked,
            due_rare_items: active.clock.due_count,
            rare_request_draws: active.rare_request_draws,
            rare_request_path_basis: self.rare_request_path_basis,
            population_basis_before: active.population_basis_before,
            population_basis_after_rare: self.population_basis,
            shared_state_after_rare: self.rare.stream().shared_state().as_array(),
            rng_after_rare: predicted_rng_after_rare,
            rng_origin,
            timer,
            rng_after_selector,
            spot,
            request_draw,
            rng_after_direct_path: rng.as_array(),
        };
        self.next_call_index += 1;
        Ok(trace)
    }
}

fn request_init_calls(due: PlannedDue) -> u64 {
    match due {
        PlannedDue::NoSpecies(_) => 0,
        PlannedDue::Selected {
            plan:
                AttemptPlan::Calls {
                    rare_create_call,
                    follower_create_calls,
                    ..
                },
            ..
        } => u64::from(rare_create_call) + u64::from(follower_create_calls),
        PlannedDue::ForcedGoldie {
            plan:
                GoldieAttemptPlan::Calls {
                    goldie_create_call,
                    follower_create_calls,
                    ..
                },
            ..
        } => u64::from(goldie_create_call) + u64::from(follower_create_calls),
        PlannedDue::Selected {
            plan: AttemptPlan::Skipped(_),
            ..
        }
        | PlannedDue::ForcedGoldie {
            plan: GoldieAttemptPlan::Skipped(_),
            ..
        } => 0,
    }
}
