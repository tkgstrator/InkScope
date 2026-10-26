//! Default equipment-subtype weapon IDs and postprocessing for JobType 0's five wave indices.
//!
//! The ring reader runs before validation and random-pool replacement. The
//! postprocessor requires resolved packed node values after those steps and the weapon RNG
//! state after their draws. Neither step maps slots to players.

use crate::{
    rng::XorShift128,
    weapon_catalog::{EffectiveWeaponPool, SourcePoolKind},
};

pub const WEAPON_RNG_DOMAIN: u32 = 0xa0c1_474b;

/// Weapon IDs in physical ring-buffer order, not logical order.
#[derive(Clone, Copy, Debug)]
pub struct WeaponCandidateRing<'a> {
    pub physical_ids: &'a [i32],
    pub start: usize,
    pub count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeaponSlotSource {
    /// The game's validation callback can turn this ID into a random sentinel.
    CandidateNeedsValidation(i32),
    StandardRandom,
    ThirdPoolRandom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeaponRandomPool {
    HelperNormal,
    SceneRare,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeaponError {
    InvalidWaveIndex(u8),
    InvalidSlotIndex(u8),
    InvalidRing {
        capacity: usize,
        start: usize,
        count: usize,
    },
}

/// Read one prevalidation source for WAVE index 0–4 and slot 0–3.
///
/// `wave_three_override` means the runtime condition at 0x37b7850–0x37b78a4
/// is true. It applies only to index 3 and is not derived from the scene seed.
/// The game checks nonnegative IDs before this override; validation side
/// effects and all random-pool draws are outside this function.
pub fn weapon_slot_source(
    candidates: &WeaponCandidateRing<'_>,
    wave_index: u8,
    slot_index: u8,
    wave_three_override: bool,
) -> Result<WeaponSlotSource, WeaponError> {
    if wave_index > 4 {
        return Err(WeaponError::InvalidWaveIndex(wave_index));
    }
    if slot_index > 3 {
        return Err(WeaponError::InvalidSlotIndex(slot_index));
    }
    let capacity = candidates.physical_ids.len();
    if candidates.count > capacity
        || (capacity == 0 && candidates.start != 0)
        || (capacity > 0 && candidates.start >= capacity)
    {
        return Err(WeaponError::InvalidRing {
            capacity,
            start: candidates.start,
            count: candidates.count,
        });
    }

    let id = if usize::from(slot_index) >= candidates.count {
        -1
    } else {
        let rotated = usize::from((wave_index + slot_index) & 3);
        let physical_index = if rotated >= candidates.count {
            0
        } else {
            (candidates.start + rotated) % capacity
        };
        candidates.physical_ids[physical_index]
    };
    if wave_index == 3 && wave_three_override {
        return Ok(WeaponSlotSource::ThirdPoolRandom);
    }
    Ok(match id {
        -2 => WeaponSlotSource::ThirdPoolRandom,
        i if i < 0 => WeaponSlotSource::StandardRandom,
        i => WeaponSlotSource::CandidateNeedsValidation(i),
    })
}

/// Draw the index used by a `-2` slot from the post-preprocessing weapon RNG.
///
/// A negative signed count selects index zero without a draw. A zero count
/// still consumes one draw and returns zero. The caller must establish that
/// the game's pool pointer can be dereferenced; this only computes an index.
pub fn draw_third_pool_index(rng: &mut XorShift128, signed_count: i32) -> u32 {
    if signed_count < 0 {
        return 0;
    }
    let value = rng.next32();
    ((u64::from(signed_count as u32) * u64::from(value)) >> 32) as u32
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThirdPoolNode {
    pub resolved_id: u16,
    /// The packed value written if the game's output slot has capacity.
    pub packed_value: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThirdPoolError {
    MissingEntry(usize),
}

/// Resolve source ID `-2` from the effective third pool at `+0x13a8`.
///
/// The game's `ldrh` reads the selected pool pointer's ID directly, without
/// an asset lookup. Count zero still consumes a draw and reads index zero;
/// negative count skips the draw but also reads index zero. An unavailable
/// selected entry is reported instead of manufacturing a fallback ID. This
/// returns the would-be node even when the game's 20-slot array is full.
pub fn draw_third_pool_node_value(
    rng: &mut XorShift128,
    signed_count: i32,
    ids: &[u16],
) -> Result<ThirdPoolNode, ThirdPoolError> {
    let index = draw_third_pool_index(rng, signed_count) as usize;
    let Some(&resolved_id) = ids.get(index) else {
        return Err(ThirdPoolError::MissingEntry(index));
    };
    Ok(ThirdPoolNode {
        resolved_id,
        packed_value: (0x0101_0000_u32 | u32::from(resolved_id)) as i32,
    })
}

/// Pick the pool branch for a negative weapon ID other than `-2`.
///
/// `threshold` is the effective runtime f32 at the configuration child's
/// `+0x80`, not necessarily its constructor default of 0.2. This consumes
/// exactly one draw from the post-preprocessing weapon RNG. An unordered
/// comparison (NaN) follows the game's scene-rare fallthrough.
pub fn choose_standard_random_pool(rng: &mut XorShift128, threshold: f32) -> WeaponRandomPool {
    let value = rng.next32();
    let sample = f32::from_bits(0x3f80_0000 | (value >> 9)) - 1.0;
    if threshold <= sample {
        WeaponRandomPool::HelperNormal
    } else {
        WeaponRandomPool::SceneRare
    }
}

/// Effective helper-normal pool after game-side asset filtering.
///
/// The signed count controls the draw. Even count zero or negative can lead
/// the executable to read element zero. `ids` is indexed in the game's pool
/// order, starting at zero, and must contain the selected entry.
#[derive(Clone, Copy, Debug)]
pub struct HelperNormalPool<'a> {
    pub signed_count: i32,
    pub ids: &'a [u16],
}

/// Effective scene-rare ring after scenario processing.
#[derive(Clone, Copy, Debug)]
pub struct SceneRarePool<'a> {
    pub physical_ids: &'a [i32],
    pub start: usize,
    /// Count at the scene branch; values below one select candidate ID -1.
    pub signed_count: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct StandardRandomInput<'a> {
    /// A negative source other than -2, after the nonnegative validation gate.
    pub source_id: i32,
    pub threshold: f32,
    pub normal: HelperNormalPool<'a>,
    /// `None` means the scene-rare object is absent.
    pub scene_rare: Option<SceneRarePool<'a>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StandardRandomNode {
    pub pool: WeaponRandomPool,
    pub resolved_id: i32,
    /// The packed value the game would write if this slot has capacity.
    pub packed_value: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StandardRandomError {
    UnsupportedSource(i32),
    MissingNormalEntry(usize),
    InvalidSceneRing {
        capacity: usize,
        start: usize,
        count: i32,
    },
}

/// Resolve a standard random (`-1` or another negative except `-2`) node.
///
/// The caller supplies the RNG after preprocessing and earlier slots, the
/// effective pools, and the scene-rare asset lookup result. `asset_usable`
/// represents both a nonnull lookup and its `+0xe0` validity byte; it is
/// called only for a present scene-rare pool. The returned packed value is
/// what 0x37b7d74..0x37b7d84 would write *if* the output array has room.
/// Errors mean a required source entry was unavailable to this model; prior
/// draws made by this call are not rolled back.
pub fn draw_standard_random_node_value(
    rng: &mut XorShift128,
    input: StandardRandomInput<'_>,
    mut asset_usable: impl FnMut(i32) -> bool,
) -> Result<StandardRandomNode, StandardRandomError> {
    if input.source_id >= 0 || input.source_id == -2 {
        return Err(StandardRandomError::UnsupportedSource(input.source_id));
    }
    let pool = choose_standard_random_pool(rng, input.threshold);
    let resolved_id = match pool {
        WeaponRandomPool::HelperNormal => {
            let index = draw_third_pool_index(rng, input.normal.signed_count) as usize;
            let Some(&id) = input.normal.ids.get(index) else {
                return Err(StandardRandomError::MissingNormalEntry(index));
            };
            i32::from(id)
        },
        WeaponRandomPool::SceneRare => {
            if let Some(rare) = input.scene_rare {
                let candidate = if rare.signed_count < 1 {
                    -1
                } else {
                    let capacity = rare.physical_ids.len();
                    if rare.start >= capacity
                        || usize::try_from(rare.signed_count).unwrap_or(usize::MAX) > capacity
                    {
                        return Err(StandardRandomError::InvalidSceneRing {
                            capacity,
                            start: rare.start,
                            count: rare.signed_count,
                        });
                    }
                    let offset = if rare.signed_count >= 2 {
                        let value = rng.next32();
                        ((u64::from(rare.signed_count as u32) * u64::from(value)) >> 32) as usize
                    } else {
                        0
                    };
                    rare.physical_ids[(rare.start + offset) % capacity]
                };
                if asset_usable(candidate) {
                    candidate
                } else {
                    20_900
                }
            } else {
                20_900
            }
        },
    };
    let packed_value = (0x0101_0000_u32 | (resolved_id as u32 & 0xffff)) as i32;
    Ok(StandardRandomNode {
        pool,
        resolved_id,
        packed_value,
    })
}

/// Validation order for a nonnegative full 32-bit source ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeaponPoolMembership {
    Third,
    Normal,
    Missing,
}

/// Check full pool-node IDs before any random replacement draw.
///
/// The game searches the third pool before the normal pool. The later random
/// draws read only the selected node's low 16 bits; those are not sufficient
/// for this membership test.
pub fn weapon_pool_membership(
    id: i32,
    normal: &EffectiveWeaponPool,
    third: &EffectiveWeaponPool,
) -> WeaponPoolMembership {
    if third.stored_ids.contains(&id) {
        WeaponPoolMembership::Third
    } else if normal.stored_ids.contains(&id) {
        WeaponPoolMembership::Normal
    } else {
        WeaponPoolMembership::Missing
    }
}

/// Runtime inputs for the conditional 5 × 4 pre-postprocess weapon grid.
///
/// `ready` means the caller established that the normal allocation and
/// registry/mode pointers reached this loop. The candidate ring and pools
/// must represent the state after preprocessing. The asset callback given to
/// [`build_weapon_grid`] supplies a known result and must not advance its RNG.
#[derive(Clone, Copy, Debug)]
pub struct WeaponGridBuildInput<'a> {
    pub ready: bool,
    pub candidates: WeaponCandidateRing<'a>,
    pub wave_three_override: bool,
    pub normal: &'a EffectiveWeaponPool,
    pub third: &'a EffectiveWeaponPool,
    pub threshold: f32,
    pub scene_rare: Option<SceneRarePool<'a>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeaponGridBuildError {
    NormalPoolKind(SourcePoolKind),
    ThirdPoolKind(SourcePoolKind),
    MalformedNormalPool,
    MalformedThirdPool,
    Slot(WeaponError),
    Third(ThirdPoolError),
    Standard(StandardRandomError),
}

/// Build all twenty packed nodes in row-major WAVE/slot order.
///
/// A validated nonnegative ID packs as `ID & 0xffff` with zero high bits.
/// Random replacement paths use the existing helpers and their `0x01010000`
/// prefix. Errors preserve every draw already consumed on the attempted path;
/// the returned grid is the normal successful 20-slot output-capacity path.
pub fn build_weapon_grid(
    rng: &mut XorShift128,
    input: WeaponGridBuildInput<'_>,
    mut asset_usable: impl FnMut(i32) -> bool,
) -> Result<Option<[[i32; 4]; 5]>, WeaponGridBuildError> {
    if !input.ready {
        return Ok(None);
    }
    if input.normal.kind != SourcePoolKind::Normal {
        return Err(WeaponGridBuildError::NormalPoolKind(input.normal.kind));
    }
    if input.third.kind != SourcePoolKind::ThirdRare {
        return Err(WeaponGridBuildError::ThirdPoolKind(input.third.kind));
    }
    if !effective_pool_is_aligned(input.normal) {
        return Err(WeaponGridBuildError::MalformedNormalPool);
    }
    if !effective_pool_is_aligned(input.third) {
        return Err(WeaponGridBuildError::MalformedThirdPool);
    }

    let mut grid = [[0_i32; 4]; 5];
    for (wave, row) in grid.iter_mut().enumerate() {
        for (slot, packed) in row.iter_mut().enumerate() {
            let source = weapon_slot_source(&input.candidates, wave as u8, slot as u8, false)
                .map_err(WeaponGridBuildError::Slot)?;
            // The game validates a nonnegative ring ID before the WAVE-3
            // override. This membership scan is pure and consumes no RNG.
            let membership = match source {
                WeaponSlotSource::CandidateNeedsValidation(id) => {
                    weapon_pool_membership(id, input.normal, input.third)
                },
                _ => WeaponPoolMembership::Missing,
            };
            let source = if wave == 3 && input.wave_three_override {
                WeaponSlotSource::ThirdPoolRandom
            } else {
                source
            };
            *packed = match source {
                WeaponSlotSource::CandidateNeedsValidation(id)
                    if membership != WeaponPoolMembership::Missing =>
                {
                    (id as u32 & 0xffff) as i32
                },
                WeaponSlotSource::ThirdPoolRandom => {
                    draw_third_pool_node_value(
                        rng,
                        input.third.draw_ids.len() as i32,
                        &input.third.draw_ids,
                    )
                    .map_err(WeaponGridBuildError::Third)?
                    .packed_value
                },
                WeaponSlotSource::StandardRandom
                | WeaponSlotSource::CandidateNeedsValidation(_) => {
                    draw_standard_random_node_value(
                        rng,
                        StandardRandomInput {
                            source_id: -1,
                            threshold: input.threshold,
                            normal: HelperNormalPool {
                                signed_count: input.normal.draw_ids.len() as i32,
                                ids: &input.normal.draw_ids,
                            },
                            scene_rare: input.scene_rare,
                        },
                        &mut asset_usable,
                    )
                    .map_err(WeaponGridBuildError::Standard)?
                    .packed_value
                },
            };
        }
    }
    Ok(Some(grid))
}

fn effective_pool_is_aligned(pool: &EffectiveWeaponPool) -> bool {
    pool.stored_ids.len() == pool.draw_ids.len()
        && pool.stored_ids.len() == pool.sources.len()
        && pool
            .stored_ids
            .iter()
            .zip(&pool.draw_ids)
            .all(|(&stored, &draw)| stored as u16 == draw)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeaponGridSources {
    /// Original slot index now occupying each postprocessed slot, shared by
    /// all five WAVE rows.
    pub slot_sources: [u8; 4],
    /// Original WAVE row now occupying each row. Row 4 is a copy of another
    /// row, so this mapping can contain a duplicate.
    pub wave_sources: [u8; 5],
}

/// Run the seven weapon-grid draws after the 5 × 4 packed values are filled.
///
/// This is the default subtype (verified JobType 0) path at 0x37b7ea0–0x37b84f4. Supply the
/// weapon RNG *after* 0x37b3da0 preprocessing, all validation callbacks, and
/// every random-pool draw for the twenty values. `skip_wave_three_swap` is the
/// runtime WAVE-3 override condition; its fourth draw is consumed even when
/// the swap is skipped. The returned grid precedes per-slot sequence building,
/// duplicate removal, and assignment to players.
pub fn postprocess_weapon_grid(
    rng: &mut XorShift128,
    grid: &mut [[i32; 4]; 5],
    skip_wave_three_swap: bool,
) -> WeaponGridSources {
    let mut slot_sources = [0, 1, 2, 3];
    for last in (1..=3).rev() {
        let chosen = multiply_high_index(rng.next32(), last + 1);
        for wave in grid.iter_mut() {
            wave.swap(chosen, last);
        }
        slot_sources.swap(chosen, last);
    }

    let mut wave_sources = [0, 1, 2, 3, 4];
    let chosen = multiply_high_index(rng.next32(), 4);
    if !skip_wave_three_swap {
        grid.swap(chosen, 3);
        wave_sources.swap(chosen, 3);
    }
    for last in (1..=2).rev() {
        let chosen = multiply_high_index(rng.next32(), last + 1);
        grid.swap(chosen, last);
        wave_sources.swap(chosen, last);
    }
    let copied = multiply_high_index(rng.next32(), 3);
    grid[4] = grid[copied];
    wave_sources[4] = wave_sources[copied];
    WeaponGridSources {
        slot_sources,
        wave_sources,
    }
}

/// Transpose the postprocessed grid into four slot lists and remove duplicates.
///
/// The comparison at 0x37b8890–0x37b8960 uses only the low 16 bits of each
/// packed value, while keeping its full 32 bits. The game's inner index
/// advances even after removal, so shifted neighbors can remain duplicated.
/// These are slot lists, not player mappings.
pub fn weapon_slot_sequences(grid: &[[i32; 4]; 5]) -> [Vec<i32>; 4] {
    std::array::from_fn(|slot| {
        let mut sequence: Vec<i32> = grid.iter().map(|row| row[slot]).collect();
        let mut i = 0;
        while i + 1 < sequence.len() {
            let mut j = i + 1;
            while j < sequence.len() {
                if (sequence[i] as u32 & 0xffff) == (sequence[j] as u32 & 0xffff) {
                    sequence.remove(j);
                }
                j += 1;
            }
            i += 1;
        }
        sequence
    })
}

fn multiply_high_index(value: u32, count: usize) -> usize {
    ((u64::from(value) * count as u64) >> 32) as usize
}
