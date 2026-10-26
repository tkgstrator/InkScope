//! The static stage part of ID13's first Artillery point selection.
//!
//! A selected matching ordinal is not a callback success, placed enemy, or
//! final coordinate. Runtime record order is unverified against Banc order.

use crate::{
    SeedWords,
    enemy_object::{artillery_reservoir_replace, initial_enemy_object_rng},
    rng::XorShift128,
    stage_data::{SpawnActor, StageSpawns},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtilleryStageError {
    InvalidTide(u8),
    InvalidDirection(u8),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArtilleryStageRoute {
    /// The initial flag is clear or the tide is High; the candidate callback
    /// declines ID13, so the spot reservoir is not invoked on this path.
    CandidateVeto,
    /// No source record belongs to the requested tide/direction.
    NoTideRecords,
    /// The reservoir's zero-based ordinal among matching tide records.
    /// `source_actor` is available only for a singleton Banc group. The game
    /// still performs actor/use checks before accepting a point.
    ReservoirSelected {
        matching_ordinal: usize,
        source_actor: Option<&'static SpawnActor>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArtilleryStageResult {
    pub initial_flag: bool,
    pub route: ArtilleryStageRoute,
    /// Count for the invoked spot scan; zero when the candidate is vetoed.
    pub matching_records: usize,
    pub object_rng_after: XorShift128,
}

/// Project ID13's first point reservoir onto the stage's static Banc data.
///
/// Assumes the loaded Artillery records match this stage snapshot, WAVE init
/// cleared `object+0xf8`, no intervening object-local draws or flag mutations,
/// and the ordinary scheduler selects ID13 when the candidate gate allows it.
/// When the gate allows it, the result models the spot callback's reservoir
/// scan, not its later actor validation or final position. Since only the
/// number of matching tide records controls that scan, its selected *matching
/// ordinal* is known even if Low/Mid records are interleaved in unknown order.
pub fn predict_first_artillery_stage(
    seed: SeedWords,
    stage: &StageSpawns,
    tide: u8,
    direction: u8,
) -> Result<ArtilleryStageResult, ArtilleryStageError> {
    if tide > 2 {
        return Err(ArtilleryStageError::InvalidTide(tide));
    }
    let Some(points) = stage.artillery_group(tide, direction) else {
        return Err(ArtilleryStageError::InvalidDirection(direction));
    };
    let mut rng = initial_enemy_object_rng(seed, 13).expect("ID13 is valid");
    let initial_flag = rng.next32() & 0x8000_0000 == 0;
    if !initial_flag || tide == 2 {
        return Ok(ArtilleryStageResult {
            initial_flag,
            route: ArtilleryStageRoute::CandidateVeto,
            matching_records: 0,
            object_rng_after: rng,
        });
    }

    let mut selected = None;
    for ordinal in 0..points.len() {
        if artillery_reservoir_replace(rng.next32(), (ordinal + 1) as u32) {
            selected = Some(ordinal);
        }
    }
    let route = match selected {
        Some(matching_ordinal) => ArtilleryStageRoute::ReservoirSelected {
            matching_ordinal,
            source_actor: (points.len() == 1).then_some(&points[0]),
        },
        None => ArtilleryStageRoute::NoTideRecords,
    };
    Ok(ArtilleryStageResult {
        initial_flag,
        route,
        matching_records: points.len(),
        object_rng_after: rng,
    })
}
