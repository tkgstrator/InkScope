//! Nominal Rare queue times for event IDs 0, 5, and 6 on JobType 0/1.
//!
//! The game may reject or change an actual spawn later. The caller provides
//! the branch selected by runtime context; this module does not infer it.

use crate::rng::{SeedWords, derive};

pub const SCHEDULE_DOMAIN: u32 = 0xe6da_1c4b;
const DURATION_TICKS: u32 = 4320;
const QUEUE_CAPACITY: u32 = 48;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduleBranch {
    Normal,
    Special,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    Base,
    Extra,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NominalEntry {
    pub tick: u32,
    pub kind: EntryKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleInput {
    pub seed: SeedWords,
    pub event_type: u8,
    pub wave_change_num: u32,
    pub rare_appear_num: u32,
    /// A caller-supplied assumption about the game's runtime branch.
    pub assumed_branch: ScheduleBranch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduleError {
    UnsupportedEvent(u8),
    InvalidCounts,
    CapacityExceeded,
}

pub fn nominal_schedule(input: ScheduleInput) -> Result<Vec<NominalEntry>, ScheduleError> {
    // JobType 6 shuffles this queue; the caller must use the JobType 0/1 path.
    if !matches!(input.event_type, 0 | 5 | 6) {
        return Err(ScheduleError::UnsupportedEvent(input.event_type));
    }
    let n = input.wave_change_num;
    let r = input.rare_appear_num;
    if n < 2 {
        return Err(ScheduleError::InvalidCounts);
    }
    if n > QUEUE_CAPACITY || r > QUEUE_CAPACITY {
        return Err(ScheduleError::CapacityExceeded);
    }

    let interval = DURATION_TICKS / (n - 1);
    let m = n - 1;
    let mut rng = derive(input.seed, SCHEDULE_DOMAIN);
    let mut entries = Vec::with_capacity(n.max(r) as usize);
    for i in 0..n {
        entries.push(NominalEntry {
            tick: i * interval,
            kind: EntryKind::Base,
        });
    }
    for j in 0..r.saturating_sub(n) {
        let k = match input.assumed_branch {
            ScheduleBranch::Normal => m - 1 - (j % m),
            ScheduleBranch::Special => j % m,
        };
        let tick = if input.assumed_branch == ScheduleBranch::Special && k == 0 {
            0
        } else {
            let value = rng.next32();
            k * interval + ((u64::from(value) * u64::from(interval)) >> 32) as u32
        };
        entries.push(NominalEntry {
            tick,
            kind: EntryKind::Extra,
        });
    }
    game_heap_sort_by_tick(&mut entries);
    Ok(entries)
}

/// Mirror the in-place max-heapsort at 0x3763f50..0x376410c. Equal ticks
/// keep the left child and stop sifting, but extraction can still reorder
/// equal entries. The queue consumer observes this order.
fn game_heap_sort_by_tick(entries: &mut [NominalEntry]) {
    fn sift_down(entries: &mut [NominalEntry], start: usize, end: usize) {
        let mut root = start;
        let value = entries[root];
        while let Some(mut child) = root.checked_mul(2).and_then(|index| index.checked_add(1)) {
            if child >= end {
                break;
            }
            if child + 1 < end && entries[child].tick < entries[child + 1].tick {
                child += 1;
            }
            if value.tick >= entries[child].tick {
                break;
            }
            entries[root] = entries[child];
            root = child;
        }
        entries[root] = value;
    }

    for start in (0..entries.len() / 2).rev() {
        sift_down(entries, start, entries.len());
    }
    for end in (1..entries.len()).rev() {
        entries.swap(0, end);
        sift_down(entries, 0, end);
    }
}
