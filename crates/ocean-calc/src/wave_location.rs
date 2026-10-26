//! Selected JobType 1 WAVE view of common-Zako direction and request points.
//!
//! This joins a generated and directly applied WAVE with the relevant
//! species-free location calculation. Runtime snapshots are required for a
//! location; a seed and stage Banc alone do not establish one. Event 0 is one
//! caller-observed attempt, while Rush can trace a complete call sequence.
//! Returned points are creation-request transforms, not confirmed spawns or
//! final enemy coordinates.

use crate::{
    applied_preview::{
        AppliedPreviewError, AppliedScheduleStatus, JobOneSchedulerState,
        predict_job_one_applied_preview,
    },
    applied_wave::{DirectAppliedWave, JobOneApplyState},
    preview::QueueCountSource,
    records::WaveSelection,
    rush_location::{
        RushLocationCallInput, RushLocationError, RushLocationInput, RushLocationTrace,
        trace_rush_locations,
    },
    schedule::ScheduleBranch,
    stage_wave::StageWaveInput,
    zako_spot::{ModeZeroZakoSpotInput, ZakoSpotError, ZakoSpotTrace, predict_mode_zero_zako_spot},
};

/// Runtime inputs for Rush, separate from the generated WAVE record.
#[derive(Clone, Copy, Debug)]
pub struct RushRuntimeInput<'a> {
    /// Keys from the actual registered stage spawner at this WAVE.
    pub stage_keys: &'a [u8],
    /// Effective queue `WaveChangeNum`; the Event 1 nominal Rare queue does
    /// not supply it.
    pub wave_change_num: i32,
    pub initial_timer: i32,
    /// Every scheduler call from WAVE start through the target, in order.
    pub calls: &'a [RushLocationCallInput<'a>],
}

/// The Event 0 spot input is for one reached call only. The caller derives
/// its direction through the ordinary Rare stream and supplies the observed
/// post-Zako-selector `+0x38c` RNG state. No species ID is needed.
#[derive(Clone, Copy, Debug)]
pub enum LocationRuntime<'a> {
    Rush(RushRuntimeInput<'a>),
    EventZero(ModeZeroZakoSpotInput<'a>),
}

pub struct JobOneWaveLocationInput<'a> {
    pub stage_wave: StageWaveInput<'a>,
    pub counts: QueueCountSource<'a>,
    pub branches: [ScheduleBranch; 5],
    pub states: [JobOneApplyState; 5],
    pub schedulers: [JobOneSchedulerState; 5],
    /// Original generated record index, including after a cutoff.
    pub wave_index: u8,
    pub runtime: Option<LocationRuntime<'a>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LocationStatus {
    /// Event 0 or 1 needs the actual per-call runtime state.
    MissingRuntime,
    /// Event 8 may change state in an unresolved indirect handler.
    RequiresHandlerObservation(u8),
    /// No species-free location route is composed for this applied event.
    UnsupportedAppliedEvent(u8),
    Rush(RushLocationTrace),
    EventZeroAttempt {
        /// Game direction group number only after reaching the spot path;
        /// not a compass bearing or a confirmed spawn direction.
        direction: Option<u8>,
        spot: ZakoSpotTrace,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct JobOneWaveLocationReport {
    pub effective_difficulty: u32,
    pub generated: WaveSelection,
    pub direct: DirectAppliedWave,
    pub schedule: AppliedScheduleStatus,
    pub location: LocationStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaveLocationError {
    Preview(AppliedPreviewError),
    WaveNotGenerated(u8),
    RuntimeForWrongEvent { applied_event: u8 },
    EmptyRushCalls,
    InvalidAppliedTide(u32),
    RuntimeTideMismatch { applied: u32, supplied: i32 },
    EventZeroRuntimeEventMismatch(u8),
    Rush(RushLocationError),
    EventZero(ZakoSpotError),
}

/// Calculate one selected WAVE and the Zako request point when inputs permit.
///
/// The generation seed is reused for Rush internally. The caller still must
/// establish that this is the live scene seed and that the supplied scheduler
/// calls, gates, stage keys, candidate order, and RNG observations match the
/// game. Other code may change these between calls. Missing data produces an
/// explicit status rather than a guessed location.
pub fn predict_job_one_wave_location(
    input: JobOneWaveLocationInput<'_>,
) -> Result<JobOneWaveLocationReport, WaveLocationError> {
    let seed = input.stage_wave.seed;
    let preview = predict_job_one_applied_preview(
        input.stage_wave,
        input.counts,
        input.branches,
        input.states,
        input.schedulers,
    )
    .map_err(WaveLocationError::Preview)?;
    let wave = preview
        .waves
        .into_iter()
        .find(|wave| wave.generated.wave_index == input.wave_index)
        .ok_or(WaveLocationError::WaveNotGenerated(input.wave_index))?;
    let applied_event = wave.direct.event_id;
    let applied_tide = wave.direct.tide_id;
    let location = match (applied_event, input.runtime) {
        (8, None) if wave.direct.requires_handler_observation => {
            LocationStatus::RequiresHandlerObservation(8)
        },
        (0 | 1, None) => LocationStatus::MissingRuntime,
        (event, None) => LocationStatus::UnsupportedAppliedEvent(event),
        (1, Some(LocationRuntime::Rush(runtime))) => {
            validate_tide(applied_tide)?;
            if runtime.calls.is_empty() {
                return Err(WaveLocationError::EmptyRushCalls);
            }
            for call in runtime.calls {
                if let Some(selector) = call.selector
                    && selector.tide as u32 != applied_tide
                {
                    return Err(WaveLocationError::RuntimeTideMismatch {
                        applied: applied_tide,
                        supplied: selector.tide,
                    });
                }
            }
            LocationStatus::Rush(
                trace_rush_locations(RushLocationInput {
                    seed,
                    stage_keys: runtime.stage_keys,
                    wave_change_num: runtime.wave_change_num,
                    initial_timer: runtime.initial_timer,
                    calls: runtime.calls,
                })
                .map_err(WaveLocationError::Rush)?,
            )
        },
        (0, Some(LocationRuntime::EventZero(runtime))) => {
            validate_tide(applied_tide)?;
            if runtime.applied_event != 0 {
                return Err(WaveLocationError::EventZeroRuntimeEventMismatch(
                    runtime.applied_event,
                ));
            }
            if runtime.tide as u32 != applied_tide {
                return Err(WaveLocationError::RuntimeTideMismatch {
                    applied: applied_tide,
                    supplied: runtime.tide,
                });
            }
            let spot =
                predict_mode_zero_zako_spot(runtime).map_err(WaveLocationError::EventZero)?;
            LocationStatus::EventZeroAttempt {
                direction: spot
                    .group_key
                    .and_then(|_| u8::try_from(runtime.direction).ok()),
                spot,
            }
        },
        (event, Some(_)) => {
            return Err(WaveLocationError::RuntimeForWrongEvent {
                applied_event: event,
            });
        },
    };
    Ok(JobOneWaveLocationReport {
        effective_difficulty: preview.stage_wave.effective_difficulty,
        generated: wave.generated,
        direct: wave.direct,
        schedule: wave.schedule,
        location,
    })
}

fn validate_tide(tide: u32) -> Result<(), WaveLocationError> {
    if tide <= 2 {
        Ok(())
    } else {
        Err(WaveLocationError::InvalidAppliedTide(tide))
    }
}
