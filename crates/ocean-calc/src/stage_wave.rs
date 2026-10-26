//! Compose the ordinary JobType 0/1 wave selection inputs from known sources.
//!
//! The caller identifies the currently loaded stage and supplies scene or
//! participant data plus live mode and global values. ROMFS-derived weights
//! are source values; later application can override generated wave values.

use crate::{
    SeedWords,
    difficulty::{ParticipantRating, effective_difficulty_job_one, effective_difficulty_job_zero},
    levels::common_weights,
    records::{
        GenerationError, GenerationOutcome, NormalWaveInput, SpecialWaveState, predict_selections,
    },
    stage_data::StageSpawns,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaveWeights {
    /// Candidate weights before event eligibility is applied.
    pub event: [u32; 9],
    /// Candidate weights before event-dependent tide eligibility is applied.
    pub tide: [u32; 3],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaveWeightSource<'a> {
    /// Common 11.3.0 level weights with the selected stage's water override.
    RomfsDerived,
    /// Observed pre-eligibility weights for each generated wave index.
    Explicit(&'a [WaveWeights; 5]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeightOrigin {
    RomfsDerived,
    Explicit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DifficultySource<'a> {
    JobZero(&'a [ParticipantRating]),
    JobOne {
        match_mode: u32,
        scene_difficulty: i32,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct StageWaveInput<'a> {
    pub seed: SeedWords,
    /// Must correspond to the game's currently loaded stage resource.
    pub stage: &'a StageSpawns,
    pub difficulty: DifficultySource<'a>,
    /// Current map's `spl__CoopMapInfo.IsBigRun` for JobType 0/1.
    pub is_big_run: bool,
    pub global_word: u32,
    pub cutoff: Option<u8>,
    /// Supplied runtime special-tide state for JobType 0 or 1.
    pub special: Option<SpecialWaveState>,
    pub weights: WaveWeightSource<'a>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StageWaveOutcome {
    /// Calculated from the selected JobType source. Informational when weights
    /// are explicit; only `RomfsDerived` uses it to select source weights.
    pub effective_difficulty: u32,
    pub weight_origin: WeightOrigin,
    pub weights_used: [WaveWeights; 5],
    pub generation: GenerationOutcome,
}

/// Predict generated event/tide values for five wave slots or a suffix.
///
/// These are not applied round values: solo play can force event zero, and a
/// special wave can reuse the prior applied tide from this runtime context.
pub fn predict_stage_wave_selections(
    input: StageWaveInput<'_>,
) -> Result<StageWaveOutcome, GenerationError> {
    let (job_type, effective_difficulty) = match input.difficulty {
        DifficultySource::JobZero(participants) => (0, effective_difficulty_job_zero(participants)),
        DifficultySource::JobOne {
            match_mode,
            scene_difficulty,
        } => (
            1,
            effective_difficulty_job_one(match_mode, scene_difficulty),
        ),
    };
    let (weights_used, weight_origin) = match input.weights {
        WaveWeightSource::RomfsDerived => {
            let common = common_weights(effective_difficulty);
            let weights = WaveWeights {
                event: common.event,
                tide: input.stage.water_override.unwrap_or(common.water),
            };
            ([weights; 5], WeightOrigin::RomfsDerived)
        },
        WaveWeightSource::Explicit(weights) => (*weights, WeightOrigin::Explicit),
    };
    let per_wave = std::array::from_fn(|index| NormalWaveInput {
        event_weights: weights_used[index].event,
        tide_weights: weights_used[index].tide,
        mode_one: input.is_big_run,
        ban_event_mask: input.stage.ban_event_mask,
        global_word: input.global_word,
    });
    let generation =
        predict_selections(input.seed, input.cutoff, per_wave, job_type, input.special)?;
    Ok(StageWaveOutcome {
        effective_difficulty,
        weight_origin,
        weights_used,
        generation,
    })
}
