//! Conditional default-subtype (verified JobType 0) five-WAVE weapon grid.
//!
//! The caller establishes a normal completed preprocessing branch, the
//! effective scene ring/pools, known asset callback outcomes, and a stable
//! WAVE3 override flag across grid construction and postprocessing. The slot
//! sequences are not assignments to individual players.

use crate::{
    SeedWords,
    rng::XorShift128,
    weapon_preprocess::{
        PreprocessError, PreprocessRng, PreprocessShape, weapon_rng_after_preprocess,
    },
    weapons::{
        WeaponGridBuildError, WeaponGridBuildInput, WeaponGridSources, build_weapon_grid,
        postprocess_weapon_grid, weapon_slot_sequences,
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponGridPreview {
    pub pregrid: [[i32; 4]; 5],
    pub rng_after_build: XorShift128,
    pub grid: [[i32; 4]; 5],
    pub sources: WeaponGridSources,
    /// Values retain their packed high bits; they do not identify players.
    pub slot_sequences: [Vec<i32>; 4],
    pub final_rng: XorShift128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeaponPreviewOutcome {
    pub preprocess: PreprocessRng,
    /// `None` means the supplied runtime readiness gate skipped the grid.
    pub grid: Option<WeaponGridPreview>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeaponPreviewError {
    Preprocess(PreprocessError),
    Build {
        error: WeaponGridBuildError,
        preprocess: PreprocessRng,
        rng_at_error: XorShift128,
    },
}

/// Compose preprocessing, twenty packed nodes, seven grid draws, and slot lists.
///
/// The preprocessing shape is valid only for the completed normal branch with
/// no unmodeled callback RNG mutation. `input` must be the effective scene
/// state, not an unverified scenario setting ring. The same observed
/// `wave_three_override` condition must hold at both game-side checks.
pub fn predict_weapon_grid(
    seed: SeedWords,
    shape: PreprocessShape,
    input: WeaponGridBuildInput<'_>,
    asset_usable: impl FnMut(i32) -> bool,
) -> Result<WeaponPreviewOutcome, WeaponPreviewError> {
    let preprocess =
        weapon_rng_after_preprocess(seed, shape).map_err(WeaponPreviewError::Preprocess)?;
    let mut rng = preprocess.state;
    let pregrid = build_weapon_grid(&mut rng, input, asset_usable).map_err(|error| {
        WeaponPreviewError::Build {
            error,
            preprocess,
            rng_at_error: rng,
        }
    })?;
    let Some(pregrid) = pregrid else {
        return Ok(WeaponPreviewOutcome {
            preprocess,
            grid: None,
        });
    };
    let rng_after_build = rng;
    let mut grid = pregrid;
    let sources = postprocess_weapon_grid(&mut rng, &mut grid, input.wave_three_override);
    let slot_sequences = weapon_slot_sequences(&grid);
    Ok(WeaponPreviewOutcome {
        preprocess,
        grid: Some(WeaponGridPreview {
            pregrid,
            rng_after_build,
            grid,
            sources,
            slot_sequences,
            final_rng: rng,
        }),
    })
}
