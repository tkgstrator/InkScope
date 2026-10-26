//! Conditional countdown for the common Zako timer and Event 7/8 gates.
//!
//! For ordinary Events 0/5/6, the scheduler processes due Rare queue items
//! before this timer. Events 7/8 bypass that queue's consumption but still
//! call the timer. Passing its gates does not establish a species, spot,
//! creation call, or successful actor registration.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ZakoTimerInput {
    /// Queue object `[director+0x6f0]+0x44` before this scheduler call's timer step.
    pub timer_before: i32,
    /// Observed ordinary-mode pre-countdown gate at `0x3764a28..0x3764a3c`.
    pub pre_countdown_allows: bool,
    /// Population/marker gate result; needed only when the timer is due.
    pub population_gate_allows: Option<bool>,
    /// Effective queue `+0x40`, copied into queue `+0x44` after that gate passes.
    pub reset_interval: Option<i32>,
    /// Effective optional virtual gate: true when bypassed or allowed, false
    /// when it vetoes. Needed only after the timer has been reset.
    pub post_reset_allows: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ZakoTimerStep {
    pub timer_after: i32,
    /// Whether this step copied `+0x40` into `+0x44`.
    pub timer_reset: bool,
    /// Whether control reaches the later Zako selection path. This is not a
    /// guarantee that `0x375d2a0` is called or an actor registers.
    pub spawn_path_entered: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZakoTimerError {
    MissingPopulationGate,
    MissingResetInterval,
    MissingPostResetOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecialZakoEvent {
    Relay7,
    Tamaire8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpecialZakoTimerInput {
    /// The applied event of a confirmed nonnull scene pointer.
    pub event: SpecialZakoEvent,
    pub common: ZakoTimerInput,
    /// Director `+0x44c`; read before countdown for Relay only. Compute
    /// `common.population_gate_allows` from this same director snapshot.
    pub active_total: Option<i32>,
    /// Runtime candidate count from `[director+0x6f8]+0x68` for Relay or
    /// `[director+0x6f8]+0x48` for Tamaire; read only when the timer is due.
    pub runtime_candidate_count: Option<i32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecialZakoTimerError {
    MissingActiveTotal,
    MissingCandidateCount,
    Common(ZakoTimerError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RushNominalError {
    /// The caller supplied a source sentinel or otherwise unresolved reset.
    UnresolvedEffectiveInterval(i32),
}

/// Evaluate the common timer's live population gate at `0x3764ad8..0x3764afc`.
///
/// `active_total` is director `+0x44c`, `follower_budget` is director `+0x448`,
/// `deadline_marker` is queue `+0x4e8`, and `queue_time` is queue `+0x30`. The ELF's
/// 32-bit conditional increment wraps before its signed budget comparison.
pub fn common_zako_population_gate_allows(
    active_total: i32,
    follower_budget: i32,
    deadline_marker: i32,
    queue_time: i32,
) -> bool {
    let adjusted = if deadline_marker != i32::MIN && deadline_marker < queue_time {
        active_total.wrapping_add(1)
    } else {
        active_total
    };
    adjusted < follower_budget
}

/// Step `queue+0x44` once after the current scheduler call's Rare queue path.
///
/// This covers ordinary Event 0/1/5/6; Event 1 skips the Rare queue itself.
/// Event 3/4/7/8 have additional gates and must not use this helper as a
/// complete timer model. The countdown branch is
/// signed: values at least 2 decrement; values at most 1 clamp to zero and
/// then inspect the population gate. Once that gate passes, the interval is
/// copied before the optional virtual callback can veto later creation.
pub fn step_common_zako_timer(input: ZakoTimerInput) -> Result<ZakoTimerStep, ZakoTimerError> {
    if !input.pre_countdown_allows {
        return Ok(ZakoTimerStep {
            timer_after: input.timer_before,
            timer_reset: false,
            spawn_path_entered: false,
        });
    }
    if input.timer_before >= 2 {
        return Ok(ZakoTimerStep {
            timer_after: input.timer_before - 1,
            timer_reset: false,
            spawn_path_entered: false,
        });
    }
    let allows = input
        .population_gate_allows
        .ok_or(ZakoTimerError::MissingPopulationGate)?;
    if !allows {
        return Ok(ZakoTimerStep {
            timer_after: 0,
            timer_reset: false,
            spawn_path_entered: false,
        });
    }
    let interval = input
        .reset_interval
        .ok_or(ZakoTimerError::MissingResetInterval)?;
    let post_reset_allows = input
        .post_reset_allows
        .ok_or(ZakoTimerError::MissingPostResetOutcome)?;
    Ok(ZakoTimerStep {
        timer_after: interval,
        timer_reset: true,
        spawn_path_entered: post_reset_allows,
    })
}

/// Step the common Zako timer under Relay (Event 7) or Tamaire (Event 8) gates.
///
/// The caller must have confirmed a nonnull scene pointer and its applied
/// event. Relay's signed active-total cap is checked before countdown. Both
/// events inspect their signed runtime candidate count only when due. Passing
/// these gates does not establish a creation call or actor registration.
pub fn step_relay_or_tamaire_zako_timer(
    input: SpecialZakoTimerInput,
) -> Result<ZakoTimerStep, SpecialZakoTimerError> {
    if !input.common.pre_countdown_allows {
        return step_common_zako_timer(input.common).map_err(SpecialZakoTimerError::Common);
    }

    if input.event == SpecialZakoEvent::Relay7 {
        let active_total = input
            .active_total
            .ok_or(SpecialZakoTimerError::MissingActiveTotal)?;
        if active_total > 28 {
            return Ok(ZakoTimerStep {
                timer_after: input.common.timer_before,
                timer_reset: false,
                spawn_path_entered: false,
            });
        }
    }

    if input.common.timer_before >= 2 {
        return step_common_zako_timer(input.common).map_err(SpecialZakoTimerError::Common);
    }

    let candidates = input
        .runtime_candidate_count
        .ok_or(SpecialZakoTimerError::MissingCandidateCount)?;
    if candidates < 1 {
        return Ok(ZakoTimerStep {
            timer_after: 0,
            timer_reset: false,
            spawn_path_entered: false,
        });
    }

    step_common_zako_timer(input.common).map_err(SpecialZakoTimerError::Common)
}

/// Conditional EventRush Zako-path call indices under ideal timer gates.
///
/// The returned zero-based indices count scheduler calls, not seconds, actor
/// creation calls, or registered enemies. This assumes Event 1's ordinary
/// timer path, an explicitly assumed initial `queue+0x44=0`, constant resolved
/// queue `+0x40`, and that
/// mode, population, and post-reset gates allow every due attempt. A negative
/// EventRush source value (notably difficulty 0's -1) requires the effective
/// Round fallback interval first. For live varying gates or intervals, call
/// `step_common_zako_timer` with fresh inputs on each scheduler call instead.
pub fn nominal_rush_attempt_calls(
    effective_interval: i32,
    scheduler_call_count: u32,
) -> Result<Vec<u32>, RushNominalError> {
    if effective_interval < 0 {
        return Err(RushNominalError::UnresolvedEffectiveInterval(
            effective_interval,
        ));
    }
    let mut timer = 0;
    let mut attempts = Vec::new();
    for call_index in 0..scheduler_call_count {
        let step = step_common_zako_timer(ZakoTimerInput {
            timer_before: timer,
            pre_countdown_allows: true,
            population_gate_allows: Some(true),
            reset_interval: Some(effective_interval),
            post_reset_allows: Some(true),
        })
        .expect("ideal timer inputs are complete");
        timer = step.timer_after;
        if step.spawn_path_entered {
            attempts.push(call_index);
        }
    }
    Ok(attempts)
}
