use salmon_wave_lookahead::scenario_weapon::{
    ScenarioMainWeaponRing, ScenarioWeaponError, SettingWeaponRing,
    scenario_main_weapon_setting_ring,
};

fn source<'a>(physical_ids: &'a [i32], start: usize, count: usize) -> ScenarioMainWeaponRing<'a> {
    ScenarioMainWeaponRing {
        physical_ids,
        start,
        count,
    }
}

#[test]
fn job_zero_setting_ring_fills_four_entries_from_source_physical_zero() {
    for (ids, count, expected) in [
        (vec![10], 1, [10, 10, 10, 10]),
        (vec![10, 20], 2, [10, 20, 10, 10]),
        (vec![10, 20, 30], 3, [10, 20, 30, 10]),
        (vec![10, 20, 30, 40], 4, [10, 20, 30, 40]),
        (vec![10, 20, 30, 40, 50], 5, [10, 20, 30, 40]),
    ] {
        assert_eq!(
            scenario_main_weapon_setting_ring(source(&ids, 0, count)),
            Ok(SettingWeaponRing {
                physical_ids: expected
            })
        );
    }
}

#[test]
fn shifted_source_uses_logical_entries_but_physical_zero_padding() {
    assert_eq!(
        scenario_main_weapon_setting_ring(source(&[99, 10, 20, 77], 1, 2)),
        Ok(SettingWeaponRing {
            physical_ids: [10, 20, 99, 99]
        })
    );
    assert_eq!(
        scenario_main_weapon_setting_ring(source(&[30, 40, 10, 20], 2, 4)),
        Ok(SettingWeaponRing {
            physical_ids: [10, 20, 30, 40]
        })
    );
}

#[test]
fn empty_and_invalid_scenario_rings_do_not_invent_scene_candidates() {
    assert_eq!(
        scenario_main_weapon_setting_ring(source(&[123], 0, 0)),
        Err(ScenarioWeaponError::EmptyIndeterminate)
    );
    assert_eq!(
        scenario_main_weapon_setting_ring(source(&[1, 2, 3, 4], 4, 2)),
        Err(ScenarioWeaponError::InvalidRing {
            capacity: 4,
            start: 4,
            count: 2
        })
    );
    assert_eq!(
        scenario_main_weapon_setting_ring(source(&[1, 2], 0, 3)),
        Err(ScenarioWeaponError::InvalidRing {
            capacity: 2,
            start: 0,
            count: 3
        })
    );
    assert_eq!(
        scenario_main_weapon_setting_ring(source(&[0; 13], 0, 13)),
        Err(ScenarioWeaponError::TooManyEntries(13))
    );
}
