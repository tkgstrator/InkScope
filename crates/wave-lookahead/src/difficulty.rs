//! Effective 11.3.0 difficulty for the ordinary JobType 0/1 paths.
//!
//! These functions need scene fields or live participant records. They do not
//! infer the selected private-job scenario's JobType or MatchMode.

const MAX_DIFFICULTY: i32 = 1_665;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParticipantRating {
    /// Raw participant-record field `+0x120`; only zero records are included.
    pub status_word: u32,
    /// Signed 32-bit participant-record difficulty at `+0x9d8`.
    pub difficulty: i32,
}

/// Average included participants using the game's wrapping 32-bit sum.
pub fn effective_difficulty_job_zero(records: &[ParticipantRating]) -> u32 {
    let mut sum = 0i32;
    let mut count = 0i32;
    for record in records {
        if record.status_word == 0 {
            sum = sum.wrapping_add(record.difficulty);
            count += 1;
        }
    }
    if count == 0 {
        return 0;
    }
    (sum / count).clamp(0, MAX_DIFFICULTY) as u32
}

/// MatchMode's conversion table maps only raw 11 to the scene override path.
pub fn effective_difficulty_job_one(match_mode: u32, scene_difficulty: i32) -> u32 {
    if match_mode != 11 {
        return 0;
    }
    scene_difficulty.clamp(0, MAX_DIFFICULTY) as u32
}
