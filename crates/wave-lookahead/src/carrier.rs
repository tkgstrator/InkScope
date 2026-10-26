//! Conditional Event 4 carrier box-location selection in Splatoon 3 11.3.0.
//!
//! The caller must establish that the Carrier's spawn gate reached this
//! selection attempt and supply the current ordered candidates and occupied
//! slots. A fresh-tick attempt reseeds independently; its returned RNG state
//! must not be carried to the next attempt without observing a retained-state
//! branch. Candidate selection precedes the spawn request and does not prove
//! that a SakeCopter was created. Stage Banc candidates are conditional on
//! ordinary actor registration and unchanged runtime transforms.

use crate::{
    rng::{XorShift128, expand_seed_word},
    stage_data::StageSpawns,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CarrierRandomInput {
    /// Carrier flag `+0x1e8 == 0`: reseed this attempt from the low 32 bits
    /// of `nn::os::GetSystemTick`. Zero is not a scene-seed normalization case.
    FreshTickLow32(u32),
    /// Carrier flag `+0x1e8 != 0`: use an independently observed live state.
    RetainedState(XorShift128),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CarrierChoice {
    NoCandidates,
    AllOccupied,
    InvalidSelectedX {
        index: usize,
    },
    /// Host-selected request position, before creation success is known.
    Selected {
        index: usize,
        position: [f32; 3],
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarrierSelection {
    pub draw: u32,
    pub start_index: Option<usize>,
    pub choice: CarrierChoice,
    /// State after this attempt's one draw. A fresh-tick next attempt reseeds.
    pub random_state_after: XorShift128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CarrierSelectionError {
    SlotCountMismatch { candidates: usize, slots: usize },
    TooManyCandidates(usize),
}

/// Select one location after the Carrier's earlier population and timing gates.
///
/// The candidate array is the runtime tide list in its actual registration
/// order. Negative slot IDs are available. The game consumes one RNG value
/// even when this array is empty, then scans circularly from the multiply-high
/// start index without further draws. The selected X NaN check follows the
/// scan. Creation and slot-ID updates are separate later operations.
pub fn select_carrier_candidate(
    candidates: &[[f32; 3]],
    slot_ids: &[i32],
    random: CarrierRandomInput,
) -> Result<CarrierSelection, CarrierSelectionError> {
    if candidates.len() != slot_ids.len() {
        return Err(CarrierSelectionError::SlotCountMismatch {
            candidates: candidates.len(),
            slots: slot_ids.len(),
        });
    }
    let count = u32::try_from(candidates.len())
        .ok()
        .filter(|&count| count <= i32::MAX as u32)
        .ok_or(CarrierSelectionError::TooManyCandidates(candidates.len()))?;
    let mut rng = match random {
        CarrierRandomInput::FreshTickLow32(tick) => expand_seed_word(tick),
        CarrierRandomInput::RetainedState(state) => state,
    };
    let draw = rng.next32();
    let start_index = if count == 0 {
        None
    } else {
        Some(((u64::from(count) * u64::from(draw)) >> 32) as usize)
    };
    let choice = if let Some(start) = start_index {
        let selected = (0..candidates.len())
            .map(|offset| (start + offset) % candidates.len())
            .find(|&index| slot_ids[index] < 0);
        match selected {
            Some(index) if candidates[index][0].is_nan() => {
                CarrierChoice::InvalidSelectedX { index }
            },
            Some(index) => CarrierChoice::Selected {
                index,
                position: candidates[index],
            },
            None => CarrierChoice::AllOccupied,
        }
    } else {
        CarrierChoice::NoCandidates
    };
    Ok(CarrierSelection {
        draw,
        start_index,
        choice,
        random_state_after: rng,
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StageCarrierSelection {
    /// The Carrier falls back to the Low list for a missing or invalid tide.
    pub effective_tide: u8,
    pub selection: CarrierSelection,
}

/// Use the owned 11.3.0 Banc snapshot as a conditional Event 4 candidate list.
///
/// The game reads the registered actor transform and adds f32 `(0, 3, 0)`.
/// This static table is equivalent only if the expected actors registered in
/// Banc order and their runtime transforms remain equal to the Banc values.
pub fn predict_stage_carrier_candidate(
    stage: &StageSpawns,
    runtime_tide: Option<u8>,
    slot_ids: &[i32],
    random: CarrierRandomInput,
) -> Result<StageCarrierSelection, CarrierSelectionError> {
    let effective_tide = runtime_tide.filter(|&tide| tide <= 2).unwrap_or(0);
    let sources = stage
        .carrier_box_sources(effective_tide)
        .expect("validated effective tide");
    let candidates: Vec<[f32; 3]> = sources
        .iter()
        .map(|actor| {
            let [x, y, z] = actor.translate;
            [x + 0.0_f32, y + 3.0_f32, z + 0.0_f32]
        })
        .collect();
    let selection = select_carrier_candidate(&candidates, slot_ids, random)?;
    Ok(StageCarrierSelection {
        effective_tide,
        selection,
    })
}
