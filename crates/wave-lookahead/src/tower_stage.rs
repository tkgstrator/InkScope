//! ID8 Tower's first arrival-candidate draw under static WAVE-start conditions.
//!
//! The returned index is into filtered runtime registration order, which has
//! not been shown to equal Banc order. The later Tower callback can still
//! fail after this draw and fall back to the director's generic spot path.

use crate::{
    SeedWords, enemy_object::initial_enemy_object_rng, rng::XorShift128, stage_data::StageSpawns,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TowerFirstError {
    InvalidTide(u8),
    InvalidDirection(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TowerFirstArrival {
    pub candidate_count: u32,
    /// Index within the filtered runtime candidate list. No actor ID or
    /// final coordinate can be inferred from this index alone.
    pub runtime_candidate_index: Option<u32>,
    pub object_rng_after: XorShift128,
}

/// Calculate the first ID8 arrival draw for the requested tide/direction.
///
/// Assumes the loaded Tower arrival registry contains exactly the actors in
/// the stage snapshot, all are live and unused, the WAVE reset cleared usage
/// counters, and no object-local draw or state update intervened. The ID8
/// WAVE-init vtable adds no draw. One local xorshift draw is consumed for a
/// nonempty candidate group, followed by `high32(q * count)` at ELF
/// `0x377262c..0x3772674`. Callback success also depends on active records or
/// a `CoopSpawnPointZako` fallback, neither determined here. This local draw
/// does not advance the shared direction/species/spot RNG.
pub fn predict_first_tower_arrival(
    seed: SeedWords,
    stage: &StageSpawns,
    tide: u8,
    direction: u8,
) -> Result<TowerFirstArrival, TowerFirstError> {
    if tide > 2 {
        return Err(TowerFirstError::InvalidTide(tide));
    }
    let Some(arrivals) = stage.tower_arrival_group(tide, direction) else {
        return Err(TowerFirstError::InvalidDirection(direction));
    };
    let mut rng = initial_enemy_object_rng(seed, 8).expect("ID8 is valid");
    let candidate_count = u32::try_from(arrivals.len()).expect("owned Tower groups fit u32");
    let runtime_candidate_index = if candidate_count == 0 {
        None
    } else {
        let value = rng.next32();
        Some(((u64::from(value) * u64::from(candidate_count)) >> 32) as u32)
    };
    Ok(TowerFirstArrival {
        candidate_count,
        runtime_candidate_index,
        object_rng_after: rng,
    })
}
