//! Conditional species prediction for the first ordinary Rare queue item.
//!
//! This uses the normal WAVE-start stage construction and object flags from
//! 11.3.0. The loaded stage, current tide, global word, effective queue counts,
//! and first scheduler tick must match the game. This assumes the standard
//! constructed Rare objects and callback registration: IDs other than 11 and
//! 13 have an always-true candidate callback. No prior spawn or unmodeled
//! shared-RNG draw may intervene. The normal species and spot APIs cover
//! applied events 0, 5, and 6, conditional on Fog's indirect prelude not
//! advancing the shared Rare RNG. Their result is a species draw, not
//! confirmed actor creation. A periodic ID 15 force is exposed separately
//! without inventing normal-scan or spot details; continued processing needs
//! the stateful stream with an explicit ID 15 pre-spot gate.

use crate::{
    direction::{
        DirectionError, advance_direction_for_elapsed, direction_interval,
        predict_initial_directions,
    },
    enemy_object::artillery_initial_callback_allows,
    rng::XorShift128,
    schedule::{NominalEntry, ScheduleError, ScheduleInput, nominal_schedule},
    species::{
        GOLDIE_BOSS_ID, PeriodicGoldieError, PeriodicGoldieInput, PeriodicGoldieSource,
        RareCandidateState, choose_rare_boss, normal_first_rare_candidates, periodic_goldie_source,
    },
    spot::choose_generic_spot_index,
    stage_data::{PillarStaticGate, StageSpawns},
};

#[derive(Clone, Copy, Debug)]
pub struct FirstRareInput<'a> {
    pub schedule: ScheduleInput,
    /// The actual loaded 11.3.0 stage. Normal Banc actor registration is an
    /// explicit precondition for the Pillar's static initial gate.
    pub stage: &'a StageSpawns,
    pub tide: u8,
    pub global_word: u32,
    /// Elapsed tick of the first scheduler call that processes the tick-0
    /// item. A delayed call may update direction before species selection.
    pub first_scheduler_elapsed_tick: i32,
    /// Runtime applied-event periodic context at the first item's predicate, before
    /// that item's possible accepted-counter increment.
    pub periodic: PeriodicGoldieInput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirstRarePrediction {
    pub nominal_first_entry: NominalEntry,
    pub wave_start_direction: Option<u8>,
    pub after_queue_build_direction: Option<u8>,
    pub first_direction: u8,
    pub direction_branch_invoked: bool,
    pub candidate_states: [RareCandidateState; 32],
    pub species_id: u8,
    pub shared_state_after_species: XorShift128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirstEventRarePrediction {
    Normal(FirstRarePrediction),
    /// ID 15 bypasses the normal candidate scan. Direction, post-spot RNG,
    /// creation, and position require separate live inputs and processing.
    ForcedGoldie {
        nominal_first_entry: NominalEntry,
        source: PeriodicGoldieSource,
    },
}

impl FirstEventRarePrediction {
    pub fn species_id(self) -> u8 {
        match self {
            Self::Normal(prediction) => prediction.species_id,
            Self::ForcedGoldie { .. } => GOLDIE_BOSS_ID,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirstRareError {
    Schedule(ScheduleError),
    Direction(DirectionError),
    InvalidTide(u8),
    NegativeElapsedTick(i32),
    MissingDirection,
    PillarDependsOnRuntimeOrder,
    UnexpectedRuntimeEvent(u8),
    Periodic(PeriodicGoldieError),
    ForcedGoldieRequiresStream,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirstSpotRoute {
    /// Species-specific spot callback declined without using the shared RNG.
    GenericStageGroup,
    /// Species-specific spot callback chose a spot without using the shared RNG.
    HandledWithoutSharedDraw,
    /// The callback result or shared-RNG side effects have not been resolved.
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirstRareSpotDraw {
    /// Index into the game's runtime actor group, whose order is not yet proven
    /// to match the Banc order in `StageSpawns`.
    Generic {
        candidate_count: u32,
        index: Option<u32>,
    },
    HandledByCallback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirstRareSpotPrediction {
    pub rare: FirstRarePrediction,
    pub spot: FirstRareSpotDraw,
    pub shared_state_after_spot: XorShift128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirstRareSpotError {
    Rare(FirstRareError),
    UnknownSpotRoute,
}

#[derive(Clone, Copy)]
struct ValidatedFirstRare {
    nominal_first_entry: NominalEntry,
    forced_source: Option<PeriodicGoldieSource>,
}

fn validate_first_rare_input(
    input: FirstRareInput<'_>,
) -> Result<ValidatedFirstRare, FirstRareError> {
    if !matches!(input.schedule.event_type, 0 | 5 | 6) {
        return Err(FirstRareError::Schedule(ScheduleError::UnsupportedEvent(
            input.schedule.event_type,
        )));
    }
    if input.tide > 2 {
        return Err(FirstRareError::InvalidTide(input.tide));
    }
    if input.first_scheduler_elapsed_tick < 0 {
        return Err(FirstRareError::NegativeElapsedTick(
            input.first_scheduler_elapsed_tick,
        ));
    }
    let entries = nominal_schedule(input.schedule).map_err(FirstRareError::Schedule)?;
    if let Some(event) = input.periodic.event
        && event != input.schedule.event_type
    {
        return Err(FirstRareError::UnexpectedRuntimeEvent(event));
    }
    let forced_source = periodic_goldie_source(input.periodic).map_err(FirstRareError::Periodic)?;
    Ok(ValidatedFirstRare {
        nominal_first_entry: entries[0],
        forced_source,
    })
}

/// Predict the first EventType 0/5/6, JobType 0/1 Rare species. A periodic
/// forced ID 15 returns only its ID and source, before normal callbacks.
pub fn predict_first_event_rare_species(
    input: FirstRareInput<'_>,
) -> Result<FirstEventRarePrediction, FirstRareError> {
    let validated = validate_first_rare_input(input)?;
    if let Some(source) = validated.forced_source {
        return Ok(FirstEventRarePrediction::ForcedGoldie {
            nominal_first_entry: validated.nominal_first_entry,
            source,
        });
    }
    predict_first_normal(input, validated.nominal_first_entry).map(FirstEventRarePrediction::Normal)
}

/// Predict the first nonforced EventType 0/5/6, JobType 0/1 Rare species draw
/// under the preconditions documented on `FirstRareInput`.
pub fn predict_first_ordinary_rare(
    input: FirstRareInput<'_>,
) -> Result<FirstRarePrediction, FirstRareError> {
    let validated = validate_first_rare_input(input)?;
    if validated.forced_source.is_some() {
        return Err(FirstRareError::ForcedGoldieRequiresStream);
    }
    predict_first_normal(input, validated.nominal_first_entry)
}

fn predict_first_normal(
    input: FirstRareInput<'_>,
    nominal_first_entry: NominalEntry,
) -> Result<FirstRarePrediction, FirstRareError> {
    let stage_keys = input.stage.available_keys();
    let initial = predict_initial_directions(input.schedule.seed, &stage_keys)
        .map_err(FirstRareError::Direction)?;
    let mut rng = initial.shared_state;
    let advanced = advance_direction_for_elapsed(
        &mut rng,
        &stage_keys,
        initial.after_queue_build,
        -1,
        input.first_scheduler_elapsed_tick,
        direction_interval(input.schedule.wave_change_num as i32),
    )
    .map_err(FirstRareError::Direction)?;
    let direction = advanced.direction.ok_or(FirstRareError::MissingDirection)?;

    let mut callbacks = [true; 11];
    callbacks[11 - 4] = match input
        .stage
        .pillar_initial_static_gate(input.tide, direction)
        .expect("validated tide and direction")
    {
        PillarStaticGate::Available => true,
        PillarStaticGate::Unavailable => false,
        PillarStaticGate::DependsOnRuntimeOrder => {
            return Err(FirstRareError::PillarDependsOnRuntimeOrder);
        },
    };
    callbacks[13 - 4] =
        artillery_initial_callback_allows(input.schedule.seed, input.tide).expect("validated tide");
    let candidate_states = normal_first_rare_candidates(input.global_word, callbacks);
    let species_id = choose_rare_boss(&mut rng, &candidate_states);
    Ok(FirstRarePrediction {
        nominal_first_entry,
        wave_start_direction: initial.wave_start,
        after_queue_build_direction: initial.after_queue_build,
        first_direction: direction,
        direction_branch_invoked: advanced.branch_invoked,
        candidate_states,
        species_id,
        shared_state_after_species: rng,
    })
}

/// Continue the first nonforced event-0/5/6 species draw through its spot
/// callback route.
///
/// The caller must establish the route for selected special-spot species.
/// For the generic route, normal scene actor registration and an unfiltered runtime
/// group matching the supplied stage are preconditions. The result is an
/// actor-array index and next shared RNG state, not a resolved actor or XYZ.
/// Standard constructed IDs 4/6/7/9/10/12/14 always decline their spot
/// callback. An untouched ID11 group with every anchor linked always handles
/// it. `route_for_species` is called only when these rules do not settle the
/// route (IDs 5/8/13 and unresolved ID11 groups).
pub fn predict_first_ordinary_rare_with_spot<F>(
    input: FirstRareInput<'_>,
    route_for_species: F,
) -> Result<FirstRareSpotPrediction, FirstRareSpotError>
where
    F: FnOnce(u8) -> FirstSpotRoute,
{
    let rare = predict_first_ordinary_rare(input).map_err(FirstRareSpotError::Rare)?;
    continue_first_ordinary_rare_spot(input, rare, route_for_species)
}

/// Continue a normal first-item prediction without repeating its species scan.
/// The caller must pair `rare` with this same immutable `input`.
pub(crate) fn continue_first_ordinary_rare_spot<F>(
    input: FirstRareInput<'_>,
    rare: FirstRarePrediction,
    route_for_species: F,
) -> Result<FirstRareSpotPrediction, FirstRareSpotError>
where
    F: FnOnce(u8) -> FirstSpotRoute,
{
    let mut rng = rare.shared_state_after_species;
    let route = match rare.species_id {
        4 | 6 | 7 | 9 | 10 | 12 | 14 => FirstSpotRoute::GenericStageGroup,
        11 if input
            .stage
            .pillar_initial_spot_guaranteed(input.tide, rare.first_direction) =>
        {
            FirstSpotRoute::HandledWithoutSharedDraw
        },
        5 | 8 | 11 | 13 => route_for_species(rare.species_id),
        _ => return Err(FirstRareSpotError::UnknownSpotRoute),
    };
    let spot = match route {
        FirstSpotRoute::GenericStageGroup => {
            let group = input
                .stage
                .group(input.tide, rare.first_direction)
                .expect("first Rare prediction validated tide and direction");
            let candidate_count =
                u32::try_from(group.len()).expect("owned 11.3.0 stage groups fit in u32");
            FirstRareSpotDraw::Generic {
                candidate_count,
                index: choose_generic_spot_index(&mut rng, candidate_count),
            }
        },
        FirstSpotRoute::HandledWithoutSharedDraw => FirstRareSpotDraw::HandledByCallback,
        FirstSpotRoute::Unknown => return Err(FirstRareSpotError::UnknownSpotRoute),
    };
    Ok(FirstRareSpotPrediction {
        rare,
        spot,
        shared_state_after_spot: rng,
    })
}
