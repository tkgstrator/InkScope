//! Direct event/tide writes when JobType 1 applies a generated WAVE record.
//!
//! This is the state at the end of the direct writes in 0x37f6920, not a
//! guarantee about opaque event handlers or later game-state changes.

use crate::records::WaveSelection;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobOneApplyState {
    /// `+0x598` read through the global singleton pointer before the
    /// generated record is copied. Supply 0 if that pointer is null. It may
    /// differ from `SpecialWaveState.stored_tide` used during generation.
    pub previous_applied_tide: u32,
    /// Return value of the runtime player-count helper at application time.
    /// The executable compares it as a signed 32-bit integer with 1.
    pub active_player_count: i32,
    /// Runtime context bytes `+0x591` and `+0x624`.
    pub mode_flag_591: bool,
    pub special_flag_624: bool,
    /// Runtime context word `+0x620`.
    pub special_target_index_620: u32,
    /// The runtime object `[+0x6690][+0x3c]` is available and
    /// `(raw_mode & !1) == 2`. Without the special-target gate this forces
    /// the applied tide to 1. Supply false when the object is absent.
    pub runtime_mode_forces_mid_tide: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectAppliedWave {
    pub wave_index: u8,
    pub event_id: u8,
    /// Raw context `+0x594` after direct writes. Its game-facing name is
    /// unconfirmed; 0/2/1 distinguish event 0/5/other, and special target 3.
    pub applied_word_594: u32,
    /// The executable copies raw 32-bit context values; no range clamp is
    /// imposed on `previous_applied_tide` when the special branch uses it.
    pub tide_id: u32,
    pub special_tide_used: bool,
    /// Event 8 takes an indirect path whose later effects are unresolved.
    pub requires_handler_observation: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyJobOneError {
    InvalidWaveIndex(u8),
    InvalidGeneratedEvent(u8),
    InvalidGeneratedTide(u8),
}

/// Raw context words after copying the generated record and applying the
/// event-8-specific configuration branch. These are not enemy spawn results.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventEightAppliedFields {
    pub word_59c: u32,
    pub word_5a0: u32,
    pub event_eight_override_applied: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventEightFieldsError {
    UnknownEffectiveConfig,
}

/// Project the direct writes of JobType 1's WAVE application path.
///
/// 11.3.0 ELF `0x37f6920..0x37f6d60` copies the record, forces event zero
/// for at most one active player, writes tide zero for applied event 6/7,
/// then optionally restores the previously applied tide or forces tide 1
/// for runtime mode class 2/3. The special gate requires both runtime flags
/// and a target index match and takes precedence. Event 5 sets raw
/// word `+0x594` to 2 and returns through direct writes; event 8 takes an
/// indirect path whose later effects are outside this projection.
pub fn apply_job_one_direct_writes(
    generated: WaveSelection,
    state: JobOneApplyState,
) -> Result<DirectAppliedWave, ApplyJobOneError> {
    if generated.wave_index > 4 {
        return Err(ApplyJobOneError::InvalidWaveIndex(generated.wave_index));
    }
    if generated.event_id > 8 {
        return Err(ApplyJobOneError::InvalidGeneratedEvent(generated.event_id));
    }
    if generated.tide_id > 2 {
        return Err(ApplyJobOneError::InvalidGeneratedTide(generated.tide_id));
    }

    let event_id = if state.active_player_count <= 1 {
        0
    } else {
        generated.event_id
    };
    let mut tide_id = if matches!(event_id, 6 | 7) {
        0
    } else {
        u32::from(generated.tide_id)
    };
    let special_tide_used = state.mode_flag_591
        && state.special_flag_624
        && state.special_target_index_620 == u32::from(generated.wave_index);
    if special_tide_used {
        tide_id = state.previous_applied_tide;
    } else if state.runtime_mode_forces_mid_tide {
        tide_id = 1;
    }
    let applied_word_594 = if special_tide_used {
        3
    } else {
        match event_id {
            0 => 0,
            5 => 2,
            _ => 1,
        }
    };
    Ok(DirectAppliedWave {
        wave_index: generated.wave_index,
        event_id,
        applied_word_594,
        tide_id,
        special_tide_used,
        requires_handler_observation: event_id == 8,
    })
}

/// Project the two raw context words copied from record fields at context
/// offsets `+0x5c4/+0x5c8` (zero-based record indices 2/3).
///
/// The event-8 branch uses the *applied* event after the one-player override.
/// Its config byte is the effective runtime value of
/// `InGame.TamaireSpanwZakoFromSpawnPoint`, not necessarily the owned RomFS
/// value. This local branch makes no event/tide write or RNG draw. Event-8
/// enemy behavior and later uses of `+0x5a0` remain unresolved.
pub fn project_job_one_event_eight_fields(
    direct: DirectAppliedWave,
    record_word_5c4: u32,
    record_word_5c8: u32,
    effective_tamaire_spawn_from_point: Option<bool>,
) -> Result<EventEightAppliedFields, EventEightFieldsError> {
    if direct.event_id != 8 {
        return Ok(EventEightAppliedFields {
            word_59c: record_word_5c4,
            word_5a0: record_word_5c8,
            event_eight_override_applied: false,
        });
    }
    let Some(config) = effective_tamaire_spawn_from_point else {
        return Err(EventEightFieldsError::UnknownEffectiveConfig);
    };
    Ok(EventEightAppliedFields {
        word_59c: if config { record_word_5c4 } else { 4 },
        word_5a0: if config { record_word_5c8 } else { u32::MAX },
        event_eight_override_applied: !config,
    })
}
