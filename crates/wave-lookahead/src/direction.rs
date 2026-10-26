//! Direction selection for the ordinary event's stage spawn-point keys.
//!
//! Keys are `3 * d + layer` for `CoopSpawnPointEnemy{d}` actors, where
//! layer Low/Mid/High is 0/1/2. The game tests key bits 0, 1, and 2 while
//! reducing the direction candidates; this module preserves that behavior.

use crate::rng::{SeedWords, XorShift128, derive, expand_seed_word};

pub const DIRECTION_DOMAIN: u32 = 0x8d56_161f;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectionError {
    InvalidStageKey(u8),
    InvalidPrevious(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InitialDirections {
    pub wave_start: Option<u8>,
    pub after_queue_build: Option<u8>,
    /// Shared RNG state after director initialization and two direction selections.
    pub shared_state: XorShift128,
    /// Independent director state at `+0x38c` immediately after WAVE initialization.
    /// Later creation and default spot paths may advance this separately.
    pub director_rng_38c: XorShift128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectionAdvance {
    pub direction: Option<u8>,
    /// The scheduler called the direction selector. A selector with no
    /// candidates can keep the old direction without consuming an RNG draw.
    pub branch_invoked: bool,
}

/// Queue interval `I` for the ordinary event's `WaveChangeNum` source value.
///
/// The queue leaves its interval at zero when `N < 2`. For positive `N` the
/// division is signed and truncates toward zero, matching the game code.
pub fn direction_interval(wave_change_num: i32) -> i32 {
    if wave_change_num < 2 {
        0
    } else {
        4320 / (wave_change_num - 1)
    }
}

/// Run the ordinary scheduler's direction update before processing due items.
///
/// `old_elapsed_tick` is the prior queue tick (initially -1), and
/// `new_elapsed_tick` is the newly clamped elapsed tick. The game invokes
/// direction selection at most once per scheduler call, even when several
/// intervals are crossed. Every due item processed in that call sees the
/// resulting direction. The caller interleaves boss/spot draws on `rng`.
pub fn advance_direction_for_elapsed(
    rng: &mut XorShift128,
    stage_keys: &[u8],
    previous: Option<u8>,
    old_elapsed_tick: i32,
    new_elapsed_tick: i32,
    interval: i32,
) -> Result<DirectionAdvance, DirectionError> {
    if interval == 0
        || old_elapsed_tick.wrapping_div(interval) >= new_elapsed_tick.wrapping_div(interval)
    {
        return Ok(DirectionAdvance {
            direction: previous,
            branch_invoked: false,
        });
    }
    let direction = choose_direction(rng, stage_keys, previous)?;
    Ok(DirectionAdvance {
        direction,
        branch_invoked: true,
    })
}

/// Choose a direction, consuming one draw even if only one candidate remains.
///
/// `None` represents the game's negative initial sentinel. If no candidate
/// remains, the previous value is returned unchanged without a draw.
pub fn choose_direction(
    rng: &mut XorShift128,
    stage_keys: &[u8],
    previous: Option<u8>,
) -> Result<Option<u8>, DirectionError> {
    if let Some(invalid) = stage_keys.iter().copied().find(|key| *key > 8) {
        return Err(DirectionError::InvalidStageKey(invalid));
    }
    if let Some(invalid) = previous.filter(|value| *value > 2) {
        return Err(DirectionError::InvalidPrevious(invalid));
    }

    let mask = stage_keys
        .iter()
        .fold(0u32, |bits, key| bits | (1u32 << key));
    let mut candidates = vec![0u8, 1, 2];
    for direction in 0..3u8 {
        if mask & (1u32 << direction) == 0
            && let Some(index) = candidates.iter().position(|value| *value == direction)
        {
            candidates.swap_remove(index);
        }
    }
    if let Some(previous) = previous
        && let Some(index) = candidates.iter().position(|value| *value == previous)
    {
        candidates.swap_remove(index);
    }
    if candidates.is_empty() {
        return Ok(previous);
    }
    let value = rng.next32();
    let index = ((u64::from(value) * candidates.len() as u64) >> 32) as usize;
    Ok(Some(candidates[index]))
}

/// Predict the two initial direction selections when the ordinary JobType 0/1 queue builder runs.
///
/// The caller must establish that the stage spawner exists (`+0x6f0` non-null)
/// and the ordinary event's queue builder executes. Director initialization
/// advances the derived shared state once before either direction selection
/// (`0x375b11c..0x375b17c`). The second selector excludes the first result.
pub fn predict_initial_directions(
    seed: SeedWords,
    stage_keys: &[u8],
) -> Result<InitialDirections, DirectionError> {
    let mut rng = derive(seed, DIRECTION_DOMAIN);
    let hidden_output = rng.next32();
    let director_rng_38c = expand_seed_word(hidden_output);
    let wave_start = choose_direction(&mut rng, stage_keys, None)?;
    let after_queue_build = choose_direction(&mut rng, stage_keys, wave_start)?;
    Ok(InitialDirections {
        wave_start,
        after_queue_build,
        shared_state: rng,
        director_rng_38c,
    })
}
