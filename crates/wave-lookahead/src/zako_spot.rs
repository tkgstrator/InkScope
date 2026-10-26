//! Conditional common-Zako spawn-point selection for applied Events 0/1, mode 0.
//!
//! This starts after the stock species selector. The caller establishes its
//! success and the ID 0/1/2 default callback route, then supplies the live
//! direction, tide, ordered runtime candidates, capacity, and director RNG.
//! Species identity is neither calculated nor returned. The selected transform
//! is the spawn-point request passed to creation, not a final enemy position.

use crate::rng::XorShift128;
use crate::spot::choose_generic_spot_index;
use crate::stage_data::SpawnActor;
use crate::zako_timer::ZakoTimerStep;

/// Apply only the common-Zako selector's directly observed RNG draws.
///
/// The ELF advances `director+0x38c` once for each visited candidate that
/// passes its active-cap check and has nonzero effective weight. The caller
/// supplies that visit count, validated against its runtime iterator snapshot;
/// it need not identify the selected species. This does not model possible
/// RNG side effects of the selector's registry, cap, or weight callbacks.
/// Its result is a spot-selector input only when those calls and earlier code
/// do not consume additional `+0x38c` draws.
pub fn advance_zako_selector_inline_draws(
    rng_before: [u32; 4],
    eligible_visit_count: u32,
) -> [u32; 4] {
    let mut rng = XorShift128::from_words(rng_before);
    for _ in 0..eligible_visit_count {
        rng.next32();
    }
    rng.as_array()
}

/// The first six floats of a runtime candidate record at the spot-call boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RuntimeZakoSpot {
    pub origin: [f32; 3],
    pub forward: [f32; 3],
}

/// Reconstruct the initial runtime spot record from a stock main Banc actor.
///
/// The normal main-Banc reader uses an identity load matrix. Missing `Rotate`
/// is zero-initialized by the actor parser, and its angles are radians. This
/// models the core actor's position and forward column when the spot builder
/// reads it, provided no intervening code has changed that transform. Host
/// trigonometry is not established as bit-identical to the game's `sinf/cosf`.
/// Runtime filtering, auxiliary candidates, and final enemy XYZ are separate.
pub fn stock_main_banc_spot(actor: &SpawnActor) -> RuntimeZakoSpot {
    let [rx, ry, rz] = actor.rotate.unwrap_or([0.0; 3]);
    let (sx, cx) = (rx.sin(), rx.cos());
    let (sy, cy) = (ry.sin(), ry.cos());
    let (sz, cz) = (rz.sin(), rz.cos());
    RuntimeZakoSpot {
        origin: actor.translate,
        // Third column of Rz * Ry * Rx, using the ELF's fused operations.
        forward: [
            (cx * cz).mul_add(sy, sx * sz),
            (cx * sz).mul_add(sy, -(sx * cz)),
            cx * cy,
        ],
    }
}

/// Caller-observed inputs immediately before the ordinary spot selector.
#[derive(Clone, Copy, Debug)]
pub struct ModeZeroZakoSpotInput<'a> {
    pub timer_step: ZakoTimerStep,
    /// Whether the stock `0x3764b90` species stage returned a valid ID.
    pub species_selection_succeeded: bool,
    /// The caller verified the stock ID 0/1/2 false spot-callback route.
    pub default_callback_route_confirmed: bool,
    pub applied_event: u8,
    /// Effective director `+0x378`, not a generated record field by itself.
    pub applied_mode: i32,
    /// Director `+0x3bc` when the spot call begins, after scheduler updates.
    pub direction: i32,
    /// Scene `+0x598` when the spot call begins.
    pub tide: i32,
    /// Director `+0x38c` after species selection, before the spot draw.
    pub rng_after_species: [u32; 4],
    /// Nine runtime groups. `None` means the key is absent; `Some(&[])` exists.
    pub runtime_groups: [Option<&'a [Option<RuntimeZakoSpot>]>; 9],
    /// In-order entries of the runtime tree at the candidate object `+0x30`.
    pub tree_30: &'a [Option<RuntimeZakoSpot>],
    /// In-order entries of the runtime tree at the candidate object `+0x50`.
    pub tree_50: &'a [Option<RuntimeZakoSpot>],
    /// Effective signed number of scratch pointer slots at director `+0x89c`.
    pub scratch_capacity: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZakoSpotSource {
    Group { source_index: usize },
    Tree30 { source_index: usize },
    Tree50 { source_index: usize },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ZakoSpotOutcome {
    TimerBlocked,
    SpeciesSelectionFailed,
    MissingGroup,
    NoCandidates,
    SelectedNull {
        combined_index: usize,
        source: ZakoSpotSource,
    },
    SelectedPoint {
        combined_index: usize,
        source: ZakoSpotSource,
        spot: RuntimeZakoSpot,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZakoSpotTrace {
    /// `tide + 3 * direction`, or `None` if the timer/selector stopped earlier.
    pub group_key: Option<usize>,
    pub candidate_count: usize,
    /// RNG after the spot selector only; later creation can consume more.
    pub rng_after_spot: [u32; 4],
    pub outcome: ZakoSpotOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZakoSpotError {
    UnsupportedEvent(u8),
    UnconfirmedDefaultCallbackRoute,
    UnsupportedMode(i32),
    InvalidDirection(i32),
    InvalidTide(i32),
}

impl ZakoSpotTrace {
    fn without_draw(rng: [u32; 4], group_key: Option<usize>, outcome: ZakoSpotOutcome) -> Self {
        Self {
            group_key,
            candidate_count: 0,
            rng_after_spot: rng,
            outcome,
        }
    }
}

/// Reproduce one ordinary common-Zako spot choice from a complete live snapshot.
///
/// An absent group key exits before either tree. An existing empty group can
/// still receive tree candidates. Null scratch slots count as candidates and
/// consume the draw when selected, but the game's spot call then fails.
pub fn predict_mode_zero_zako_spot(
    input: ModeZeroZakoSpotInput<'_>,
) -> Result<ZakoSpotTrace, ZakoSpotError> {
    let rng_words = input.rng_after_species;
    if !input.timer_step.spawn_path_entered {
        return Ok(ZakoSpotTrace::without_draw(
            rng_words,
            None,
            ZakoSpotOutcome::TimerBlocked,
        ));
    }
    if !input.species_selection_succeeded {
        return Ok(ZakoSpotTrace::without_draw(
            rng_words,
            None,
            ZakoSpotOutcome::SpeciesSelectionFailed,
        ));
    }
    if input.applied_event > 1 {
        return Err(ZakoSpotError::UnsupportedEvent(input.applied_event));
    }
    if !input.default_callback_route_confirmed {
        return Err(ZakoSpotError::UnconfirmedDefaultCallbackRoute);
    }
    if input.applied_mode != 0 {
        return Err(ZakoSpotError::UnsupportedMode(input.applied_mode));
    }
    if !(0..=2).contains(&input.direction) {
        return Err(ZakoSpotError::InvalidDirection(input.direction));
    }
    if !(0..=2).contains(&input.tide) {
        return Err(ZakoSpotError::InvalidTide(input.tide));
    }

    let group_key = (input.tide + 3 * input.direction) as usize;
    let Some(group) = input.runtime_groups[group_key] else {
        return Ok(ZakoSpotTrace::without_draw(
            rng_words,
            Some(group_key),
            ZakoSpotOutcome::MissingGroup,
        ));
    };

    let capacity = input.scratch_capacity.max(0) as usize;
    let mut candidates =
        Vec::with_capacity(capacity.min(group.len() + input.tree_30.len() + input.tree_50.len()));
    let sources = [(group, 0u8), (input.tree_30, 1u8), (input.tree_50, 2u8)];
    for (records, kind) in sources {
        for (source_index, spot) in records.iter().copied().enumerate() {
            if candidates.len() >= capacity {
                break;
            }
            let source = match kind {
                0 => ZakoSpotSource::Group { source_index },
                1 => ZakoSpotSource::Tree30 { source_index },
                _ => ZakoSpotSource::Tree50 { source_index },
            };
            candidates.push((source, spot));
        }
    }

    if candidates.is_empty() {
        return Ok(ZakoSpotTrace::without_draw(
            rng_words,
            Some(group_key),
            ZakoSpotOutcome::NoCandidates,
        ));
    }
    let mut rng = XorShift128::from_words(rng_words);
    let combined_index = choose_generic_spot_index(&mut rng, candidates.len() as u32)
        .expect("nonempty candidate array consumes one spot draw")
        as usize;
    let (source, selected) = candidates[combined_index];
    let outcome = match selected {
        Some(spot) => ZakoSpotOutcome::SelectedPoint {
            combined_index,
            source,
            spot,
        },
        None => ZakoSpotOutcome::SelectedNull {
            combined_index,
            source,
        },
    };
    Ok(ZakoSpotTrace {
        group_key: Some(group_key),
        candidate_count: candidates.len(),
        rng_after_spot: rng.as_array(),
        outcome,
    })
}
