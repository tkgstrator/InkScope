//! Evidence-scoped bridge from scenario seed entries to scene seed words.
//!
//! The 11.3.0 controller reads `CoopScenario.RandomSeedAry` as a ring and
//! forms the four-word game seed. Callers supply its **logical** entry order, after
//! applying the runtime ring start and capacity. This module does not parse a
//! packet or save file, establish that those entries reached the controller,
//! or infer the current scene from scenario contents alone.
//!
//! The game seed words can be replaced by a separate global RNG path or a later
//! construction path. Even if copied into the scene at an observed activation
//! point, later scene overwrites and shared RNG initialization are separate
//! events. The resolver therefore requires four explicit observations.

use crate::rng::{SeedWords, XorShift128};

/// An invalid or indeterminate logical scenario seed array.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScenarioSeedError {
    /// The zero-entry path reads a scratch word not initialized by this block.
    EmptyIndeterminate,
    /// The 11.3.0 decoder accepts at most four entries.
    TooManyEntries(usize),
}

/// Expand one to four logical scenario entries into the four-word game seed.
///
/// Missing positions use the **first** entry, not a cycle over all entries.
/// This does not normalize `[0; 4]`; normalization occurs only if the shared
/// RNG later initializes from the resulting raw scene fields.
pub fn expand_scenario_seeds(entries: &[u32]) -> Result<[u32; 4], ScenarioSeedError> {
    let Some(&first) = entries.first() else {
        return Err(ScenarioSeedError::EmptyIndeterminate);
    };
    if entries.len() > 4 {
        return Err(ScenarioSeedError::TooManyEntries(entries.len()));
    }
    let mut words = [first; 4];
    words[..entries.len()].copy_from_slice(entries);
    Ok(words)
}

/// Caller-supplied evidence about one scenario-to-scene activation path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Observation {
    Confirmed,
    Refuted,
    Unknown,
}

/// The reason a scenario-derived scene seed is refuted or unresolved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScenarioSeedFact {
    ControllerEntriesRead,
    PreservedAtW28Gate,
    ExactControllerWords,
    SceneCopyAtActivation,
}

/// Four distinct facts required to connect the supplied entries to a scene.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScenarioSeedObservations {
    /// The supplied logical entries were present at controller `+0x1b0` and
    /// read by `0x3894fcc..0x3895024`.
    pub controller_entries_read: Observation,
    /// `w28 & 1` retained the expanded game seed at `0x3896028`; the
    /// global RNG replacement branch did not run.
    pub preserved_at_w28_gate: Observation,
    /// Those exact four words survived downstream construction and appeared
    /// at controller `+0x7f0..+0x7fc` immediately before scene transfer.
    /// Establish this from the proved direct path or a four-word readback;
    /// the gate above alone is insufficient.
    pub exact_controller_words: Observation,
    /// The actual `0x3896d70..0x3896dcc` copy installed those controller
    /// words into scene `+0x2e0..+0x2ec` at the caller's activation point.
    /// Controller flags alone do not confirm that the copy ran.
    pub scene_copy_at_activation: Observation,
}

/// Scene-word conclusion at the activation point supplied by the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScenarioSeedResolution {
    Confirmed {
        /// Raw scene fields at the observed activation point. All zeros remain
        /// zero here, and later overwrites are not ruled out.
        raw_scene_words: [u32; 4],
        /// Value the shared RNG would use **if** `0x37acfd0` subsequently
        /// initializes from these same scene fields. This does not establish
        /// that initialization happened.
        rng_seed_if_initialized: SeedWords,
    },
    ScenarioPathRefuted(ScenarioSeedFact),
    Unresolved(ScenarioSeedFact),
}

/// Resolve the scenario path only when all four observations are confirmed.
///
/// Any refuted fact takes priority over unknown facts. The result concerns the
/// specified activation point; it is not a claim about later scene state.
pub fn resolve_scenario_seed(
    entries: &[u32],
    observations: ScenarioSeedObservations,
) -> Result<ScenarioSeedResolution, ScenarioSeedError> {
    let raw_scene_words = expand_scenario_seeds(entries)?;
    let facts = [
        (
            ScenarioSeedFact::ControllerEntriesRead,
            observations.controller_entries_read,
        ),
        (
            ScenarioSeedFact::PreservedAtW28Gate,
            observations.preserved_at_w28_gate,
        ),
        (
            ScenarioSeedFact::ExactControllerWords,
            observations.exact_controller_words,
        ),
        (
            ScenarioSeedFact::SceneCopyAtActivation,
            observations.scene_copy_at_activation,
        ),
    ];
    if let Some((fact, _)) = facts
        .iter()
        .find(|(_, observation)| *observation == Observation::Refuted)
    {
        return Ok(ScenarioSeedResolution::ScenarioPathRefuted(*fact));
    }
    if let Some((fact, _)) = facts
        .iter()
        .find(|(_, observation)| *observation == Observation::Unknown)
    {
        return Ok(ScenarioSeedResolution::Unresolved(*fact));
    }
    Ok(ScenarioSeedResolution::Confirmed {
        raw_scene_words,
        rng_seed_if_initialized: SeedWords::from_scene(raw_scene_words),
    })
}

/// Four game seed words from the separate `w28 & 1 == 0` global RNG path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlobalRngSettingWords {
    pub setting_words: [u32; 4],
    pub advanced_global_state: XorShift128,
}

/// Draw four consecutive words from an explicitly supplied global RNG state.
///
/// This path mutates the game's global state and replaces the scenario-derived
/// game seed words. Its incoming state cannot be inferred from `RandomSeedAry`.
pub fn global_rng_setting_words(mut incoming: XorShift128) -> GlobalRngSettingWords {
    let setting_words = std::array::from_fn(|_| incoming.next32());
    GlobalRngSettingWords {
        setting_words,
        advanced_global_state: incoming,
    }
}
