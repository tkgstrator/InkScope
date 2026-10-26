//! Conditional JobType 1 special assignments for owned Splatoon 3 11.3.0.
//!
//! Inputs are effective scene state and ordered runtime candidates, not a raw
//! scenario seed or assumed static pools. Candidate asset lookup happens after
//! shuffling. Results use internal player indices: the equipment callback reads
//! the list with the in-process event's `+0x18` byte. Room join order and network
//! payload correspondence remain unproven.

use crate::{
    rng::SeedWords,
    weapon_preprocess::{CandidateStreams, PreprocessRng, preprocess_candidates},
};

const PRIVATE_SPECIAL_RNG_DOMAIN: u32 = 0x3529_79a0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecialBranch {
    Low,
    High,
}

/// An explicit asset lookup result; unknown evidence cannot become a guessed ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecialLookup {
    Known(u16),
    Missing,
    Unknown,
}

/// Physical main-weapon IDs plus the runtime ring's logical start and count.
/// Capacity is the slice length; healthy rings have capacity at most four.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MainRing<'a> {
    pub physical_ids: &'a [i32],
    pub start: usize,
    pub count: usize,
}

impl MainRing<'_> {
    fn validate(self, ring: MainRingKind) -> Result<(), SpecialError> {
        let capacity = self.physical_ids.len();
        let valid_start = if capacity == 0 {
            self.start == 0
        } else {
            self.start < capacity
        };
        if capacity > 4 || self.count > capacity || !valid_start {
            return Err(SpecialError::InvalidRing { ring });
        }
        Ok(())
    }

    fn logical_ids(self) -> impl Iterator<Item = i32> {
        (0..self.count)
            .map(move |index| self.physical_ids[(self.start + index) % self.physical_ids.len()])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MainRingKind {
    Current,
    Source,
}

/// The caller establishes the scene gate; `Active` means `+0x328 != 0` and
/// `+0x79c == 1`. Matching main rings must still meet `active_count`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecialReorder<'a> {
    Inactive,
    Active {
        current_main_ring: MainRing<'a>,
        source_main_ring: MainRing<'a>,
        /// Filtered runtime participant count, in 0..=4 (zero passes the gate).
        active_count: u8,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateSpecialInput<'a> {
    /// Four words registered by ordinary scene initialization for the shared
    /// RNG object. A raw scenario array or an all-zero object on a nonstandard
    /// path is not equivalent evidence for this input.
    pub seed: SeedWords,
    /// Scene `+0x798` when `+0x328 != 0`, otherwise global `0x60f65c8`.
    pub effective_g: u32,
    pub low_candidates: &'a [u16],
    pub prior_candidates: &'a [u16],
    pub v6_candidates: &'a [u16],
    pub reorder: SpecialReorder<'a>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateSpecialPrediction {
    pub branch: SpecialBranch,
    /// State after special selection, before the private builder's later main
    /// weapon draws. Remapping consumes no draws.
    pub rng: PreprocessRng,
    /// Index is the internal player index, not an established room join order.
    pub initial_specials: [u16; 4],
    pub final_specials: [u16; 4],
    pub remap_applied: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecialError {
    UnknownAsset { candidate: u16 },
    LowMissingAsset { candidate: u16 },
    InsufficientSpecials { found: usize },
    InvalidRing { ring: MainRingKind },
    InvalidActiveCount(u8),
}

/// Resolve at most four shuffled candidates, requiring a full assignment.
///
/// Low source lists retain up to eight entries; high source lists use the
/// shared nine-entry work-list path. Missing low lookups are unsupported;
/// missing high lookups skip that selected entry without resampling.
pub fn predict_private_specials(
    input: PrivateSpecialInput<'_>,
    mut lookup: impl FnMut(u16) -> SpecialLookup,
) -> Result<PrivateSpecialPrediction, SpecialError> {
    let (branch, streams) = if input.effective_g >> 17 <= 2 {
        (
            SpecialBranch::Low,
            CandidateStreams::Low(input.low_candidates),
        )
    } else {
        (
            SpecialBranch::High,
            CandidateStreams::High {
                first: input.prior_candidates,
                second: input.v6_candidates,
            },
        )
    };
    let (candidates, rng) = preprocess_candidates(input.seed, PRIVATE_SPECIAL_RNG_DOMAIN, streams);
    let mut initial_specials = [0; 4];
    let mut found = 0;
    // The low path visits only the first min(capacity, candidate count)
    // positions; the high selected list already has at most four positions.
    for candidate in candidates.into_iter().take(initial_specials.len()) {
        match lookup(candidate) {
            SpecialLookup::Known(id) => {
                initial_specials[found] = id;
                found += 1;
            },
            SpecialLookup::Missing if branch == SpecialBranch::High => {},
            SpecialLookup::Missing => return Err(SpecialError::LowMissingAsset { candidate }),
            SpecialLookup::Unknown => return Err(SpecialError::UnknownAsset { candidate }),
        }
    }
    if found != initial_specials.len() {
        return Err(SpecialError::InsufficientSpecials { found });
    }
    let mut final_specials = initial_specials;
    let mut remap_applied = false;
    if let SpecialReorder::Active {
        current_main_ring,
        source_main_ring,
        active_count,
    } = input.reorder
    {
        if active_count > 4 {
            return Err(SpecialError::InvalidActiveCount(active_count));
        }
        current_main_ring.validate(MainRingKind::Current)?;
        source_main_ring.validate(MainRingKind::Source)?;
        let mut source_indices = [None; 4];
        let mut matched = 0;
        for (destination, main_id) in current_main_ring.logical_ids().enumerate() {
            source_indices[destination] = source_main_ring
                .logical_ids()
                .position(|source_id| source_id == main_id);
            if source_indices[destination].is_some() {
                matched += 1;
            }
        }
        if matched >= active_count {
            remap_applied = true;
            for (destination, source) in source_indices.into_iter().enumerate() {
                if let Some(source) = source {
                    final_specials[destination] = initial_specials[source];
                }
            }
        }
    }
    Ok(PrivateSpecialPrediction {
        branch,
        rng,
        initial_specials,
        final_specials,
        remap_applied,
    })
}
