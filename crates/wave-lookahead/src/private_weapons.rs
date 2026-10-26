//! Conditional five-row main-weapon builder for Salmon Run JobType 1 in 11.3.0.
//!
//! The input RNG is the state after the private subtype's special selection.
//! This builder reads the same scene main-weapon ring slot in every row and
//! uses the private subtype's two distinct Type0 pools. Its four columns are
//! internal player indices, not established room join order.

use crate::{
    rng::XorShift128,
    weapon_catalog::{EffectiveWeaponPool, SourcePoolKind},
};

const ROWS: usize = 5;
const PLAYERS: usize = 4;
const RANDOM_FLAG: u32 = 0x0001_0000;

/// The full physical scene ring, including unused slots. Its slice length
/// must equal the live scene capacity at `+0xb0`, not the logical count.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateMainRing<'a> {
    pub physical_ids: &'a [i32],
    pub start: usize,
    pub count: usize,
}

impl PrivateMainRing<'_> {
    fn valid(self) -> bool {
        let capacity = self.physical_ids.len();
        capacity <= PLAYERS
            && self.count <= capacity
            && if capacity == 0 {
                self.start == 0
            } else {
                self.start < capacity
            }
    }

    fn at(self, slot: usize) -> Option<i32> {
        if slot >= self.count {
            return None;
        }
        let capacity = self.physical_ids.len();
        // The ELF subtracts capacity at most once for a healthy <=4 ring.
        let mut physical = self.start + slot;
        if physical >= capacity {
            physical -= capacity;
        }
        self.physical_ids.get(physical).copied()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PrivateWeaponInput<'a> {
    /// RNG after special selection, before any main-weapon draw.
    pub rng_after_specials: XorShift128,
    /// Caller confirmed a nonnull shared RNG object, an invoked JobType 1
    /// builder, and successful initialization of its five-row array.
    pub builder_ready_confirmed: bool,
    pub scene_main_ring: PrivateMainRing<'a>,
    /// Effective Type0 pool at private equipment object `+0x178/+0x180`.
    pub base_pool: &'a EffectiveWeaponPool,
    /// Separate Type0 membership pool at `+0x13a0/+0x13a8`.
    pub alternate_pool: &'a EffectiveWeaponPool,
    /// Caller established stable runtime global mode `+0x130` in {1,2}.
    pub mode_allows_direct: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateWeaponSource {
    DirectMode,
    DirectAlternatePool,
    BaseFallback,
    BaseDraw { index: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivateWeaponPrediction {
    /// Packed node values, indexed by WAVE row 0..4 and internal player 0..3.
    pub packed_rows: [[u32; PLAYERS]; ROWS],
    pub sources: [[PrivateWeaponSource; PLAYERS]; ROWS],
    pub draws: u8,
    pub final_rng: XorShift128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivatePool {
    Base,
    Alternate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PoolProblem {
    WrongKind(SourcePoolKind),
    LengthMismatch,
    TooManyEntries(usize),
    IdMismatch(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateWeaponError {
    BuilderNotConfirmed,
    InvalidRing {
        capacity: usize,
        start: usize,
        count: usize,
    },
    InvalidPool {
        pool: PrivatePool,
        problem: PoolProblem,
    },
    BaseEntryUnavailable {
        wave_index: u8,
        internal_player_index: u8,
    },
}

fn validate_pool(pool: &EffectiveWeaponPool, which: PrivatePool) -> Result<(), PrivateWeaponError> {
    let problem = if pool.kind != SourcePoolKind::Normal {
        Some(PoolProblem::WrongKind(pool.kind))
    } else if pool.stored_ids.len() != pool.draw_ids.len()
        || pool.stored_ids.len() != pool.sources.len()
    {
        Some(PoolProblem::LengthMismatch)
    } else if pool.stored_ids.len() > 256 {
        Some(PoolProblem::TooManyEntries(pool.stored_ids.len()))
    } else {
        pool.stored_ids
            .iter()
            .zip(&pool.draw_ids)
            .position(|(stored, draw)| *stored as u16 != *draw)
            .map(PoolProblem::IdMismatch)
    };
    if let Some(problem) = problem {
        Err(PrivateWeaponError::InvalidPool {
            pool: which,
            problem,
        })
    } else {
        Ok(())
    }
}

/// Predict packed JobType 1 main-weapon rows on the confirmed builder path.
///
/// The runtime mode value and both pools must be the values used by this
/// invocation. The builder reads the scene main ring before deciding whether
/// to draw from the base pool. A missing base entry errors only if that branch
/// actually tries to read it. No row permutation is performed here.
pub fn predict_private_weapon_rows(
    input: PrivateWeaponInput<'_>,
) -> Result<PrivateWeaponPrediction, PrivateWeaponError> {
    if !input.builder_ready_confirmed {
        return Err(PrivateWeaponError::BuilderNotConfirmed);
    }
    let ring = input.scene_main_ring;
    if !ring.valid() {
        return Err(PrivateWeaponError::InvalidRing {
            capacity: ring.physical_ids.len(),
            start: ring.start,
            count: ring.count,
        });
    }
    validate_pool(input.base_pool, PrivatePool::Base)?;
    validate_pool(input.alternate_pool, PrivatePool::Alternate)?;

    let mut rng = input.rng_after_specials;
    let mut draws = 0u8;
    let mut packed_rows = [[0; PLAYERS]; ROWS];
    let mut sources = [[PrivateWeaponSource::BaseFallback; PLAYERS]; ROWS];
    for wave_index in 0..ROWS {
        for internal_player_index in 0..PLAYERS {
            let candidate = ring.at(internal_player_index);
            let (packed, source) = match candidate {
                Some(id) if id >= 0 && input.mode_allows_direct => {
                    (id as u32 & 0xffff, PrivateWeaponSource::DirectMode)
                },
                Some(id) if id >= 0 && input.alternate_pool.stored_ids.contains(&id) => {
                    (id as u32 & 0xffff, PrivateWeaponSource::DirectAlternatePool)
                },
                Some(id) if id >= 0 => {
                    let Some(&first) = input.base_pool.draw_ids.first() else {
                        return Err(PrivateWeaponError::BaseEntryUnavailable {
                            wave_index: wave_index as u8,
                            internal_player_index: internal_player_index as u8,
                        });
                    };
                    (u32::from(first), PrivateWeaponSource::BaseFallback)
                },
                _ => {
                    let word = rng.next32();
                    draws += 1;
                    let index =
                        ((input.base_pool.draw_ids.len() as u64 * u64::from(word)) >> 32) as usize;
                    let Some(&selected) = input.base_pool.draw_ids.get(index) else {
                        return Err(PrivateWeaponError::BaseEntryUnavailable {
                            wave_index: wave_index as u8,
                            internal_player_index: internal_player_index as u8,
                        });
                    };
                    (
                        RANDOM_FLAG | u32::from(selected),
                        PrivateWeaponSource::BaseDraw { index },
                    )
                },
            };
            packed_rows[wave_index][internal_player_index] = packed;
            sources[wave_index][internal_player_index] = source;
        }
    }
    Ok(PrivateWeaponPrediction {
        packed_rows,
        sources,
        draws,
        final_rng: rng,
    })
}
