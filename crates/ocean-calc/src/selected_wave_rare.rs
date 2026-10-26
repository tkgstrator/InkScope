//! Conditional first Rare draw for one directly applied JobType 1 WAVE.
//!
//! The caller establishes that the supplied scene seed is actually active.
//! Direct application writes may be changed later, and this first-item model
//! assumes zero active/reserved Rare population, normal stage registration,
//! and no earlier spawn, callback, or shared-RNG draw before the queue item.
//! A generic spot additionally requires the unfiltered runtime actor group
//! to match the source stage group in count and order.
//! A generic spot index is into the runtime actor candidate array, not a
//! verified world coordinate or a successfully registered enemy.

use crate::{
    applied_preview::{
        AppliedPreviewError, AppliedScheduleStatus, JobOneSchedulerState,
        predict_job_one_applied_preview,
    },
    applied_wave::{DirectAppliedWave, JobOneApplyState},
    first_rare::{
        FirstEventRarePrediction, FirstRareError, FirstRareInput, FirstRarePrediction,
        FirstRareSpotDraw, FirstRareSpotError, FirstSpotRoute, continue_first_ordinary_rare_spot,
        predict_first_event_rare_species,
    },
    levels::{QueueCounts, queue_counts},
    preview::{CountOrigin, QueueCountSource},
    records::WaveSelection,
    rng::XorShift128,
    schedule::{NominalEntry, ScheduleBranch, ScheduleError, ScheduleInput},
    species::{PeriodicGoldieInput, PeriodicGoldieSource, UNSPECIFIED_BOSS_ID},
    stage_wave::StageWaveInput,
};

/// Caller assertion for a runtime condition; `AssumedIdealized` is never
/// presented as observed game state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConditionBasis {
    Observed,
    AssumedIdealized,
}

/// Evidence that later game code did not replace the direct event and tide.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppliedValueEvidence {
    /// Caller assumes no later override. A conditional output remains so marked.
    AssumedNoLaterOverride,
    /// Caller observed the effective values; they must match the direct writes.
    Observed { event_id: u8, tide_id: u32 },
}

/// Conditions of the first queue item, asserted by the caller independently.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirstRareRuntimeEvidence {
    /// The supplied four words equal the seed used by the live scene.
    pub scene_seed: ConditionBasis,
    pub applied_values: AppliedValueEvidence,
    pub zero_rare_population: ConditionBasis,
    pub no_prequeue_effects: ConditionBasis,
    pub normal_stage_registration: ConditionBasis,
    pub unfiltered_actor_group: ConditionBasis,
}

#[derive(Clone, Copy, Debug)]
pub struct SelectedWaveFirstRareInput<'a> {
    pub stage_wave: StageWaveInput<'a>,
    pub counts: QueueCountSource<'a>,
    pub branches: [ScheduleBranch; 5],
    pub states: [JobOneApplyState; 5],
    pub schedulers: [JobOneSchedulerState; 5],
    /// Original record index, including after a regeneration cutoff.
    pub wave_index: u8,
    /// First actual scheduler call to process the tick-zero nominal item.
    pub first_scheduler_elapsed_tick: i32,
    /// Current-event pointer state at this item's periodic predicate.
    pub periodic: PeriodicGoldieInput,
    pub runtime_evidence: FirstRareRuntimeEvidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpotStatus {
    /// The normal scan found no Rare ID; no spot path is reached.
    NoSpecies,
    /// A special callback route needs a live result. Only pre-spot RNG is known.
    NeedsCallback,
    Predicted {
        draw: FirstRareSpotDraw,
        shared_state_after_spot: XorShift128,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirstRareStatus {
    QueueBypassedByRuntimeMode,
    RequiresHandlerObservation(u8),
    UnsupportedAppliedEvent(u8),
    RejectedSchedule(ScheduleError),
    ForcedGoldie {
        nominal_first_entry: NominalEntry,
        source: PeriodicGoldieSource,
    },
    Normal {
        rare: FirstRarePrediction,
        spot: SpotStatus,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectedWaveFirstRareReport {
    pub effective_difficulty: u32,
    pub count_origin: CountOrigin,
    pub generated: WaveSelection,
    /// Direct writes, conditionally treated as final under `runtime_evidence`.
    pub direct: DirectAppliedWave,
    pub runtime_evidence: FirstRareRuntimeEvidence,
    /// Effective counts when a modeled Rare queue was attempted, including a
    /// rejected schedule whose counts failed validation.
    pub counts: Option<QueueCounts>,
    pub assumed_branch: Option<ScheduleBranch>,
    pub status: FirstRareStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectedWaveFirstRareError {
    Preview(AppliedPreviewError),
    WaveNotGenerated(u8),
    ObservedAppliedMismatch {
        direct_event: u8,
        direct_tide: u32,
        observed_event: u8,
        observed_tide: u32,
    },
    InvalidAppliedTide(u32),
    FirstRare(FirstRareError),
}

/// Compose applied WAVE selection with its first ordinary Rare item.
///
/// The packet game seed is usable here only after it is established as the
/// live scene seed. The result remains conditional on the explicit runtime
/// evidence, queue branch/count inputs, normal initial candidate callbacks,
/// and the absence of intervening state changes. `route_for_species` is
/// consulted only for an unresolved species-specific spot callback; returning
/// `Unknown` retains the species and pre-spot RNG without inventing a spot.
pub fn predict_selected_job_one_first_rare<F>(
    input: SelectedWaveFirstRareInput<'_>,
    route_for_species: F,
) -> Result<SelectedWaveFirstRareReport, SelectedWaveFirstRareError>
where
    F: FnOnce(u8) -> FirstSpotRoute,
{
    let preview = predict_job_one_applied_preview(
        input.stage_wave,
        input.counts,
        input.branches,
        input.states,
        input.schedulers,
    )
    .map_err(SelectedWaveFirstRareError::Preview)?;
    let selected = preview
        .waves
        .into_iter()
        .find(|wave| wave.generated.wave_index == input.wave_index)
        .ok_or(SelectedWaveFirstRareError::WaveNotGenerated(
            input.wave_index,
        ))?;
    if let AppliedValueEvidence::Observed { event_id, tide_id } =
        input.runtime_evidence.applied_values
        && (event_id != selected.direct.event_id || tide_id != selected.direct.tide_id)
    {
        return Err(SelectedWaveFirstRareError::ObservedAppliedMismatch {
            direct_event: selected.direct.event_id,
            direct_tide: selected.direct.tide_id,
            observed_event: event_id,
            observed_tide: tide_id,
        });
    }

    let (counts, assumed_branch, status) = match selected.schedule {
        AppliedScheduleStatus::QueueBypassedByRuntimeMode => {
            (None, None, FirstRareStatus::QueueBypassedByRuntimeMode)
        },
        AppliedScheduleStatus::RequiresHandlerObservation(id) => {
            (None, None, FirstRareStatus::RequiresHandlerObservation(id))
        },
        AppliedScheduleStatus::UnsupportedAppliedEvent(id) => {
            (None, None, FirstRareStatus::UnsupportedAppliedEvent(id))
        },
        AppliedScheduleStatus::Rejected(reason) => {
            let index = usize::from(selected.generated.wave_index);
            let effective_counts = match input.counts {
                QueueCountSource::RomfsClampedRoundByWave => queue_counts(
                    preview.stage_wave.effective_difficulty,
                    selected.generated.wave_index.min(2),
                )
                .expect("generated record index is clamped to a source Round"),
                QueueCountSource::Explicit(values) => values[index],
            };
            (
                Some(effective_counts),
                Some(input.branches[index]),
                FirstRareStatus::RejectedSchedule(reason),
            )
        },
        AppliedScheduleStatus::Predicted {
            counts,
            assumed_branch,
            ..
        } => {
            let tide = u8::try_from(selected.direct.tide_id)
                .ok()
                .filter(|value| *value <= 2)
                .ok_or(SelectedWaveFirstRareError::InvalidAppliedTide(
                    selected.direct.tide_id,
                ))?;
            let first_input = FirstRareInput {
                schedule: ScheduleInput {
                    seed: input.stage_wave.seed,
                    event_type: selected.direct.event_id,
                    wave_change_num: counts.wave_change_num,
                    rare_appear_num: counts.rare_appear_num,
                    assumed_branch,
                },
                stage: input.stage_wave.stage,
                tide,
                global_word: input.stage_wave.global_word,
                first_scheduler_elapsed_tick: input.first_scheduler_elapsed_tick,
                periodic: input.periodic,
            };
            let result = predict_first_event_rare_species(first_input)
                .map_err(SelectedWaveFirstRareError::FirstRare)?;
            let status = match result {
                FirstEventRarePrediction::ForcedGoldie {
                    nominal_first_entry,
                    source,
                } => FirstRareStatus::ForcedGoldie {
                    nominal_first_entry,
                    source,
                },
                FirstEventRarePrediction::Normal(rare) => {
                    let spot = if rare.species_id == UNSPECIFIED_BOSS_ID {
                        SpotStatus::NoSpecies
                    } else {
                        match continue_first_ordinary_rare_spot(
                            first_input,
                            rare,
                            route_for_species,
                        ) {
                            Ok(result) => SpotStatus::Predicted {
                                draw: result.spot,
                                shared_state_after_spot: result.shared_state_after_spot,
                            },
                            Err(FirstRareSpotError::UnknownSpotRoute) => SpotStatus::NeedsCallback,
                            Err(FirstRareSpotError::Rare(error)) => {
                                return Err(SelectedWaveFirstRareError::FirstRare(error));
                            },
                        }
                    };
                    FirstRareStatus::Normal { rare, spot }
                },
            };
            (Some(counts), Some(assumed_branch), status)
        },
    };
    Ok(SelectedWaveFirstRareReport {
        effective_difficulty: preview.stage_wave.effective_difficulty,
        count_origin: preview.count_origin,
        generated: selected.generated,
        direct: selected.direct,
        runtime_evidence: input.runtime_evidence,
        counts,
        assumed_branch,
        status,
    })
}
