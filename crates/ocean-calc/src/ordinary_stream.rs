//! Conditional shared-RNG trace for an ordinary EventType 0, 5, or 6 WAVE.
//!
//! This composes the confirmed queue, direction, periodic Goldie,
//! Rare species, and generic spot draw rules. The caller supplies fresh
//! runtime candidate states for each due item and a spot path after the
//! species is known. It assumes no
//! other code advances the shared direction/species/spot RNG between calls.
//! For Fog, this is conditional on the untraced pre-periodic callbacks leaving
//! director `+0x37c` unchanged; there is no mid-item RNG override here.
//! Results are draws, not confirmed spawned enemies or final coordinates.

use std::convert::Infallible;

use crate::{
    direction::{
        DirectionError, advance_direction_for_elapsed, direction_interval,
        predict_initial_directions,
    },
    event_zero_counter::{
        CounterAdvance, CounterProjectionError, PostSpotOutcome,
        project_event_zero_accepted_counter,
    },
    rng::XorShift128,
    schedule::{NominalEntry, ScheduleError, ScheduleInput, nominal_schedule},
    spawn_attempts::{
        AttemptBlock, AttemptInput, AttemptPlan, AttemptPlanError, GoldieAttemptPlan,
        plan_event_zero_forced_goldie_attempts, plan_ordinary_item_attempts,
    },
    species::{
        PeriodicGoldieError, PeriodicGoldieInput, RareCandidateState, RareSpeciesSelection,
        UNSPECIFIED_BOSS_ID, choose_event_rare_species, periodic_goldie_source,
    },
    spot::choose_generic_spot_index,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpotRoute {
    /// The species callback declined to pick a spot; the generic actor array
    /// has this many valid entries, in runtime enumeration order.
    Generic(u32),
    /// The species callback picked a spot and consumed no shared RNG draw.
    HandledByCallback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpotDraw {
    NoSpecies,
    /// Forced ID 15 failed its pre-spot cap/reserved gate.
    BlockedBeforeSpot,
    /// The creation-capable path had `item+8 < 1`, before spot selection.
    BlockedByRequestCount,
    Generic {
        candidate_count: u32,
        index: Option<u32>,
    },
    HandledByCallback,
}

/// The forced ID 15 branch after the periodic species predicate.
///
/// At or below the item's total-active limit, the caller must evaluate the
/// game's ID 15 cap/reserved availability lookup. Above that total limit,
/// the game still calls spot selection but does not create an actor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForcedGoldieSpotGate {
    OverTotalLimit,
    AtOrBelowTotalLimit { id_fifteen_capacity_available: bool },
}

/// Live inputs for one ordinary Rare due item. `periodic` is evaluated before any
/// early accepted-counter increment for this item. Such an increment changes
/// the value supplied for a later item, including one in the same call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventZeroDueContext {
    pub periodic: PeriodicGoldieInput,
    /// Required only if this item forces ID 15.
    pub forced_goldie_spot: Option<ForcedGoldieSpotGate>,
    /// Live values before this due item. These determine the processor's
    /// early Rare limit, total limit, and requested-count branches.
    pub runtime: EventZeroItemRuntime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventZeroItemRuntime {
    pub active_rare_total: i32,
    pub active_total: i32,
    pub item_total_limit: i32,
    pub requested_count: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockAdvance {
    pub direction: Option<u8>,
    pub direction_branch_invoked: bool,
    /// Number of entries due in this scheduler call. Call `draw_next` this
    /// many times before calling `advance_clock` again.
    pub due_count: usize,
    pub shared_state_after_direction: XorShift128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DueDraw {
    pub queue_index: usize,
    pub entry: NominalEntry,
    pub direction: Option<u8>,
    pub selection: RareSpeciesSelection,
    pub species_id: u8,
    /// Original item request count before any ID11 one-slot adjustment.
    pub requested_count: i32,
    pub spot: SpotDraw,
    pub shared_state_after: XorShift128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlannedDue {
    /// No Rare species was selected, so this item makes no creation call.
    NoSpecies(DueDraw),
    /// Forced Event 0/5 ID 15 has its own creation-call and ID 0 follower plan.
    ForcedGoldie {
        draw: DueDraw,
        plan: GoldieAttemptPlan,
    },
    /// Creation calls are conditional on the caller's runtime snapshot.
    Selected { draw: DueDraw, plan: AttemptPlan },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraceError<E> {
    Stream(OrdinaryStreamError),
    UnknownInput(E),
    Attempt(AttemptPlanError),
    Counter(CounterProjectionError),
    InconsistentAttemptInput,
    ImpossibleSpotSuccess,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CounterTracedDue {
    pub due: PlannedDue,
    pub counter: CounterAdvance,
}

enum PrepareError<E> {
    Stream(OrdinaryStreamError),
    UnknownInput(E),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrdinaryStreamError {
    Schedule(ScheduleError),
    Direction(DirectionError),
    NegativeElapsedTick(i32),
    PendingDueItems(usize),
    NoDueItem,
    InvalidCandidateId(u8),
    UnexpectedRuntimeEvent(u8),
    Periodic(PeriodicGoldieError),
    MissingForcedGoldieSpotGate,
    InconsistentForcedGoldieSpotGate,
}

/// Stateful conditional trace for one WAVE's nominal queue.
pub struct OrdinaryStream {
    configured_event: u8,
    entries: Vec<NominalEntry>,
    stage_keys: Vec<u8>,
    interval: i32,
    rng: XorShift128,
    wave_start_direction: Option<u8>,
    direction: Option<u8>,
    old_elapsed_tick: i32,
    next_queue_index: usize,
    pending_due_end: usize,
}

impl OrdinaryStream {
    /// Construct the JobType 0/1, EventType 0/5/6 queue and its two initial
    /// direction draws. The caller must establish that the game built this
    /// queue and its stage spawner exists.
    pub fn new(input: ScheduleInput, stage_keys: &[u8]) -> Result<Self, OrdinaryStreamError> {
        let entries = nominal_schedule(input).map_err(OrdinaryStreamError::Schedule)?;
        let initial = predict_initial_directions(input.seed, stage_keys)
            .map_err(OrdinaryStreamError::Direction)?;
        Ok(Self {
            configured_event: input.event_type,
            entries,
            stage_keys: stage_keys.to_vec(),
            interval: direction_interval(input.wave_change_num as i32),
            rng: initial.shared_state,
            wave_start_direction: initial.wave_start,
            direction: initial.after_queue_build,
            old_elapsed_tick: -1,
            next_queue_index: 0,
            pending_due_end: 0,
        })
    }

    pub fn configured_event(&self) -> u8 {
        self.configured_event
    }

    pub fn wave_start_direction(&self) -> Option<u8> {
        self.wave_start_direction
    }

    pub fn current_direction(&self) -> Option<u8> {
        self.direction
    }

    pub fn shared_state(&self) -> XorShift128 {
        self.rng
    }

    pub fn next_queue_index(&self) -> usize {
        self.next_queue_index
    }

    pub fn remaining_due_count(&self) -> usize {
        self.pending_due_end - self.next_queue_index
    }

    pub fn entries(&self) -> &[NominalEntry] {
        &self.entries
    }

    /// Advance one actual scheduler call to its clamped elapsed tick. A
    /// direction update occurs at most once before any due queue item.
    /// Calling this again before consuming every due item is an error.
    pub fn advance_clock(
        &mut self,
        elapsed_tick: i32,
    ) -> Result<ClockAdvance, OrdinaryStreamError> {
        if elapsed_tick < 0 {
            return Err(OrdinaryStreamError::NegativeElapsedTick(elapsed_tick));
        }
        if self.remaining_due_count() != 0 {
            return Err(OrdinaryStreamError::PendingDueItems(
                self.remaining_due_count(),
            ));
        }
        let due_count = self.entries[self.next_queue_index..]
            .iter()
            .take_while(|entry| entry.tick <= elapsed_tick as u32)
            .count();
        let advanced = advance_direction_for_elapsed(
            &mut self.rng,
            &self.stage_keys,
            self.direction,
            self.old_elapsed_tick,
            elapsed_tick,
            self.interval,
        )
        .map_err(OrdinaryStreamError::Direction)?;
        self.direction = advanced.direction;
        self.old_elapsed_tick = elapsed_tick;
        self.pending_due_end = self.next_queue_index + due_count;
        Ok(ClockAdvance {
            direction: self.direction,
            direction_branch_invoked: advanced.branch_invoked,
            due_count,
            shared_state_after_direction: self.rng,
        })
    }

    /// Select one due item's Rare species and then its spot route.
    ///
    /// Only IDs 4–14 may be non-Skip in the normal scan. Build these states
    /// from the latest active/pending counts and callback results, including
    /// changes caused by the previous item. The callback is invoked only if a
    /// species was selected and passed any pre-spot gate; it sees the shared
    /// RNG state after the species draw. It must
    /// report whether the species callback handled the spot without a shared
    /// draw or provide the current generic actor count. Runtime callback
    /// effects on other state remain the caller's responsibility.
    pub fn draw_next<F>(
        &mut self,
        context: EventZeroDueContext,
        candidate_states: [RareCandidateState; 32],
        spot_route: F,
    ) -> Result<DueDraw, OrdinaryStreamError>
    where
        F: FnOnce(u8, Option<u8>, XorShift128) -> SpotRoute,
    {
        let draw = match self.prepare_draw(
            context,
            Ok::<_, Infallible>(candidate_states),
            |id, direction, rng| Ok::<_, Infallible>(spot_route(id, direction, rng)),
        ) {
            Ok(draw) => draw,
            Err(PrepareError::Stream(error)) => return Err(error),
            Err(PrepareError::UnknownInput(never)) => match never {},
        };
        self.commit_draw(draw);
        Ok(draw)
    }

    /// Trace one due item's draws and conditional creation calls atomically.
    ///
    /// Unknown inputs needed by the chosen branch leave this stream's shared
    /// RNG and queue cursor unchanged. A forced ID 15 bypasses the normal
    /// candidate states but needs its own creation-call input after spot
    /// selection. The closures should
    /// only observe external state; their own side effects cannot be rolled
    /// back. A
    /// `Generic(Some(index))` draw does not prove that the selected runtime
    /// actor pointer is valid, so the final closure must provide the observed
    /// `spot_succeeded` value. For a normal ID 4–14, the selected species
    /// replaces the ID in its supplied `AttemptInput` before planning; a
    /// forced Goldie input must explicitly identify ID 15.
    pub fn trace_next<E, F, G>(
        &mut self,
        context: EventZeroDueContext,
        candidate_states: Result<[RareCandidateState; 32], E>,
        spot_route: F,
        attempt_input_for_draw: G,
    ) -> Result<PlannedDue, TraceError<E>>
    where
        F: FnOnce(u8, Option<u8>, XorShift128) -> Result<SpotRoute, E>,
        G: FnOnce(DueDraw) -> Result<AttemptInput, E>,
    {
        if self.remaining_due_count() == 0 {
            return Err(TraceError::Stream(OrdinaryStreamError::NoDueItem));
        }
        let draw = self
            .prepare_draw(context, candidate_states, spot_route)
            .map_err(|error| match error {
                PrepareError::Stream(error) => TraceError::Stream(error),
                PrepareError::UnknownInput(error) => TraceError::UnknownInput(error),
            })?;
        let result = if matches!(draw.selection, RareSpeciesSelection::ForcedGoldie(_)) {
            let plan = if let Some(block) = forced_skip_before_post_spot(context, draw) {
                GoldieAttemptPlan::Skipped(block)
            } else {
                let input = attempt_input_for_draw(draw).map_err(TraceError::UnknownInput)?;
                if !attempt_pre_item_matches(context, input) || input.selected_rare_id != 15 {
                    return Err(TraceError::InconsistentAttemptInput);
                }
                if matches!(draw.spot, SpotDraw::Generic { index: None, .. })
                    && input.spot_succeeded
                {
                    return Err(TraceError::ImpossibleSpotSuccess);
                }
                plan_event_zero_forced_goldie_attempts(input).map_err(TraceError::Attempt)?
            };
            PlannedDue::ForcedGoldie { draw, plan }
        } else if draw.species_id == UNSPECIFIED_BOSS_ID {
            PlannedDue::NoSpecies(draw)
        } else if context.runtime.active_rare_total >= 15 {
            PlannedDue::Selected {
                draw,
                plan: AttemptPlan::Skipped(AttemptBlock::RareGlobalLimit),
            }
        } else if context.runtime.active_total > context.runtime.item_total_limit {
            PlannedDue::Selected {
                draw,
                plan: AttemptPlan::Skipped(AttemptBlock::TotalGlobalLimit),
            }
        } else if draw.spot == SpotDraw::BlockedByRequestCount {
            PlannedDue::Selected {
                draw,
                plan: AttemptPlan::Skipped(AttemptBlock::RequestCount),
            }
        } else {
            let mut input = attempt_input_for_draw(draw).map_err(TraceError::UnknownInput)?;
            if !attempt_pre_item_matches(context, input) {
                return Err(TraceError::InconsistentAttemptInput);
            }
            if matches!(draw.spot, SpotDraw::Generic { index: None, .. }) && input.spot_succeeded {
                return Err(TraceError::ImpossibleSpotSuccess);
            }
            input.selected_rare_id = draw.species_id;
            let plan = plan_ordinary_item_attempts(input).map_err(TraceError::Attempt)?;
            PlannedDue::Selected { draw, plan }
        };
        self.commit_draw(draw);
        Ok(result)
    }

    /// Trace one item through the accepted-counter update before committing.
    ///
    /// The post-spot callback is used only on a creation-capable path that
    /// actually reached spot selection. A failed spot needs no virtual-gate
    /// result; the caller reports `SpotFailed`. The ordinary creation input is
    /// requested only after a successful spot and allowed gate. All errors
    /// leave the stream cursor and shared RNG at the prior item.
    pub fn trace_next_with_counter<E, F, G, H>(
        &mut self,
        context: EventZeroDueContext,
        candidate_states: Result<[RareCandidateState; 32], E>,
        spot_route: F,
        post_spot_for_draw: G,
        attempt_input_for_draw: H,
    ) -> Result<CounterTracedDue, TraceError<E>>
    where
        F: FnOnce(u8, Option<u8>, XorShift128) -> Result<SpotRoute, E>,
        G: FnOnce(DueDraw) -> Result<Option<PostSpotOutcome>, E>,
        H: FnOnce(DueDraw) -> Result<AttemptInput, E>,
    {
        let draw = self
            .prepare_draw(context, candidate_states, spot_route)
            .map_err(|error| match error {
                PrepareError::Stream(error) => TraceError::Stream(error),
                PrepareError::UnknownInput(error) => TraceError::UnknownInput(error),
            })?;
        let forced = matches!(draw.selection, RareSpeciesSelection::ForcedGoldie(_));
        let rare_over = !forced && context.runtime.active_rare_total >= 15;
        let total_over = context.runtime.active_total > context.runtime.item_total_limit;
        let blocked = matches!(
            draw.spot,
            SpotDraw::BlockedBeforeSpot | SpotDraw::BlockedByRequestCount
        );
        let no_species = draw.species_id == UNSPECIFIED_BOSS_ID;

        let (due, post_spot) = if no_species {
            (PlannedDue::NoSpecies(draw), None)
        } else if forced && (total_over || blocked) {
            let reason = forced_skip_before_post_spot(context, draw)
                .expect("forced total or pre-spot/request block has a skip reason");
            (
                PlannedDue::ForcedGoldie {
                    draw,
                    plan: GoldieAttemptPlan::Skipped(reason),
                },
                None,
            )
        } else if !forced && (rare_over || total_over || blocked) {
            let reason = if rare_over {
                AttemptBlock::RareGlobalLimit
            } else if total_over {
                AttemptBlock::TotalGlobalLimit
            } else {
                AttemptBlock::RequestCount
            };
            (
                PlannedDue::Selected {
                    draw,
                    plan: AttemptPlan::Skipped(reason),
                },
                None,
            )
        } else {
            let outcome = post_spot_for_draw(draw).map_err(TraceError::UnknownInput)?;
            let outcome = outcome.ok_or(TraceError::Counter(
                CounterProjectionError::MissingPostSpotOutcome,
            ))?;
            if matches!(draw.spot, SpotDraw::Generic { index: None, .. })
                && outcome != PostSpotOutcome::SpotFailed
            {
                return Err(TraceError::Counter(
                    CounterProjectionError::ImpossibleSpotSuccess,
                ));
            }
            let due = if forced {
                let plan = match outcome {
                    PostSpotOutcome::SpotFailed => {
                        GoldieAttemptPlan::Skipped(AttemptBlock::SpotFailed)
                    },
                    PostSpotOutcome::Vetoed => {
                        GoldieAttemptPlan::Skipped(AttemptBlock::PostSpotVeto)
                    },
                    PostSpotOutcome::Allowed => {
                        let input =
                            attempt_input_for_draw(draw).map_err(TraceError::UnknownInput)?;
                        if !attempt_pre_item_matches(context, input)
                            || input.selected_rare_id != 15
                            || !input.spot_succeeded
                            || !input.post_spot_allows
                        {
                            return Err(TraceError::InconsistentAttemptInput);
                        }
                        plan_event_zero_forced_goldie_attempts(input)
                            .map_err(TraceError::Attempt)?
                    },
                };
                PlannedDue::ForcedGoldie { draw, plan }
            } else {
                let plan = match outcome {
                    PostSpotOutcome::SpotFailed => AttemptPlan::Skipped(AttemptBlock::SpotFailed),
                    PostSpotOutcome::Vetoed => AttemptPlan::Skipped(AttemptBlock::PostSpotVeto),
                    PostSpotOutcome::Allowed => {
                        let mut input =
                            attempt_input_for_draw(draw).map_err(TraceError::UnknownInput)?;
                        if !attempt_pre_item_matches(context, input)
                            || !input.spot_succeeded
                            || !input.post_spot_allows
                        {
                            return Err(TraceError::InconsistentAttemptInput);
                        }
                        input.selected_rare_id = draw.species_id;
                        plan_ordinary_item_attempts(input).map_err(TraceError::Attempt)?
                    },
                };
                PlannedDue::Selected { draw, plan }
            };
            (due, Some(outcome))
        };
        let counter = project_event_zero_accepted_counter(context, draw, post_spot)
            .map_err(TraceError::Counter)?;
        self.commit_draw(draw);
        Ok(CounterTracedDue { due, counter })
    }

    fn prepare_draw<E, F>(
        &self,
        context: EventZeroDueContext,
        candidate_states: Result<[RareCandidateState; 32], E>,
        spot_route: F,
    ) -> Result<DueDraw, PrepareError<E>>
    where
        F: FnOnce(u8, Option<u8>, XorShift128) -> Result<SpotRoute, E>,
    {
        if self.remaining_due_count() == 0 {
            return Err(PrepareError::Stream(OrdinaryStreamError::NoDueItem));
        }
        if let Some(event) = context.periodic.event
            && event != self.configured_event
        {
            return Err(PrepareError::Stream(
                OrdinaryStreamError::UnexpectedRuntimeEvent(event),
            ));
        }

        let forced = periodic_goldie_source(context.periodic)
            .map_err(|error| PrepareError::Stream(OrdinaryStreamError::Periodic(error)))?
            .is_some();
        let states = match candidate_states {
            Ok(states) => states,
            Err(_) if forced => [RareCandidateState::Skip; 32],
            Err(error) => return Err(PrepareError::UnknownInput(error)),
        };

        let mut next_rng = self.rng;
        let selection = choose_event_rare_species(&mut next_rng, context.periodic, &states)
            .map_err(|error| {
                PrepareError::Stream(match error {
                    PeriodicGoldieError::InvalidCandidateId(id) => {
                        OrdinaryStreamError::InvalidCandidateId(id)
                    },
                    other => OrdinaryStreamError::Periodic(other),
                })
            })?;
        let species_id = selection.species_id();
        let over_total = context.runtime.active_total > context.runtime.item_total_limit;
        let early_rare_limit = !forced && context.runtime.active_rare_total >= 15;
        let blocked_before_spot = if forced {
            if over_total {
                if matches!(
                    context.forced_goldie_spot,
                    Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit { .. })
                ) {
                    return Err(PrepareError::Stream(
                        OrdinaryStreamError::InconsistentForcedGoldieSpotGate,
                    ));
                }
                false
            } else {
                match context.forced_goldie_spot {
                    Some(ForcedGoldieSpotGate::AtOrBelowTotalLimit {
                        id_fifteen_capacity_available,
                    }) => !id_fifteen_capacity_available,
                    Some(ForcedGoldieSpotGate::OverTotalLimit) => {
                        return Err(PrepareError::Stream(
                            OrdinaryStreamError::InconsistentForcedGoldieSpotGate,
                        ));
                    },
                    None => {
                        return Err(PrepareError::Stream(
                            OrdinaryStreamError::MissingForcedGoldieSpotGate,
                        ));
                    },
                }
            }
        } else {
            false
        };
        let spot = if blocked_before_spot {
            SpotDraw::BlockedBeforeSpot
        } else if species_id == UNSPECIFIED_BOSS_ID {
            SpotDraw::NoSpecies
        } else if !early_rare_limit && !over_total && context.runtime.requested_count < 1 {
            SpotDraw::BlockedByRequestCount
        } else {
            match spot_route(species_id, self.direction, next_rng)
                .map_err(PrepareError::UnknownInput)?
            {
                SpotRoute::Generic(count) => SpotDraw::Generic {
                    candidate_count: count,
                    index: choose_generic_spot_index(&mut next_rng, count),
                },
                SpotRoute::HandledByCallback => SpotDraw::HandledByCallback,
            }
        };
        Ok(DueDraw {
            queue_index: self.next_queue_index,
            entry: self.entries[self.next_queue_index],
            direction: self.direction,
            selection,
            species_id,
            requested_count: context.runtime.requested_count,
            spot,
            shared_state_after: next_rng,
        })
    }

    fn commit_draw(&mut self, draw: DueDraw) {
        self.rng = draw.shared_state_after;
        self.next_queue_index += 1;
    }
}

fn attempt_pre_item_matches(context: EventZeroDueContext, input: AttemptInput) -> bool {
    input.requested_count == context.runtime.requested_count
        && input.active_rare_total == context.runtime.active_rare_total
        && input.active_total == context.runtime.active_total
        && input.item_total_limit == context.runtime.item_total_limit
}

fn forced_skip_before_post_spot(
    context: EventZeroDueContext,
    draw: DueDraw,
) -> Option<AttemptBlock> {
    if context.runtime.active_total > context.runtime.item_total_limit {
        Some(AttemptBlock::TotalGlobalLimit)
    } else if draw.spot == SpotDraw::BlockedBeforeSpot {
        Some(AttemptBlock::GoldiePreSpotCapacity)
    } else if draw.spot == SpotDraw::BlockedByRequestCount {
        Some(AttemptBlock::RequestCount)
    } else {
        None
    }
}
