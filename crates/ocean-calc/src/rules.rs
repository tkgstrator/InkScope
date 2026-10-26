//! Candidate restrictions on the ordinary event and tide draw path.
//!
//! The caller supplies actual runtime context and effective weights. Forced
//! tide, preset rounds, and the later one-player event override are separate.

use crate::wave::Candidate;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventContext {
    /// The runtime mode flag: map IsBigRun on JobType 0/1, forced true when
    /// the observed nonnull runtime mode object's value is JobType 4.
    pub mode_one: bool,
    /// Bit `1 << event_id` is set for a stage-banned event.
    pub ban_event_mask: u32,
    /// Raw value read at 0x60f65c8. The comparison uses `global_word >> 8`.
    pub global_word: u32,
    pub previous_event: u8,
}

pub fn event_candidates(weights: [u32; 9], context: EventContext) -> [Candidate; 9] {
    std::array::from_fn(|id| {
        let mode_rejects = context.mode_one && matches!(id, 2 | 6 | 7 | 8);
        let stage_rejects = id != 0 && (context.ban_event_mask & (1 << id)) != 0;
        let global_rejects = (context.global_word >> 8) <= 256 && matches!(id, 4 | 7 | 8);
        let repeat_rejects =
            context.previous_event != 0 && usize::from(context.previous_event) == id;
        Candidate {
            weight: weights[id],
            eligible: !(mode_rejects || stage_rejects || global_rejects || repeat_rejects),
        }
    })
}

pub fn tide_candidates(weights: [u32; 3], mode_one: bool, event_id: u8) -> [Candidate; 3] {
    let reject_zero = if mode_one {
        matches!(event_id, 2 | 8)
    } else {
        matches!(event_id, 1 | 2 | 3 | 8)
    };
    std::array::from_fn(|id| Candidate {
        weight: weights[id],
        eligible: id != 0 || !reject_zero,
    })
}
