use super::*;

fn setting(main_ring: &[i32], version: u32) -> EquipmentSetting {
    EquipmentSetting {
        unlocked_season: 4,
        main_ring: main_ring.to_vec(),
        scenario_app_version: version,
        scenario_competition_type: 0,
        original_main_ring: vec![],
    }
}

#[test]
fn synthetic_seed_uses_post_special_rng_and_unpacks_all_twenty_main_ids() {
    // Independently calculated from the existing owned [1,2,3,4] special
    // vectors, the stock 57-entry season4 pool, and xorshift128 draws.
    for (version, expected) in [
        (
            0,
            [
                [24030, 20250, 25010, 20200],
                [20050, 20080, 21110, 20020],
                [27020, 20040, 20090, 24000],
                [20080, 20030, 22050, 27020],
                [22070, 21110, 25040, 26010],
            ],
        ),
        (
            3 << 17,
            [
                [20020, 27020, 20040, 20090],
                [24000, 20080, 20030, 22050],
                [27020, 22070, 21110, 25040],
                [26010, 23030, 20250, 20230],
                [20400, 20260, 20210, 21110],
            ],
        ),
    ] {
        let preview = calculate(&setting(&[-1; 4], version), 1, [1, 2, 3, 4]);
        assert_eq!(preview.status, "conditional");
        assert_eq!(preview.main_weapons, expected.map(|row| row.map(Some)));
    }
}

#[test]
fn unresolved_mode_cells_are_null_without_discarding_independent_cells() {
    let preview = calculate(&setting(&[65532, 22030, -1, 20040], 0), 1, [1, 2, 3, 4]);
    for row in preview.main_weapons {
        assert_eq!(row[0], None);
        assert_eq!(row[1], Some(22030));
        assert!(row[2].is_some());
        assert_eq!(row[3], Some(20040));
    }
}

#[test]
fn unsupported_job_or_slot_count_has_a_fixed_null_matrix_and_reason() {
    for (job_type, ids) in [(0, &[-1; 4][..]), (1, &[-1; 3][..])] {
        let preview = calculate(&setting(ids, 0), job_type, [1, 2, 3, 4]);
        assert_eq!(preview.status, "unavailable");
        assert_eq!(preview.main_weapons, [[None; 4]; 5]);
        assert!(preview.reason.is_some());
        let json = serde_json::to_value(preview).unwrap();
        assert_eq!(json["main_weapons"].as_array().unwrap().len(), 5);
        assert_eq!(json["main_weapons"][0].as_array().unwrap().len(), 4);
    }
}

#[test]
fn setting_field_roundtrip_retains_signed_ids_and_all_dedup_metadata() {
    let mut input = setting(&[-3, -1, 22030, 65532], u32::MAX);
    input.unlocked_season = -1;
    input.scenario_competition_type = 1;
    input.original_main_ring = vec![-2];
    assert_eq!(EquipmentSetting::from_fields(&input.fields()), Some(input));
}

#[test]
fn malformed_field_metadata_cannot_become_generator_input() {
    for (name, value) in [
        ("UnlockedSeason", "-2"),
        ("UnlockedSeason", "1023"),
        ("WeaponIdCount", "5"),
        ("WeaponId[0]", "65533"),
        ("WeaponId[0]", "-4"),
        ("ScenarioAppVersion", "4294967296"),
        ("ScenarioCompetitionType", "2"),
        ("ScenarioOriginalWeaponIdCount", "1"),
    ] {
        let mut fields = setting(&[-1; 4], 0).fields();
        fields
            .iter_mut()
            .find(|field| field.name == name)
            .unwrap()
            .value = value.into();
        assert!(EquipmentSetting::from_fields(&fields).is_none(), "{name}");
    }
}
