//! RNG core for a processed `Rare` item's boss species selection.
//!
//! It shares the enemy RNG with direction and spot selection. The caller
//! supplies the state immediately before this draw and all runtime filters.

use crate::rng::XorShift128;

pub const UNSPECIFIED_BOSS_ID: u8 = 0x20;
pub const GOLDIE_BOSS_ID: u8 = 15;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeriodicGoldieInput {
    /// Applied event ID read through the current-event pointer; `None` means
    /// that pointer is null and the periodic branches are skipped.
    pub event: Option<u8>,
    /// Nonzero global byte at `0x60f65c4`. It halves Fog's first cycle and
    /// enables the second cycle only when the event is zero.
    pub global_cycle_flag: bool,
    /// Live director `+0x4d4` at this item's periodic predicate, before its
    /// possible early increment. That increment affects later items, not this
    /// periodic decision. This is not the queue index.
    pub accepted_rare_counter: i32,
    /// Resolved event-5 config `+0x44`; required only for event 5.
    pub fog_cycle: Option<i32>,
    /// Resolved config `+0xf0`; required only for event 0 with the global flag.
    pub event_none_cycle: Option<i32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeriodicGoldieSource {
    FogCycle,
    EventNoneGlobalCycle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeriodicGoldieError {
    InvalidEvent(u8),
    InvalidCandidateId(u8),
    MissingFogCycle,
    MissingEventNoneCycle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RareSpeciesSelection {
    /// ID 15 bypasses the normal ID 4–14 species scan and its RNG draws.
    ForcedGoldie(PeriodicGoldieSource),
    /// The normal scan may return unspecified ID `0x20`.
    Normal(u8),
}

impl RareSpeciesSelection {
    pub fn species_id(self) -> u8 {
        match self {
            Self::ForcedGoldie(_) => GOLDIE_BOSS_ID,
            Self::Normal(id) => id,
        }
    }
}

/// Decide whether the current Rare item forces ID 15 before the normal scan.
///
/// 11.3.0 ELF `0x375e538..0x375e808`: event 5 uses config `+0x44`, while
/// event 0 uses config `+0xf0` only when the global byte is nonzero. Event 6
/// does not reach either force predicate in this block. The Fog half-cycle
/// follows the game's f32 conversion before rounding up. A zero cycle disables
/// its predicate; nonzero cycles use signed remainder. Unknown required config
/// values are errors rather than assumed zeroes.
pub fn periodic_goldie_source(
    input: PeriodicGoldieInput,
) -> Result<Option<PeriodicGoldieSource>, PeriodicGoldieError> {
    let Some(event) = input.event else {
        return Ok(None);
    };
    if event > 8 {
        return Err(PeriodicGoldieError::InvalidEvent(event));
    }
    match event {
        5 => {
            let cycle = input
                .fog_cycle
                .ok_or(PeriodicGoldieError::MissingFogCycle)?;
            let effective_cycle = if input.global_cycle_flag {
                ((cycle as f32) * 0.5).ceil() as i32
            } else {
                cycle
            };
            Ok(period_matches(input.accepted_rare_counter, effective_cycle)
                .then_some(PeriodicGoldieSource::FogCycle))
        },
        0 if input.global_cycle_flag => {
            let cycle = input
                .event_none_cycle
                .ok_or(PeriodicGoldieError::MissingEventNoneCycle)?;
            Ok(period_matches(input.accepted_rare_counter, cycle)
                .then_some(PeriodicGoldieSource::EventNoneGlobalCycle))
        },
        _ => Ok(None),
    }
}

/// Select this item's Rare species, preserving the shared RNG on forced ID 15.
///
/// Callers must supply fresh normal-scan candidate states for nonforced items.
/// The caller must establish that the event actually reaches this Rare item
/// processor: runtime mode class 2/3 can bypass the ordered queue entirely.
/// This decides the species only; spot routing and actual creation are separate.
pub fn choose_event_rare_species(
    rng: &mut XorShift128,
    input: PeriodicGoldieInput,
    states: &[RareCandidateState; 32],
) -> Result<RareSpeciesSelection, PeriodicGoldieError> {
    if let Some(source) = periodic_goldie_source(input)? {
        return Ok(RareSpeciesSelection::ForcedGoldie(source));
    }
    if let Some(id) = states.iter().enumerate().find_map(|(id, state)| {
        (!(4..=14).contains(&id) && *state != RareCandidateState::Skip).then_some(id as u8)
    }) {
        return Err(PeriodicGoldieError::InvalidCandidateId(id));
    }
    Ok(RareSpeciesSelection::Normal(choose_rare_boss(rng, states)))
}

fn period_matches(counter: i32, cycle: i32) -> bool {
    cycle != 0 && i64::from(counter) % i64::from(cycle) == i64::from(cycle.wrapping_sub(1))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RareCandidateState {
    /// Not a Rare candidate, filtered by mode/global state, or callback veto.
    Skip,
    /// The count/limit branch consumes one draw but adds no candidate.
    AtLimit,
    /// Consumes one draw and joins the uniform reservoir selection.
    Eligible,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RarePopulation {
    /// Director `+0x454 + 4 * ID`, interpreted as signed by the count gate.
    pub active: i32,
    /// Director `+0x700 + 4 * ID`, interpreted as signed by the count gate.
    pub reserved: i32,
}

/// Run the ID 0–31 scan using the game's different random draw paths.
pub fn choose_rare_boss(rng: &mut XorShift128, states: &[RareCandidateState; 32]) -> u8 {
    let mut selected = UNSPECIFIED_BOSS_ID;
    let mut eligible_count = 0u32;
    for (id, state) in states.iter().enumerate() {
        match state {
            RareCandidateState::Skip => {},
            RareCandidateState::AtLimit => {
                rng.next32();
            },
            RareCandidateState::Eligible => {
                eligible_count += 1;
                let value = rng.next32();
                if ((u64::from(value) * u64::from(eligible_count)) >> 32) == 0 {
                    selected = id as u8;
                }
            },
        }
    }
    selected
}

/// Build the ID 0–31 dispositions for the game's `Rare` scan.
///
/// This matches the 11.3.0 Rare IDs 4–14, whose category makes the game's
/// limit function return 3 before its per-species ActorMax fallback. `event`
/// is `None` if the current-event object is null. Event 7 admits only IDs 8
/// and 11. `callback_allows` is the AND of all matching runtime callback
/// results for each ID; IDs without callbacks should be `true`.
///
/// The count gate precedes callbacks. An AtLimit candidate consumes one
/// shared draw, while a callback veto consumes none. The caller must supply
/// fresh counters and callbacks for every queue item.
pub fn rare_candidate_states(
    event: Option<u8>,
    global_word: u32,
    population: [RarePopulation; 11],
    callback_allows: [bool; 11],
) -> [RareCandidateState; 32] {
    std::array::from_fn(|id| {
        if !(4..=14).contains(&id)
            || (id == 14 && (global_word >> 8) < 0x101)
            || (event == Some(7) && id != 8 && id != 11)
        {
            return RareCandidateState::Skip;
        }
        let counters = population[id - 4];
        let available = 3i32.wrapping_sub(counters.active).max(0);
        if i64::from(available) + i64::from(counters.reserved) <= 0 {
            RareCandidateState::AtLimit
        } else if !callback_allows[id - 4] {
            RareCandidateState::Skip
        } else {
            RareCandidateState::Eligible
        }
    })
}

/// Candidate states for the first ordinary JobType 0/1 due queue item with
/// applied event 0, 5, or 6 and no periodic ID 15 force.
///
/// The caller must establish one of these events, zero active/pending Rare counts, and
/// the result of every per-ID runtime callback, including any callback from
/// the game's runtime-built list (not proven closed to four known objects).
/// IDs 4–14 are the Rare set.
/// ID 14 also needs `(global_word >> 8) >= 257`.
/// ID 13's WAVE-start callback condition can be computed by
/// `enemy_object::artillery_initial_callback_allows`; later flag changes are
/// not covered by that initial result.
pub fn normal_first_rare_candidates(
    global_word: u32,
    callback_allows: [bool; 11],
) -> [RareCandidateState; 32] {
    rare_candidate_states(
        Some(0),
        global_word,
        [RarePopulation::default(); 11],
        callback_allows,
    )
}
