//! JobType 0 scenario main weapons before the conditional scene transfer.
//!
//! The setting ring is not proof of the later scene ring: an intervening
//! virtual copy can stop and the controller-to-scene copy must also run.

#[derive(Clone, Copy, Debug)]
pub struct ScenarioMainWeaponRing<'a> {
    /// All physical source slots for the actual ring capacity, including
    /// index zero when it is used to pad. Slice length is treated as capacity;
    /// a slice containing only the logical entries is insufficient.
    pub physical_ids: &'a [i32],
    pub start: usize,
    pub count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingWeaponRing {
    /// Capacity-four setting ring after the normal JobType 0 fill loop.
    pub physical_ids: [i32; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScenarioWeaponError {
    /// With no logical entries, the game still reads source physical index 0;
    /// scenario contents alone do not establish that value.
    EmptyIndeterminate,
    TooManyEntries(usize),
    InvalidRing {
        capacity: usize,
        start: usize,
        count: usize,
    },
}

/// Fill the four setting entries from the JobType 0 scenario main-weapon ring.
///
/// The first `min(count, 4)` entries follow logical ring order. Any remaining
/// entries use source physical index zero, which need not be the logical first.
/// This output may be supplied as a scene ring only after the intervening
/// setting/controller/scene copies are independently established as successful
/// and unmodified. The source decoder permits at most twelve entries.
pub fn scenario_main_weapon_setting_ring(
    source: ScenarioMainWeaponRing<'_>,
) -> Result<SettingWeaponRing, ScenarioWeaponError> {
    if source.count == 0 {
        return Err(ScenarioWeaponError::EmptyIndeterminate);
    }
    if source.count > 12 {
        return Err(ScenarioWeaponError::TooManyEntries(source.count));
    }
    let capacity = source.physical_ids.len();
    if capacity == 0 || capacity > 12 || source.start >= capacity || source.count > capacity {
        return Err(ScenarioWeaponError::InvalidRing {
            capacity,
            start: source.start,
            count: source.count,
        });
    }
    let physical_ids = std::array::from_fn(|index| {
        if index < source.count {
            source.physical_ids[(source.start + index) % capacity]
        } else {
            source.physical_ids[0]
        }
    });
    Ok(SettingWeaponRing { physical_ids })
}
