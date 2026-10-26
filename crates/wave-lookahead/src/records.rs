//! Generated event and tide selections for JobType 0–4 wave records.
//!
//! This models generation only. Applying a wave can later override the event,
//! and a special wave's applied tide can reuse the previous applied tide.
//! JobType 2/3 skip event selection with no special target only when the
//! runtime mode object is present. This API requires that condition when
//! passed JobType 2/3; a missing object instead enters the ordinary event scan.
//! Passing JobType 4 likewise requires an observed, nonnull runtime mode
//! object with value 4. A saved scenario JobType alone does not establish it.

use crate::{
    SeedWords,
    rng::derive,
    rules::{EventContext, event_candidates, tide_candidates},
    wave::{WaveRng, draw_event, draw_tide},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NormalWaveInput {
    /// Effective event weights after game configuration and any overrides.
    pub event_weights: [u32; 9],
    /// Effective water weights after the optional stage overwrite.
    pub tide_weights: [u32; 3],
    /// Runtime mode flag. For JobType 0/1 it follows stage IsBigRun;
    /// an observed live JobType 4 forces the effective value to true.
    pub mode_one: bool,
    pub ban_event_mask: u32,
    pub global_word: u32,
}

/// Only the two generated values, not the full game record or applied wave.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaveSelection {
    pub wave_index: u8,
    pub event_id: u8,
    pub tide_id: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpecialWaveState {
    pub target_index: u8,
    /// 0–2 means a stored tide; 3 is the not-yet-stored sentinel.
    pub stored_tide: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitialSpecialError {
    /// The guarded initial caller skips `0x37f4c48` without its target.
    GenerationTargetAbsent,
}

/// Result of the inspected initial JobType 0 special-state lottery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InitialJobZeroSpecialDecision {
    pub draw: u32,
    pub threshold_bits: u32,
    pub uniform_bits: u32,
    pub special: Option<SpecialWaveState>,
}

/// Run the initial JobType 0 special-state lottery on the inspected direct path.
///
/// The caller establishes that the live runtime mode object exists and reports
/// JobType 0, and supplies the effective signed score read by the game.
/// `global_seed_words` is the registered shared RNG object's `+0x50..+0x5c`,
/// not the raw scene fields; ordinary scene initialization normalizes an
/// all-zero scene before registering this object. `None` means the global
/// object pointer is null. An all-zero global field itself has a distinct
/// fallback. This does not infer the score or saved-scenario seed transfer.
pub fn initial_job_type_zero_special_state(
    global_seed_words: Option<[u32; 4]>,
    effective_score: i32,
) -> InitialJobZeroSpecialDecision {
    let mut words = global_seed_words.unwrap_or([1, 0, 0, 0]);
    if global_seed_words.is_some() && words == [0; 4] {
        words = [1, 0, 0, 0x4807_7044];
    }
    let draw = derive(SeedWords::from_scene(words), 0x3217_8a0b).next32();
    let threshold = if effective_score < 5 {
        0.0_f32
    } else {
        effective_score.wrapping_mul(effective_score.wrapping_sub(4)) as f32 / 320.0_f32
    };
    let uniform = f32::from_bits(0x3f80_0000 | (draw >> 9)) - 1.0_f32;
    let special = (threshold >= uniform).then_some(SpecialWaveState {
        target_index: 3,
        stored_tide: 3,
    });
    InitialJobZeroSpecialDecision {
        draw,
        threshold_bits: threshold.to_bits(),
        uniform_bits: uniform.to_bits(),
        special,
    }
}

/// Initial JobType 1 state on the two inspected direct generation call sites.
///
/// The live mode object's `+0x590` is cleared before the same object is read
/// through `*0x6083208`, so this path cannot enable target 3. Other paths and
/// later regeneration need separately supplied runtime state. The first
/// inspected caller skips setup when the target is missing; the second does
/// not show a null guard, so its null-target behavior is outside this helper.
pub fn initial_job_type_one_special_state(
    controller_present: bool,
) -> Result<Option<SpecialWaveState>, InitialSpecialError> {
    if !controller_present {
        return Err(InitialSpecialError::GenerationTargetAbsent);
    }
    Ok(None)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationOutcome {
    pub selections: Vec<WaveSelection>,
    pub special: Option<SpecialWaveState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenerationError {
    InvalidCutoff(u8),
    UnsupportedJobType(u8),
    SpecialRequiresJobTypeZeroOrOne,
    InvalidSpecialIndex(u8),
    InvalidStoredTide(u8),
}

/// Generate the suffix after `cutoff`; `None` generates indices 0 through 4.
///
/// JobType 0/1/4 use the ordinary event-then-tide path. JobType 2/3 with no
/// special state skip event selection, set event 0, and run the tide scan.
/// A supplied special state is modeled for JobType 0/1. The caller must
/// supply the actual runtime +0x624/+0x620/+0x628 state and per-wave effective
/// inputs. For JobType 2/3 the caller must establish that the runtime mode
/// object (`[global]+0xd0` at `0x37f5d4c`) is present and carries that value;
/// if it is absent, the game enters the ordinary event scan instead. Passing
/// JobType 4 also requires the observed live runtime mode object to be present
/// with value 4: this forces the effective mode flag to one for event and tide
/// candidates. A saved scenario JobType 4 is insufficient. With a null runtime
/// object this low-level selector uses the ordinary path and map-derived flag;
/// this does not establish equivalence for other WAVE behavior. JobType 4's
/// special-state input is unsupported. Applied WAVE values can differ later.
pub fn predict_selections(
    seed: SeedWords,
    cutoff: Option<u8>,
    inputs: [NormalWaveInput; 5],
    job_type: u8,
    mut special: Option<SpecialWaveState>,
) -> Result<GenerationOutcome, GenerationError> {
    if let Some(value) = cutoff
        && value > 4
    {
        return Err(GenerationError::InvalidCutoff(value));
    }
    if job_type > 4 {
        return Err(GenerationError::UnsupportedJobType(job_type));
    }
    if special.is_some() && !matches!(job_type, 0 | 1) {
        return Err(GenerationError::SpecialRequiresJobTypeZeroOrOne);
    }
    if let Some(state) = special {
        if state.target_index > 4 {
            return Err(GenerationError::InvalidSpecialIndex(state.target_index));
        }
        if state.stored_tide > 3 {
            return Err(GenerationError::InvalidStoredTide(state.stored_tide));
        }
    }
    let mut rng = WaveRng::from_seed(seed);
    let mut previous_event = 0;
    let mut selections = Vec::new();
    for (wave_index, input) in inputs.into_iter().enumerate() {
        if cutoff.is_some_and(|last_existing| wave_index <= usize::from(last_existing)) {
            continue;
        }
        let mode_one = input.mode_one || job_type == 4;
        if special.is_some_and(|state| usize::from(state.target_index) == wave_index) {
            let state = special.expect("validated special state exists");
            previous_event = 0;
            let tide_id = if state.stored_tide == 3 {
                draw_tide(&mut rng, &tide_candidates(input.tide_weights, mode_one, 0))
            } else {
                state.stored_tide
            };
            selections.push(WaveSelection {
                wave_index: wave_index as u8,
                event_id: 0,
                tide_id,
            });
            continue;
        }
        let event_id = if matches!(job_type, 2 | 3) {
            0
        } else {
            let event_context = EventContext {
                mode_one,
                ban_event_mask: input.ban_event_mask,
                global_word: input.global_word,
                previous_event,
            };
            draw_event(
                &mut rng,
                &event_candidates(input.event_weights, event_context),
            )
        };
        previous_event = event_id;
        let tide_id = draw_tide(
            &mut rng,
            &tide_candidates(input.tide_weights, mode_one, event_id),
        );
        if let Some(state) = special.as_mut()
            && wave_index + 1 == usize::from(state.target_index)
            && state.stored_tide == 3
        {
            state.stored_tide = tide_id;
        }
        selections.push(WaveSelection {
            wave_index: wave_index as u8,
            event_id,
            tide_id,
        });
    }
    Ok(GenerationOutcome {
        selections,
        special,
    })
}

/// Ordinary JobType 0/1 generation when special-tide flag +0x624 is disabled.
pub fn predict_unforced_selections(
    seed: SeedWords,
    cutoff: Option<u8>,
    inputs: [NormalWaveInput; 5],
) -> Result<Vec<WaveSelection>, GenerationError> {
    predict_selections(seed, cutoff, inputs, 0, None).map(|outcome| outcome.selections)
}
