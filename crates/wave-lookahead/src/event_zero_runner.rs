//! Stateful conditional Event 0/5/6 trace over the ordinary Rare queue.
//!
//! This layer carries the accepted counter and population between queue
//! items. External creation outcomes and recount/death boundaries remain
//! explicit caller inputs.
//! Fog results have the same untraced pre-periodic `+0x37c` condition as the
//! underlying ordinary stream.

use crate::{
    event_zero_counter::PostSpotOutcome,
    levels::{OrdinaryItemLimitInput, ordinary_item_runtime_limits},
    ordinary_stream::{
        ClockAdvance, CounterTracedDue, DueDraw, EventZeroDueContext, EventZeroItemRuntime,
        ForcedGoldieSpotGate, OrdinaryStream, OrdinaryStreamError, PlannedDue, SpotRoute,
        TraceError,
    },
    rng::XorShift128,
    spawn_attempts::{AttemptInput, AttemptPlan, GoldieAttemptPlan},
    species::{PeriodicGoldieInput, RarePopulation, rare_candidate_states},
};

/// Director active/reserved counts at a caller-established observation point.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EventZeroPopulation {
    pub active_by_id: [i32; 32],
    pub reserved_by_id: [i32; 32],
    pub active_total: i32,
    pub active_rare_total: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunnerStateError {
    PendingReceipt,
    NeedsReconciliation,
    NoPendingReceipt,
    SelectedWithoutCall,
    TooManyFollowerRegistrations { registered: u32, planned_calls: u32 },
    Stream(OrdinaryStreamError),
}

/// Inputs known at the start of one ordinary Rare due item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventZeroTraceItem {
    pub periodic_event: Option<u8>,
    pub global_cycle_flag: bool,
    /// Resolved Fog cycle, required when `periodic_event` is 5.
    pub fog_cycle: Option<i32>,
    pub event_none_cycle: Option<i32>,
    pub global_word: u32,
    pub limits: OrdinaryItemLimitInput,
    /// Needed only when the forced ID15 branch is within the item total limit.
    pub forced_id15_capacity_available: Option<bool>,
}

/// Creation-stage values read after the selected spot and virtual gate.
/// An offline caller supplies explicit assumptions for values it cannot observe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LateAttemptState {
    pub observed_active_total: i32,
    pub observed_active_rare_total: i32,
    pub selected_cap: i32,
    pub selected_active: i32,
    pub follower_cap: i32,
    pub follower_active_after_selected: i32,
    pub follower_budget: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CreationReceipt {
    pub selected_registered: bool,
    pub follower_registered: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunnerInputError<E> {
    External(E),
    MidItemPopulationChanged {
        expected_total: i32,
        observed_total: i32,
        expected_rare_total: i32,
        observed_rare_total: i32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunnerTraceError<E> {
    PendingReceipt,
    NeedsReconciliation,
    Trace(TraceError<RunnerInputError<E>>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingCreationCalls {
    selected_id: u8,
    selected_calls: u32,
    follower_id: u8,
    follower_calls: u32,
}

/// Owns the low-level RNG trace and the counts fed into each due item.
pub struct EventZeroRunner {
    stream: OrdinaryStream,
    accepted_counter: i32,
    population: EventZeroPopulation,
    pending: Option<PendingCreationCalls>,
    needs_reconciliation: bool,
}

impl EventZeroRunner {
    pub fn new(
        stream: OrdinaryStream,
        accepted_counter: i32,
        population: EventZeroPopulation,
    ) -> Self {
        Self {
            stream,
            accepted_counter,
            population,
            pending: None,
            needs_reconciliation: false,
        }
    }

    pub fn stream(&self) -> &OrdinaryStream {
        &self.stream
    }

    pub fn accepted_counter(&self) -> i32 {
        self.accepted_counter
    }

    pub fn population(&self) -> &EventZeroPopulation {
        &self.population
    }

    pub fn pending_receipt(&self) -> bool {
        self.pending.is_some()
    }

    pub fn needs_reconciliation(&self) -> bool {
        self.needs_reconciliation
    }

    pub fn advance_clock(&mut self, elapsed_tick: i32) -> Result<ClockAdvance, RunnerStateError> {
        if self.pending.is_some() {
            return Err(RunnerStateError::PendingReceipt);
        }
        if self.needs_reconciliation {
            return Err(RunnerStateError::NeedsReconciliation);
        }
        self.stream
            .advance_clock(elapsed_tick)
            .map_err(RunnerStateError::Stream)
    }

    pub fn reconcile_population(
        &mut self,
        snapshot: EventZeroPopulation,
    ) -> Result<(), RunnerStateError> {
        if self.pending.is_some() {
            return Err(RunnerStateError::PendingReceipt);
        }
        self.population = snapshot;
        self.needs_reconciliation = false;
        Ok(())
    }

    pub fn submit_receipt(&mut self, receipt: CreationReceipt) -> Result<(), RunnerStateError> {
        let pending = self.pending.ok_or(RunnerStateError::NoPendingReceipt)?;
        if receipt.selected_registered && pending.selected_calls == 0 {
            return Err(RunnerStateError::SelectedWithoutCall);
        }
        if receipt.follower_registered > pending.follower_calls {
            return Err(RunnerStateError::TooManyFollowerRegistrations {
                registered: receipt.follower_registered,
                planned_calls: pending.follower_calls,
            });
        }
        if receipt.selected_registered {
            let id = usize::from(pending.selected_id);
            self.population.active_by_id[id] = self.population.active_by_id[id].wrapping_add(1);
            self.population.active_total = self.population.active_total.wrapping_add(1);
            if (4..=14).contains(&pending.selected_id) {
                self.population.active_rare_total =
                    self.population.active_rare_total.wrapping_add(1);
            }
        }
        if receipt.follower_registered != 0 {
            let count = receipt.follower_registered as i32;
            let id = usize::from(pending.follower_id);
            self.population.active_by_id[id] = self.population.active_by_id[id].wrapping_add(count);
            self.population.active_total = self.population.active_total.wrapping_add(count);
        }
        self.pending = None;
        Ok(())
    }

    /// Trace one due item. Supplied normal callback decisions must not be
    /// gathered by eagerly running game callbacks out of their scan order.
    pub fn trace_next<E, F, G, H>(
        &mut self,
        item: EventZeroTraceItem,
        normal_callbacks: Result<[bool; 11], E>,
        spot_route: F,
        post_spot: G,
        late_attempt: H,
    ) -> Result<CounterTracedDue, RunnerTraceError<E>>
    where
        F: FnOnce(u8, Option<u8>, XorShift128) -> Result<SpotRoute, E>,
        G: FnOnce(DueDraw) -> Result<Option<PostSpotOutcome>, E>,
        H: FnOnce(DueDraw) -> Result<LateAttemptState, E>,
    {
        if self.pending.is_some() {
            return Err(RunnerTraceError::PendingReceipt);
        }
        if self.needs_reconciliation {
            return Err(RunnerTraceError::NeedsReconciliation);
        }
        let limits = ordinary_item_runtime_limits(item.limits);
        let before = self.population;
        let states = normal_callbacks
            .map(|callbacks| {
                let population = std::array::from_fn(|index| RarePopulation {
                    active: before.active_by_id[index + 4],
                    reserved: before.reserved_by_id[index + 4],
                });
                rare_candidate_states(item.periodic_event, item.global_word, population, callbacks)
            })
            .map_err(RunnerInputError::External);
        let total_over = before.active_total > limits.item_total_limit;
        let forced_goldie_spot = if total_over {
            Some(ForcedGoldieSpotGate::OverTotalLimit)
        } else {
            item.forced_id15_capacity_available.map(|available| {
                ForcedGoldieSpotGate::AtOrBelowTotalLimit {
                    id_fifteen_capacity_available: available,
                }
            })
        };
        let context = EventZeroDueContext {
            periodic: PeriodicGoldieInput {
                event: item.periodic_event,
                global_cycle_flag: item.global_cycle_flag,
                accepted_rare_counter: self.accepted_counter,
                fog_cycle: item.fog_cycle,
                event_none_cycle: item.event_none_cycle,
            },
            forced_goldie_spot,
            runtime: EventZeroItemRuntime {
                active_rare_total: before.active_rare_total,
                active_total: before.active_total,
                item_total_limit: limits.item_total_limit,
                requested_count: limits.requested_count,
            },
        };
        let mut late_count_difference = false;
        let traced = self
            .stream
            .trace_next_with_counter(
                context,
                states,
                |id, direction, rng| {
                    spot_route(id, direction, rng).map_err(RunnerInputError::External)
                },
                |draw| post_spot(draw).map_err(RunnerInputError::External),
                |draw| {
                    let late = late_attempt(draw).map_err(RunnerInputError::External)?;
                    if late.observed_active_total != before.active_total
                        || late.observed_active_rare_total != before.active_rare_total
                    {
                        return Err(RunnerInputError::MidItemPopulationChanged {
                            expected_total: before.active_total,
                            observed_total: late.observed_active_total,
                            expected_rare_total: before.active_rare_total,
                            observed_rare_total: late.observed_active_rare_total,
                        });
                    }
                    let follower_id: u8 = if item.limits.scene_mode == 7 { 1 } else { 0 };
                    late_count_difference = late.selected_active
                        != before.active_by_id[usize::from(draw.species_id)]
                        || late.follower_active_after_selected
                            != before.active_by_id[usize::from(follower_id)];
                    Ok(AttemptInput {
                        requested_count: limits.requested_count,
                        selected_rare_id: draw.species_id,
                        active_rare_total: before.active_rare_total,
                        active_total: before.active_total,
                        item_total_limit: limits.item_total_limit,
                        spot_succeeded: true,
                        post_spot_allows: true,
                        selected_rare_cap: late.selected_cap,
                        selected_rare_active: late.selected_active,
                        follower_cap: late.follower_cap,
                        follower_active_after_rare: late.follower_active_after_selected,
                        follower_budget: late.follower_budget,
                        scene_mode: item.limits.scene_mode,
                    })
                },
            )
            .map_err(RunnerTraceError::Trace)?;
        self.accepted_counter = traced.counter.after;
        self.needs_reconciliation = late_count_difference;
        self.pending = pending_from_due(traced.due);
        Ok(traced)
    }
}

fn pending_from_due(due: PlannedDue) -> Option<PendingCreationCalls> {
    let pending = match due {
        PlannedDue::NoSpecies(_) => return None,
        PlannedDue::Selected {
            draw,
            plan:
                AttemptPlan::Calls {
                    rare_create_call,
                    follower_id,
                    follower_create_calls,
                    ..
                },
        } => PendingCreationCalls {
            selected_id: draw.species_id,
            selected_calls: u32::from(rare_create_call),
            follower_id,
            follower_calls: follower_create_calls,
        },
        PlannedDue::ForcedGoldie {
            draw,
            plan:
                GoldieAttemptPlan::Calls {
                    goldie_create_call,
                    follower_id,
                    follower_create_calls,
                    ..
                },
        } => PendingCreationCalls {
            selected_id: draw.species_id,
            selected_calls: u32::from(goldie_create_call),
            follower_id,
            follower_calls: follower_create_calls,
        },
        PlannedDue::Selected {
            plan: AttemptPlan::Skipped(_),
            ..
        }
        | PlannedDue::ForcedGoldie {
            plan: GoldieAttemptPlan::Skipped(_),
            ..
        } => return None,
    };
    (pending.selected_calls != 0 || pending.follower_calls != 0).then_some(pending)
}
