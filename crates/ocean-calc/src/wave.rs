//! Weighted selection core for a wave's event and tide.
//!
//! The caller supplies the candidates after applying game-mode, stage, and
//! previous-wave restrictions. Event selection must finish before the caller
//! builds the tide candidates; later waves reuse the same `WaveRng`.

use crate::rng::{SeedWords, XorShift128, derive};

pub const WAVE_DOMAIN: u32 = 0xf7d5_febd;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Candidate {
    pub weight: u32,
    pub eligible: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaveRng {
    state: XorShift128,
}

impl WaveRng {
    pub fn from_seed(seed: SeedWords) -> Self {
        Self {
            state: derive(seed, WAVE_DOMAIN),
        }
    }

    pub fn as_array(self) -> [u32; 4] {
        self.state.as_array()
    }

    fn next_tide32(&mut self) -> u32 {
        // The positive-weight path at 0x37f6400 branches to 0x37f67b4;
        // 0x37f67b8 writes the computed value back to the fourth word.
        self.state.next32()
    }
}

pub fn draw_event(rng: &mut WaveRng, candidates: &[Candidate; 9]) -> u8 {
    let mut choice = 0;
    let mut total_weight = 0u32;
    for (id, candidate) in candidates.iter().enumerate() {
        if !candidate.eligible || candidate.weight == 0 {
            continue;
        }
        total_weight = total_weight.wrapping_add(candidate.weight);
        let value = rng.state.next32();
        if candidate.weight > ((u64::from(total_weight) * u64::from(value)) >> 32) as u32 {
            choice = id as u8;
        }
    }
    choice
}

pub fn draw_tide(rng: &mut WaveRng, candidates: &[Candidate; 3]) -> u8 {
    let mut choice = 1;
    let mut total_weight = 0u32;
    for (id, candidate) in candidates.iter().enumerate() {
        if !candidate.eligible || candidate.weight == 0 {
            continue;
        }
        total_weight = total_weight.wrapping_add(candidate.weight);
        let value = rng.next_tide32();
        if candidate.weight > ((u64::from(total_weight) * u64::from(value)) >> 32) as u32 {
            choice = id as u8;
        }
    }
    choice
}
