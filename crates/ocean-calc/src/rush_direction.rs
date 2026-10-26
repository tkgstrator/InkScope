//! Conditional common-Zako direction trace for applied Event 1 (Rush).
//!
//! The ordinary JobType 0/1 queue builder must have run with a nonnull stage
//! spawner and positive `WaveChangeNum`. Rush skips the ordinary Rare queue,
//! so its direct path has no Rare species/spot draws between direction updates.
//! The caller supplies every scheduler-call elapsed tick from WAVE start and
//! runtime-equivalent stage keys. Unmodeled callbacks must leave the shared
//! `+0x37c` RNG, held `+0x3bc` direction, and keys unchanged.
//!
//! Directions are game group numbers, not compass bearings or confirmed spawns.

use crate::{
    direction::{
        DirectionError, advance_direction_for_elapsed, direction_interval,
        predict_initial_directions,
    },
    rng::SeedWords,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RushDirectionCall {
    pub call_index: usize,
    pub elapsed_tick: i32,
    pub direction: Option<u8>,
    /// Whether the scheduler invoked its direction selector in this call.
    pub branch_invoked: bool,
    pub shared_state_after: [u32; 4],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RushDirectionTrace {
    pub wave_start: Option<u8>,
    pub after_queue_build: Option<u8>,
    pub calls: Vec<RushDirectionCall>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RushDirectionError {
    InvalidWaveChangeNum(i32),
    InvalidElapsedTick {
        call_index: usize,
        previous_tick: i32,
        elapsed_tick: i32,
    },
    Direction(DirectionError),
}

/// Trace the direction held when each Rush scheduler call reaches the Zako timer.
///
/// `elapsed_ticks` must list every actual scheduler call from WAVE start through
/// the target call, including repeated ticks. The game draws at most once per
/// call even when one call crosses several direction intervals; this function
/// cannot detect omitted calls. It does not decide whether the Zako timer
/// reaches species selection, whether a spot is selected, or actor creation.
pub fn trace_rush_directions(
    seed: SeedWords,
    stage_keys: &[u8],
    wave_change_num: i32,
    elapsed_ticks: &[i32],
) -> Result<RushDirectionTrace, RushDirectionError> {
    if wave_change_num <= 0 {
        return Err(RushDirectionError::InvalidWaveChangeNum(wave_change_num));
    }
    let mut previous_tick = -1;
    for (call_index, &elapsed_tick) in elapsed_ticks.iter().enumerate() {
        if elapsed_tick < 0 || elapsed_tick < previous_tick {
            return Err(RushDirectionError::InvalidElapsedTick {
                call_index,
                previous_tick,
                elapsed_tick,
            });
        }
        previous_tick = elapsed_tick;
    }

    let initial =
        predict_initial_directions(seed, stage_keys).map_err(RushDirectionError::Direction)?;
    let mut shared_rng = initial.shared_state;
    let mut direction = initial.after_queue_build;
    let mut previous_tick = -1;
    let interval = direction_interval(wave_change_num);
    let mut calls = Vec::with_capacity(elapsed_ticks.len());
    for (call_index, &elapsed_tick) in elapsed_ticks.iter().enumerate() {
        let advanced = advance_direction_for_elapsed(
            &mut shared_rng,
            stage_keys,
            direction,
            previous_tick,
            elapsed_tick,
            interval,
        )
        .map_err(RushDirectionError::Direction)?;
        direction = advanced.direction;
        calls.push(RushDirectionCall {
            call_index,
            elapsed_tick,
            direction,
            branch_invoked: advanced.branch_invoked,
            shared_state_after: shared_rng.as_array(),
        });
        previous_tick = elapsed_tick;
    }
    Ok(RushDirectionTrace {
        wave_start: initial.wave_start,
        after_queue_build: initial.after_queue_build,
        calls,
    })
}
