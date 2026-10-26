//! JobType 0/1 golden egg quota from an effective Round base and player count.
//!
//! The ELF calculates a quota when a WAVE begins, but later transitions can
//! write the applied field again. Stock-source results here are conditional
//! on the live level object matching the owned 11.3.0 RomFS rows.

use crate::levels::common_round_quota_base;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuotaError {
    InvalidWaveIndex(u8),
    MissingSceneOrSequence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockWaveQuota {
    pub source_round_index: u8,
    pub source_base: i32,
    pub quota: i32,
}

/// Scale the effective Round's base quota for JobType 0/1.
///
/// `active_players` is the live value returned by `0x385aaa0` at this quota
/// calculation, not the room capacity. `special_zero_branch_taken` means the
/// early-zero path at `0x37b27c4..0x37b27d8` was actually reached and taken;
/// a raw matching scene flag is insufficient when the sequence object is null.
/// Otherwise, `0x37b2af0..0x37b2b1c` returns zero for a zero base, or clamps the signed
/// 32-bit wrapping product divided by four toward zero to at least one.
pub fn job_zero_one_quota(
    effective_round_base: i32,
    active_players: i32,
    special_zero_branch_taken: bool,
) -> i32 {
    if special_zero_branch_taken || effective_round_base == 0 {
        return 0;
    }
    effective_round_base
        .wrapping_mul(active_players)
        .wrapping_div(4)
        .max(1)
}

/// Five-slot preview under the stock three-Round source array.
///
/// The effective Round array must still contain the owned three rows in
/// order, both the scene and sequence manager must exist, `wave_index` must
/// be that manager's live current index, and `active_players` must be
/// observed for this WAVE's quota call. A null sequence manager instead uses
/// index zero and skips the special-state comparison in the ELF; this helper
/// deliberately rejects that different path.
/// WAVE indices 3/4 reuse source Round 2. This predicts the formula's value,
/// not every later write to the applied scene quota field.
pub fn stock_job_zero_one_wave_quota(
    difficulty: u32,
    wave_index: u8,
    active_players: i32,
    special_zero_branch_taken: bool,
    scene_and_sequence_present: bool,
) -> Result<StockWaveQuota, QuotaError> {
    if wave_index >= 5 {
        return Err(QuotaError::InvalidWaveIndex(wave_index));
    }
    if !scene_and_sequence_present {
        return Err(QuotaError::MissingSceneOrSequence);
    }
    let source_round_index = wave_index.min(2);
    let source_base = common_round_quota_base(difficulty, source_round_index)
        .expect("source Round is constrained to 0..2");
    Ok(StockWaveQuota {
        source_round_index,
        source_base,
        quota: job_zero_one_quota(source_base, active_players, special_zero_branch_taken),
    })
}
