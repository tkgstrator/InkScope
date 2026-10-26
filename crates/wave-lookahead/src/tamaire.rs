//! Event 8 Tamaire's next location candidate before spawn dispatch.
//!
//! This models one conditional attempt from a live controller snapshot. The
//! attempt time, actor manager contents, preceding local RNG draws, and the
//! later spawn result are not determined here.

use crate::rng::{XorShift128, expand_seed_word};

const CANDIDATE_RNG_DOMAIN: u32 = 0xb3f8_3173;

/// The first Event 8 candidate RNG state after WAVE reset at `0x3792d88`.
///
/// `Some` contains the four words stored in the shared RNG object. A missing
/// object and an existing all-zero object take different fallback paths in
/// the ELF. This state precedes any callback or spawn-attempt draws.
pub fn initial_candidate_rng_state(shared_words: Option<[u32; 4]>) -> [u32; 4] {
    let effective = match shared_words {
        None => [1, 0, 0, 0],
        Some([0, 0, 0, 0]) => [1, 0x6c07_8967, 0, 0x4807_7044],
        Some(words) => words,
    };
    let mut source = XorShift128::from_words(effective);
    expand_seed_word(source.next32().wrapping_add(CANDIDATE_RNG_DOMAIN)).as_array()
}

/// Event 8's three signed numeric-type weights.
///
/// `choose_numeric_type` expects effective runtime getters from scene
/// `+0x4c8`; `levels::common_tamaire_type_weights` returns only a stock
/// resource calculation in the same shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TamaireTypeWeights {
    pub large: i32,
    pub small: i32,
    pub standard: i32,
}

/// State immediately before the Event 8 branch at `0x3794188`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TamaireTypeInput {
    /// Effective runtime getter values, or `None` when the configuration
    /// pointer is null. In that case the game keeps numeric type 20 without
    /// reading flags or drawing from this local stream.
    pub weights: Option<TamaireTypeWeights>,
    /// Controller byte `+0x1ad4` is nonzero.
    pub forced_21: bool,
    /// Controller byte `+0x1ad5` is nonzero before this attempt.
    pub emitted_21: bool,
    /// Current controller `+0x1938..+0x1944`, after location and other draws.
    pub rng_words: [u32; 4],
}

/// Conditional numeric type and local state after the Event 8 choice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TamaireTypeChoice {
    pub numeric_type: u8,
    pub emitted_21: bool,
    /// The xorshift output, only when the nonnegative-sum branch draws.
    pub random_word: Option<u32>,
    /// `high32(random_word * (Small + Standard))` on that branch.
    pub roll: Option<u32>,
    pub rng_words: [u32; 4],
}

/// Choose numeric type 20 or 21 after the earlier location gates reach
/// `0x3794188` in the owned 11.3.0 executable.
///
/// A null config leaves the default 20. Otherwise an as-yet-unemitted forced
/// 21 skips RNG. The ordinary path uses a wrapping 32-bit sum; a negative
/// signed result skips RNG and compares Large with zero. A nonnegative sum,
/// including zero, consumes one draw and compares signed Large to its scaled
/// roll. This is only a type selected before subsequent dispatch; it does not
/// establish actor registration or the final location.
pub fn choose_numeric_type(input: TamaireTypeInput) -> TamaireTypeChoice {
    let Some(weights) = input.weights else {
        return TamaireTypeChoice {
            numeric_type: 20,
            emitted_21: input.emitted_21,
            random_word: None,
            roll: None,
            rng_words: input.rng_words,
        };
    };
    if input.forced_21 && !input.emitted_21 {
        return TamaireTypeChoice {
            numeric_type: 21,
            emitted_21: true,
            random_word: None,
            roll: None,
            rng_words: input.rng_words,
        };
    }

    let sum = weights.small.wrapping_add(weights.standard);
    let (random_word, roll, rng_words, threshold) = if sum < 0 {
        (None, None, input.rng_words, 0)
    } else {
        let mut rng = XorShift128::from_words(input.rng_words);
        let draw = rng.next32();
        let roll = ((u64::from(draw) * u64::from(sum as u32)) >> 32) as u32;
        (Some(draw), Some(roll), rng.as_array(), roll as i32)
    };
    let chose_21 = weights.large > threshold;
    TamaireTypeChoice {
        numeric_type: if chose_21 { 21 } else { 20 },
        emitted_21: input.emitted_21 || chose_21,
        random_word,
        roll,
        rng_words,
    }
}

/// Opaque 56 bytes of a location node that may be read after the draw.
///
/// The 11.3.0 controller stores pointers to nodes in its tide lists. It
/// copies node offsets `+0..+0x37` only after later dispatch gates succeed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TamaireNode(pub [u32; 14]);

/// Runtime tide-list snapshot in insertion order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TamaireGroup {
    pub count: i32,
    pub nodes: Vec<TamaireNode>,
}

/// Inputs at one call to the Event 8 location routine `0x3793980`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TamaireSnapshot {
    /// Scene `+0x598`; `None` means the scene pointer is absent.
    pub tide: Option<u32>,
    pub groups: [TamaireGroup; 3],
    /// Live working-index storage capacity at controller `+0x1950`.
    pub work_capacity: i32,
    /// The actor manager checked at `0x3793a28` must exist before filtering.
    pub actor_manager_present: bool,
    /// Actor `+0x158` values actually read by eligible removal callbacks,
    /// in the two runtime actor-array traversal order. Nonmatching values
    /// simply leave the working indices unchanged.
    pub active_callback_indices: Vec<u32>,
    /// Controller marks at `+0x1a50`, indexed by the selected group's source
    /// index. `i32::MIN` is the unused sentinel.
    pub cooldown_marks: Vec<i32>,
    /// Current runtime count used for cooldown comparison, clamped to >=0.
    pub now: i32,
    /// Current controller `+0x1938..+0x1944`, after any earlier draws.
    pub rng_words: [u32; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TamaireError {
    InvalidWorkCapacity(i32),
    InvalidSelectedGroup(&'static str),
    MissingCooldownMarks,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TamaireSkipReason {
    EmptyList,
    MissingActorManager,
    AllFiltered,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TamaireAttempt {
    Skipped {
        group_index: usize,
        reason: TamaireSkipReason,
        rng_unchanged: [u32; 4],
    },
    Selected {
        group_index: usize,
        source_index: usize,
        remaining_before_draw: usize,
        rng_after_draw: [u32; 4],
    },
}

/// Choose one candidate index from the live Event 8 tide list.
///
/// The game removes active locations first, then locations with a future
/// cooldown mark; both removals replace an entry with the current tail. A
/// nonempty filtered list consumes exactly one draw. The returned index is
/// not a confirmed source-node read: a later indirect gate and global pointer
/// check can return, and the tide is read again before the node lookup.
/// Invalid metadata is rejected for diagnostics rather than emulating unsafe
/// behavior on corrupted game objects.
pub fn choose_candidate(input: &TamaireSnapshot) -> Result<TamaireAttempt, TamaireError> {
    let group_index = tide_group_index(input.tide);
    let group = &input.groups[group_index];
    if !(0..=32).contains(&input.work_capacity) {
        return Err(TamaireError::InvalidWorkCapacity(input.work_capacity));
    }
    validate_group_count(group)?;
    if !input.actor_manager_present {
        return Ok(TamaireAttempt::Skipped {
            group_index,
            reason: TamaireSkipReason::MissingActorManager,
            rng_unchanged: input.rng_words,
        });
    }
    if group.count == 0 {
        return Ok(TamaireAttempt::Skipped {
            group_index,
            reason: TamaireSkipReason::EmptyList,
            rng_unchanged: input.rng_words,
        });
    }
    if input.cooldown_marks.len() < group.count as usize {
        return Err(TamaireError::MissingCooldownMarks);
    }

    let count = group.count.min(input.work_capacity) as usize;
    let mut available: Vec<u32> = (0..count as u32).collect();
    for index in &input.active_callback_indices {
        remove_first(&mut available, *index);
    }
    let now = input.now.max(0);
    for index in 0..group.count as usize {
        let mark = input.cooldown_marks[index];
        if mark != i32::MIN && now < mark {
            remove_first(&mut available, index as u32);
        }
    }
    if available.is_empty() {
        return Ok(TamaireAttempt::Skipped {
            group_index,
            reason: TamaireSkipReason::AllFiltered,
            rng_unchanged: input.rng_words,
        });
    }

    let remaining_before_draw = available.len();
    let mut rng = XorShift128::from_words(input.rng_words);
    let draw = rng.next32();
    let slot = ((u64::from(draw) * remaining_before_draw as u64) >> 32) as usize;
    let source_index = available[slot] as usize;
    Ok(TamaireAttempt::Selected {
        group_index,
        source_index,
        remaining_before_draw,
        rng_after_draw: rng.as_array(),
    })
}

/// Return the raw source node only after both post-draw continuation gates.
///
/// The first flag means execution passes the conditional indirect gate at
/// `0x3793d40..0x3793d50`; the second means the separate global object at
/// `0x3793d54..0x3793d60` exists. The game then reads the tide again and
/// looks up the previously selected index in that list. A changed tide can
/// therefore identify a different node. Neither gate outcome nor a later
/// actor registration is derived here.
pub fn lookup_source_after_gates(
    groups: &[TamaireGroup; 3],
    source_index: usize,
    later_tide: Option<u32>,
    indirect_path_continues: bool,
    global_object_present: bool,
) -> Result<Option<TamaireNode>, TamaireError> {
    if !indirect_path_continues || !global_object_present {
        return Ok(None);
    }
    let group = &groups[tide_group_index(later_tide)];
    validate_selected_group(group)?;
    if source_index >= group.count as usize {
        return Err(TamaireError::InvalidSelectedGroup(
            "selected index outside later group",
        ));
    }
    Ok(Some(group.nodes[source_index]))
}

fn tide_group_index(tide: Option<u32>) -> usize {
    match tide {
        Some(tide @ 0..=2) => tide as usize,
        _ => 0,
    }
}

fn validate_selected_group(group: &TamaireGroup) -> Result<(), TamaireError> {
    validate_group_count(group)?;
    if group.nodes.len() < group.count as usize {
        return Err(TamaireError::InvalidSelectedGroup(
            "node storage shorter than count",
        ));
    }
    Ok(())
}

fn validate_group_count(group: &TamaireGroup) -> Result<(), TamaireError> {
    if !(0..=32).contains(&group.count) {
        return Err(TamaireError::InvalidSelectedGroup("count outside 0..32"));
    }
    Ok(())
}

fn remove_first(available: &mut Vec<u32>, index: u32) {
    if let Some(position) = available.iter().position(|candidate| *candidate == index) {
        available.swap_remove(position);
    }
}
