//! Generic actor index draw for an ordered runtime spot-candidate list.
//!
//! The caller establishes that its Rare or common Zako path reached this draw
//! after any species callback and supplies the ordered, capacity-limited
//! candidate count. The result is not a final spawn XYZ.

use crate::rng::XorShift128;

/// Return the selected actor-array index, consuming one draw for count >= 1.
pub fn choose_generic_spot_index(rng: &mut XorShift128, candidate_count: u32) -> Option<u32> {
    if candidate_count == 0 {
        return None;
    }
    let value = rng.next32();
    Some(((u64::from(value) * u64::from(candidate_count)) >> 32) as u32)
}
