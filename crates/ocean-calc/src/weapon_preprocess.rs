//! Default equipment-subtype RNG advancement across 11.3.0 list-preprocessing shuffles.
//!
//! Counts describe effective lists after runtime filtering, not static RSDB rows.
//! The caller must establish which branch ran from the runtime `G >> 17 <= 2`
//! condition. A result is valid only when the selected branch completed every
//! shuffle represented here on its normal path. Resource failures and early exits
//! can leave a different state. Possible mutation by indirect callbacks remains
//! unproven.
//!
//! The public weapon APIs use the default subtype's domain `0xa0c1474b`,
//! verified for JobType 0. The internal shared shuffle helper also accepts the
//! JobType 1 special subtype's domain. These APIs do not
//! infer weapon IDs, pool contents or order, asset lookup results, or whether
//! the later 20-slot node loop ran.

use crate::{
    rng::{SeedWords, XorShift128, derive},
    weapons::WEAPON_RNG_DOMAIN,
};

/// The effective list sizes observed at the selected preprocessing branch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreprocessShape {
    /// Effective low-branch list length, with capacity eight.
    Low { n: u8 },
    /// Effective high-branch first-list length (at most nine) and the actual
    /// number appended to the second work list (at most nine). The resulting
    /// second work list has capacity nine; the final selection has at most four.
    High { a: u8, second_added: u8 },
}

/// State derived with the selected equipment subtype's RNG domain after shuffles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreprocessRng {
    pub state: XorShift128,
    pub draws: u8,
}

/// Ordered opaque candidate-pointer identities reaching the source loops.
///
/// Each stream follows reverse descriptor-chain traversal and ascending index
/// within each descriptor. Descriptor/group readiness has already been
/// checked, but per-candidate asset lookup has not yet occurred. The chosen
/// branch and these streams are runtime inputs, not derived from the seed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreprocessCandidateStreams<'a> {
    Low(&'a [u64]),
    High { first: &'a [u64], second: &'a [u64] },
}

/// Candidate-pointer identities in the order passed to later lookup.
///
/// A subsequent lookup or output-capacity check may discard an identity, so
/// these are neither final weapon IDs nor the 20 generated weapon nodes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreprocessCandidatesResult {
    pub candidates: Vec<u64>,
    pub rng: PreprocessRng,
}

/// An input count exceeds a list capacity seen in the preprocessing path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreprocessError {
    LowCountTooLarge(u8),
    FirstCountTooLarge(u8),
    SecondAddedTooLarge(u8),
    SecondCountTooLarge(u8),
}

/// Advance the derived weapon RNG by the selected branch's shuffle draws.
///
/// This is valid only if the branch completed every stated shuffle on its
/// normal path. Each shuffle of length `N` consumes `N.saturating_sub(1)` draws.
/// For `High`, `B = A - min(A, 2) + second_added` and
/// `M = min(A, 2) + min(B, 2)`; the draw count is the sum for `A`, `B`, and `M`.
/// Indirect callback RNG mutation has not been ruled out. This result cannot
/// identify a weapon or establish that the subsequent 20-slot node loop ran.
pub fn weapon_rng_after_preprocess(
    seed: SeedWords,
    shape: PreprocessShape,
) -> Result<PreprocessRng, PreprocessError> {
    let draws = match shape {
        PreprocessShape::Low { n } => {
            if n > 8 {
                return Err(PreprocessError::LowCountTooLarge(n));
            }
            n.saturating_sub(1)
        },
        PreprocessShape::High { a, second_added } => {
            if a > 9 {
                return Err(PreprocessError::FirstCountTooLarge(a));
            }
            if second_added > 9 {
                return Err(PreprocessError::SecondAddedTooLarge(second_added));
            }
            let first_selected = a.min(2);
            let b = a - first_selected + second_added;
            if b > 9 {
                return Err(PreprocessError::SecondCountTooLarge(b));
            }
            let selected = first_selected + b.min(2);
            a.saturating_sub(1) + b.saturating_sub(1) + selected.saturating_sub(1)
        },
    };

    let mut state = derive(seed, WEAPON_RNG_DOMAIN);
    for _ in 0..draws {
        state.next32();
    }
    Ok(PreprocessRng { state, draws })
}

/// Reproduce the selected branch's candidate order and shared weapon RNG.
///
/// The low branch appends up to eight source candidates. The high branch
/// appends up to nine from the first source, shuffles, takes up to two
/// head entries, appends from the second source until the remaining work
/// list reaches nine, shuffles and takes up to two more heads, then shuffles the
/// selected entries. Identities are opaque and duplicates remain distinct
/// entries. Validity assumes every represented shuffle completed and indirect
/// callbacks did not mutate the same RNG state.
pub fn preprocess_weapon_candidates(
    seed: SeedWords,
    streams: PreprocessCandidateStreams<'_>,
) -> PreprocessCandidatesResult {
    let streams = match streams {
        PreprocessCandidateStreams::Low(source) => CandidateStreams::Low(source),
        PreprocessCandidateStreams::High { first, second } => {
            CandidateStreams::High { first, second }
        },
    };
    let (candidates, rng) = preprocess_candidates(seed, WEAPON_RNG_DOMAIN, streams);
    PreprocessCandidatesResult { candidates, rng }
}

pub(crate) enum CandidateStreams<'a, T> {
    Low(&'a [T]),
    High { first: &'a [T], second: &'a [T] },
}

/// Shared list operations; the caller supplies the equipment subtype's domain.
pub(crate) fn preprocess_candidates<T: Copy>(
    seed: SeedWords,
    domain: u32,
    streams: CandidateStreams<'_, T>,
) -> (Vec<T>, PreprocessRng) {
    let mut state = derive(seed, domain);
    let mut draws = 0u8;
    let candidates = match streams {
        CandidateStreams::Low(source) => {
            let mut candidates: Vec<T> = source.iter().copied().take(8).collect();
            shuffle_candidates(&mut candidates, &mut state, &mut draws);
            candidates
        },
        CandidateStreams::High { first, second } => {
            let mut work: Vec<T> = first.iter().copied().take(9).collect();
            shuffle_candidates(&mut work, &mut state, &mut draws);
            let mut chosen = Vec::with_capacity(4);
            let selected = work.len().min(2);
            chosen.extend(work.drain(..selected));
            for &candidate in second {
                if work.len() == 9 {
                    break;
                }
                work.push(candidate);
            }
            shuffle_candidates(&mut work, &mut state, &mut draws);
            let selected = work.len().min(2);
            chosen.extend(work.drain(..selected));
            shuffle_candidates(&mut chosen, &mut state, &mut draws);
            chosen
        },
    };
    (candidates, PreprocessRng { state, draws })
}

fn shuffle_candidates<T>(candidates: &mut [T], state: &mut XorShift128, draws: &mut u8) {
    for remaining in (2..=candidates.len()).rev() {
        let value = state.next32();
        *draws += 1;
        let index = ((remaining as u64 * u64::from(value)) >> 32) as usize;
        candidates.swap(remaining - 1, index);
    }
}
