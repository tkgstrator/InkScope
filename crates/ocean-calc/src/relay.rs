//! Event 7 Relay's live candidate routing before indirect actor dispatch.
//!
//! The 11.3.0 routine at `0x3790670` depends on live group order, ring state,
//! mode bytes, and an object-local RNG. These inputs are snapshots supplied by
//! the caller. A selected source is not a confirmed actor registration.

use crate::rng::{XorShift128, expand_seed_word};

const DROP_RNG_DOMAIN: u32 = 0x4fc0_38a5;

/// Event 7 constructor state at object offsets `+0x1a8..+0x1b4`.
///
/// ELF `0x378dfc0..0x378e0b4` distinguishes a missing shared RNG object
/// from one whose four stored words are present but zero. This is the state
/// before any registered callback or drop-point trial advances it; callers
/// must account for those draws before supplying a state to [`step_drop`].
/// `Some` means the four words stored in the shared object, not a raw
/// scenario array that has yet to pass the scene's own normalization.
pub fn initial_drop_rng_state(shared_words: Option<[u32; 4]>) -> [u32; 4] {
    let effective = match shared_words {
        None => [1, 0, 0, 0],
        Some([0, 0, 0, 0]) => [1, 0x6c07_8967, 0, 0x4807_7044],
        Some(words) => words,
    };
    let mut source = XorShift128::from_words(effective);
    let expanded = source.next32().wrapping_add(DROP_RNG_DOMAIN);
    expand_seed_word(expanded).as_array()
}

/// Raw 48-byte SpawnBoxLocator transform. XYZ occupy the first three words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoxTransform(pub [u32; 12]);

impl BoxTransform {
    fn xyz(self) -> [f32; 3] {
        [
            f32::from_bits(self.0[0]),
            f32::from_bits(self.0[1]),
            f32::from_bits(self.0[2]),
        ]
    }
}

/// Raw 48-byte Golden Ikura drop point source, distinct from a box transform.
///
/// Words 0–1 hold a pointer, word 2 holds metadata, and words 3–11 hold nine
/// f32 bit patterns. They remain in source order here, before dispatch packing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropPointSource(pub [u32; 12]);

/// The selected outer record's inner count, read before the mode-byte branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropRecordSnapshot {
    pub inner_count: i32,
}

/// Live outer ring for one of the three object-local drop-point groups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OuterRingSnapshot {
    pub capacity: i32,
    pub start: i32,
    pub count: i32,
    pub records: Vec<DropRecordSnapshot>,
}

/// Inner source-ring fields re-read for each drop-point attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InnerRingSnapshot {
    pub capacity: i32,
    pub start: i32,
    pub sources: Vec<DropPointSource>,
}

/// Live inputs to the guards and branch at `0x3790670..0x3790814`.
#[derive(Clone, Debug, PartialEq)]
pub struct RelaySnapshot {
    /// Scene `+0x598`; unsigned values above 2 select box group 0.
    pub tide: u32,
    /// Ordered live SpawnBoxLocator arrays for the three tides.
    pub box_groups: [Vec<BoxTransform>; 3],
    /// Object `+0x7970`; unsigned values above 2 select drop group 0.
    pub drop_group_selector: u32,
    /// Object `+0x7974`, the logical index in the selected outer ring.
    pub outer_cursor: u32,
    /// Missing downstream groups may be omitted behind an earlier return.
    pub outer_groups: [Option<OuterRingSnapshot>; 3],
    /// Object `+0x19c9`; zero routes to drop-point attempts.
    pub mode: u8,
    /// Object `+0x19fc`; only examined for a nonzero mode.
    pub nonzero_gate: u8,
    /// Object `+0x19cc..+0x19d4`; needed only for multiple box locators.
    pub reference_xyz: Option<[f32; 3]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayError {
    MissingOuterRing,
    OuterCursorOutOfRange { cursor: u32, count: i32 },
    InvalidOuterRing(&'static str),
    MissingReference,
    InvalidInnerRing(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayRoute {
    NoAttempt {
        mode_after: u8,
    },
    BoxCandidate {
        mode_after: u8,
        box_index: usize,
        source: BoxTransform,
    },
    DropReady(DropReady),
}

/// Stack-local permutation and remaining count across one-at-a-time trials.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropReady {
    pub outer_group_index: usize,
    pub outer_physical_index: usize,
    pub remaining: i32,
    pub attempt_index: u64,
    pool: [u32; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropStep {
    pub selected_source_index: u32,
    pub inner_physical_index: usize,
    pub source: DropPointSource,
    /// Local RNG after this draw and before the indirect dispatch.
    pub rng_before_dispatch: [u32; 4],
    /// The next stack-local state; supply fresh RNG and ring snapshots to step.
    pub next: DropReady,
    /// The routine uses slot 0 for trials 0 and >=2, slot 1 for trial 1.
    pub target_slot: u8,
}

/// Route a live Relay snapshot up to, but not through, indirect dispatch.
///
/// Reached invalid snapshots are errors. A zero box count, inner count below
/// one, or closed nonzero-mode gate are game early returns with no attempt.
pub fn route_relay(input: &RelaySnapshot) -> Result<RelayRoute, RelayError> {
    let boxes = &input.box_groups[group_index(input.tide)];
    if boxes.is_empty() {
        return Ok(RelayRoute::NoAttempt {
            mode_after: input.mode,
        });
    }

    let outer_group_index = group_index(input.drop_group_selector);
    let outer = input.outer_groups[outer_group_index]
        .as_ref()
        .ok_or(RelayError::MissingOuterRing)?;

    // The ELF branches to a null-entry path when unsigned count <= cursor.
    if (outer.count as u32) <= input.outer_cursor {
        return Err(RelayError::OuterCursorOutOfRange {
            cursor: input.outer_cursor,
            count: outer.count,
        });
    }
    validate_outer(outer)?;
    let physical = wrap_once(outer.start, input.outer_cursor, outer.capacity).ok_or(
        RelayError::InvalidOuterRing("physical index outside storage"),
    )?;
    let record = outer
        .records
        .get(physical)
        .ok_or(RelayError::InvalidOuterRing("physical record absent"))?;
    if record.inner_count < 1 {
        return Ok(RelayRoute::NoAttempt {
            mode_after: input.mode,
        });
    }

    if input.mode != 0 {
        if input.nonzero_gate == 0 {
            return Ok(RelayRoute::NoAttempt {
                mode_after: input.mode,
            });
        }
        let index = if boxes.len() == 1 {
            0
        } else {
            let reference = input.reference_xyz.ok_or(RelayError::MissingReference)?;
            nearest_box_index(boxes, reference)
        };
        return Ok(RelayRoute::BoxCandidate {
            mode_after: 0,
            box_index: index,
            source: boxes[index],
        });
    }

    Ok(RelayRoute::DropReady(DropReady {
        outer_group_index,
        outer_physical_index: physical,
        remaining: record.inner_count.clamp(0, 32),
        attempt_index: 0,
        pool: std::array::from_fn(|index| index as u32),
    }))
}

/// Advance exactly one drop-point trial with current object-local snapshots.
///
/// The caller may refresh both `rng_words` and `inner` after each indirect
/// dispatch. The caller must stop after the current call's `w1` trial request
/// (one or two at the inspected call sites); an extra pure step is not a live
/// attempt. Invalid snapshots are rejected before drawing; this diagnostic
/// order does not model the game's behavior on corrupted ring metadata.
/// This function does not dispatch or establish actor creation.
pub fn step_drop(
    mut state: DropReady,
    rng_words: [u32; 4],
    inner: &InnerRingSnapshot,
) -> Result<DropStep, RelayError> {
    validate_inner(inner)?;
    let mut rng = XorShift128::from_words(rng_words);
    let draw = rng.next32();
    let raw = ((u64::from(draw) * u64::from(state.remaining as u32)) >> 32) as u32;
    let selected_slot = if raw < 32 { raw as usize } else { 0 };
    let last_raw = state.remaining.wrapping_sub(1) as u32;
    let last_slot = if last_raw < 32 { last_raw as usize } else { 0 };
    let selected_source_index = state.pool[selected_slot];
    state.pool.swap(selected_slot, last_slot);
    state.remaining = state.remaining.wrapping_sub(1);

    let physical = wrap_once(inner.start, selected_source_index, inner.capacity).ok_or(
        RelayError::InvalidInnerRing("physical index outside storage"),
    )?;
    let source = *inner
        .sources
        .get(physical)
        .ok_or(RelayError::InvalidInnerRing("physical source absent"))?;
    let target_slot = u8::from(state.attempt_index == 1);
    state.attempt_index = state.attempt_index.wrapping_add(1);

    Ok(DropStep {
        selected_source_index,
        inner_physical_index: physical,
        source,
        rng_before_dispatch: rng.as_array(),
        next: state,
        target_slot,
    })
}

fn group_index(index: u32) -> usize {
    if index < 3 { index as usize } else { 0 }
}

fn validate_outer(outer: &OuterRingSnapshot) -> Result<(), RelayError> {
    if outer.capacity < 1
        || outer.start < 0
        || outer.start >= outer.capacity
        || outer.count < 0
        || outer.count > outer.capacity
        || outer.records.len() < outer.capacity as usize
    {
        return Err(RelayError::InvalidOuterRing(
            "capacity, start, count, or storage inconsistent",
        ));
    }
    Ok(())
}

fn validate_inner(inner: &InnerRingSnapshot) -> Result<(), RelayError> {
    if inner.capacity < 1
        || inner.start < 0
        || inner.start >= inner.capacity
        || inner.sources.len() < inner.capacity as usize
    {
        return Err(RelayError::InvalidInnerRing(
            "capacity, start, or storage inconsistent",
        ));
    }
    Ok(())
}

fn wrap_once(start: i32, logical: u32, capacity: i32) -> Option<usize> {
    let sum = start.wrapping_add(logical as i32);
    let physical = if sum < capacity {
        sum
    } else {
        sum.wrapping_sub(capacity)
    };
    (0..capacity)
        .contains(&physical)
        .then_some(physical as usize)
}

fn nearest_box_index(boxes: &[BoxTransform], reference: [f32; 3]) -> usize {
    let mut best_index = 0;
    let mut best_distance = squared_distance(boxes[0].xyz(), reference);
    for (index, candidate) in boxes.iter().enumerate().skip(1) {
        let distance = squared_distance(candidate.xyz(), reference);
        if distance < best_distance {
            best_distance = distance;
            best_index = index;
        }
    }
    best_index
}

fn squared_distance(point: [f32; 3], reference: [f32; 3]) -> f32 {
    let dx = point[0] - reference[0];
    let dy = point[1] - reference[1];
    let dz = point[2] - reference[2];
    dz.mul_add(dz, dx.mul_add(dx, dy * dy))
}
