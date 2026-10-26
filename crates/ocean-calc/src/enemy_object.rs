//! Initial RNG state for one enemy object at WAVE start.
//!
//! The ordinary scenario initialization path normalizes an all-zero scene
//! seed before the game stores it in the global object. `SeedWords` models
//! that path. A direct call to the game's seeder with anomalous raw-zero
//! global words has a different fallback and is outside this API.

use crate::rng::{SeedWords, XorShift128, derive};

pub const ENEMY_OBJECT_DOMAIN: u32 = 0x4299_7d84;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnemyObjectError {
    InvalidEnemyId(u8),
    InvalidTide(u8),
}

/// Return the object-local RNG just before its vtable `+0x58` WAVE init.
///
/// ELF 0x37651a0..0x37652cc starts from the domain-derived base state,
/// advances it `enemy_id + 1` times, then expands the last output into four
/// object words. Only IDs 0–31 occur in the director's object loop.
pub fn initial_enemy_object_rng(
    seed: SeedWords,
    enemy_id: u8,
) -> Result<XorShift128, EnemyObjectError> {
    if enemy_id > 31 {
        return Err(EnemyObjectError::InvalidEnemyId(enemy_id));
    }
    let mut base = derive(seed, ENEMY_OBJECT_DOMAIN);
    let mut value = 0;
    for _ in 0..=enemy_id {
        value = base.next32();
    }
    let mut words = [0u32; 4];
    for (index, word) in words.iter_mut().enumerate() {
        value = 0x6c07_8965u32
            .wrapping_mul(value ^ (value >> 30))
            .wrapping_add(index as u32 + 1);
        *word = value;
    }
    Ok(XorShift128::from_words(words))
}

/// ID13 SakeArtillery's `+0x100` flag immediately after WAVE init.
///
/// Its vtable `+0x58` draws once from the object-local RNG and sets the flag
/// when the output's top bit is zero. Later flag mutations are not modeled.
pub fn artillery_initial_flag(seed: SeedWords) -> bool {
    let mut rng = initial_enemy_object_rng(seed, 13).expect("ID 13 is in 0..=31");
    rng.next32() & 0x8000_0000 == 0
}

/// ID13 candidate callback at WAVE start, before any later flag mutations.
///
/// The callback rejects high tide (2) or a *clear* initial object flag. This
/// assumes the live tide manager supplied a valid tide; the game's null
/// manager branch has a different default. This local RNG calculation does
/// not advance the director's shared RNG.
pub fn artillery_initial_callback_allows(
    seed: SeedWords,
    tide: u8,
) -> Result<bool, EnemyObjectError> {
    if tide > 2 {
        return Err(EnemyObjectError::InvalidTide(tide));
    }
    Ok(tide != 2 && artillery_initial_flag(seed))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtilleryFirstReservoir {
    /// ID13's candidate flag immediately after its WAVE-init draw.
    pub initial_flag: bool,
    /// Index in the caller's runtime-ordered direction records.
    pub selected_record_index: Option<usize>,
    pub matching_records: u32,
    /// Object-local RNG after the flag draw and every matching record draw.
    pub object_rng_after: XorShift128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtilleryReservoir {
    pub selected_record_index: Option<usize>,
    pub matching_records: u32,
    pub object_rng_after: XorShift128,
}

/// First ID13 spot-callback reservoir when `object+0xf8` is null.
///
/// The caller supplies the loaded direction records in their actual runtime
/// order, represented by each record's tide. This assumes no object-local
/// draws occur between WAVE init and this callback. The initial flag/tide
/// candidate gate may prevent the callback from being invoked at all in an
/// ordinary WAVE. A selected record is not a confirmed spot: actor validation,
/// use-state checks, and fallback still follow. This never advances the
/// director's shared species/spot RNG.
pub fn artillery_first_reservoir(
    seed: SeedWords,
    tide: u8,
    ordered_record_tides: &[u32],
) -> Result<ArtilleryFirstReservoir, EnemyObjectError> {
    if tide > 2 {
        return Err(EnemyObjectError::InvalidTide(tide));
    }
    let mut rng = initial_enemy_object_rng(seed, 13).expect("ID 13 is in 0..=31");
    let initial_flag = rng.next32() & 0x8000_0000 == 0;
    let scan = artillery_reservoir_from_state(rng, tide, ordered_record_tides)?;
    Ok(ArtilleryFirstReservoir {
        initial_flag,
        selected_record_index: scan.selected_record_index,
        matching_records: scan.matching_records,
        object_rng_after: scan.object_rng_after,
    })
}

/// Repeat the null-cached-point reservoir from a known object-local state.
///
/// The caller must establish that `object+0xf8` is null and provide every
/// direction record in the game's current runtime order. This models only
/// the tide-filtered selection; later actor/use checks can still fail.
pub fn artillery_reservoir_from_state(
    mut object_rng: XorShift128,
    tide: u8,
    ordered_record_tides: &[u32],
) -> Result<ArtilleryReservoir, EnemyObjectError> {
    if tide > 2 {
        return Err(EnemyObjectError::InvalidTide(tide));
    }
    let mut matching_records = 0u32;
    let mut selected_record_index = None;
    for (index, record_tide) in ordered_record_tides.iter().enumerate() {
        if *record_tide == u32::from(tide) {
            matching_records += 1;
            if artillery_reservoir_replace(object_rng.next32(), matching_records) {
                selected_record_index = Some(index);
            }
        }
    }
    Ok(ArtilleryReservoir {
        selected_record_index,
        matching_records,
        object_rng_after: object_rng,
    })
}

pub(crate) fn artillery_reservoir_replace(random: u32, matching_ordinal: u32) -> bool {
    let sample = f32::from_bits(0x3f80_0000 | (random >> 9)) - 1.0;
    sample < 1.0f32 / (matching_ordinal as f32)
}

#[cfg(test)]
#[path = "../test_support/unit/enemy_object.rs"]
mod tests;
