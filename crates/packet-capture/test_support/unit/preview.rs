use super::*;
use crate::decode::Field;

fn setting_row() -> Row {
    let mut fields = vec![
        ("PayloadBytes", "211"),
        ("TypeId", "0x9a1e036d"),
        ("StageResource", "Cop_Shakedent"),
        ("JobType", "1"),
        ("Difficulty", "150"),
    ];
    fields.extend([
        ("GameSeed[0]", "1"),
        ("GameSeed[1]", "2"),
        ("GameSeed[2]", "3"),
        ("GameSeed[3]", "4"),
    ]);
    Row {
        record: 1,
        timestamp_ns: 0,
        source: "192.0.2.1:49152".into(),
        destination: "192.0.2.2:49152".into(),
        source_mac: "02:00:00:00:00:01".into(),
        destination_mac: "02:00:00:00:00:02".into(),
        direction: None,
        kind: "OnlineCoopSetting".into(),
        status: "authenticated".into(),
        layout: Some("confirmed_fields".into()),
        protocol: Some(8),
        port: None,
        chunk: Some(0),
        fields: fields
            .into_iter()
            .map(|(name, value)| Field {
                name: name.into(),
                value: value.into(),
                bits: None,
            })
            .collect(),
        authenticated_payload_hex: None,
        wave_preview: None,
    }
}

fn equipment_row() -> Row {
    let mut row = setting_row();
    for (name, value) in [
        ("UnlockedSeason", "4"),
        ("WeaponIdCount", "4"),
        ("WeaponId[0]", "22030"),
        ("WeaponId[1]", "22030"),
        ("WeaponId[2]", "22030"),
        ("WeaponId[3]", "22030"),
        ("ScenarioAppVersion", "721664"),
        ("ScenarioCompetitionType", "0"),
        ("ScenarioOriginalWeaponIdCount", "0"),
    ] {
        row.fields.push(Field {
            name: name.into(),
            value: value.into(),
            bits: None,
        });
    }
    row
}

#[test]
fn equipment_preview_preserves_all_five_fixed_rows_and_conditional_evidence() {
    let preview = WavePreviewer::default().observe(&equipment_row()).unwrap();
    let json = serde_json::to_value(preview).unwrap();
    assert_eq!(json["equipment"]["status"], "conditional");
    assert_eq!(json["equipment"]["source"], "OnlineCoopSetting");
    assert_eq!(
        json["equipment"]["main_weapons"],
        serde_json::to_value([[22030; 4]; 5]).unwrap()
    );
    assert!(
        !json["equipment"]["assumptions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn changed_equipment_metadata_is_not_deduplicated_as_the_old_setting() {
    for name in [
        "UnlockedSeason",
        "WeaponId[1]",
        "ScenarioAppVersion",
        "ScenarioCompetitionType",
        "ScenarioOriginalWeaponIdCount",
    ] {
        let mut previewer = WavePreviewer::default();
        let mut row = equipment_row();
        assert!(previewer.observe(&row).is_some());
        row.fields
            .iter_mut()
            .find(|field| field.name == name)
            .unwrap()
            .value = match name {
            "UnlockedSeason" => "3",
            "WeaponId[1]" => "-1",
            "ScenarioAppVersion" => "1",
            _ => "1",
        }
        .into();
        if name == "ScenarioOriginalWeaponIdCount" {
            row.fields.push(Field {
                name: "ScenarioOriginalWeaponId[0]".into(),
                value: "-1".into(),
                bits: None,
            });
        }
        assert!(previewer.observe(&row).is_some(), "{name}");
        assert!(previewer.observe(&row).is_none(), "{name}");
    }
}

#[test]
fn regular_stage_has_only_non_big_run_scenarios_and_deduplicates() {
    let mut previewer = WavePreviewer::default();
    let row = setting_row();
    let preview = previewer.observe(&row).expect("first setting");
    assert_eq!(preview.status, "conditional");
    assert_eq!(preview.scenarios.len(), 2);
    assert!(
        preview
            .scenarios
            .iter()
            .all(|scenario| !scenario.is_big_run)
    );
    assert!(
        preview
            .scenarios
            .iter()
            .all(|scenario| scenario.waves.len() == 5)
    );
    assert!(
        preview
            .assumptions
            .iter()
            .any(|text| text.contains("unverified"))
    );
    assert!(previewer.observe(&row).is_none());

    let mut changed = setting_row();
    changed
        .fields
        .iter_mut()
        .find(|f| f.name == "GameSeed[0]")
        .unwrap()
        .value = "5".into();
    assert!(previewer.observe(&changed).is_some());
    let json = serde_json::to_value(preview).unwrap();
    assert_eq!(json["scenarios"].as_array().unwrap().len(), 2);
    assert!(json.get("equipment").is_none());
}

#[test]
fn big_run_stage_has_only_big_run_scenarios() {
    let mut row = setting_row();
    row.fields
        .iter_mut()
        .find(|field| field.name == "StageResource")
        .unwrap()
        .value = "Cop_Carousel".into();
    let preview = WavePreviewer::default().observe(&row).unwrap();
    assert_eq!(preview.scenarios.len(), 2);
    assert!(preview.scenarios.iter().all(|scenario| scenario.is_big_run));
}

#[test]
fn untrusted_or_unconfirmed_setting_never_triggers_preview() {
    let mut previewer = WavePreviewer::default();
    for (status, layout, length) in [
        ("authentication_failed", "confirmed_fields", "211"),
        ("authenticated", "type_fields", "211"),
        ("authenticated", "confirmed_fields", "210"),
    ] {
        let mut row = setting_row();
        row.status = status.into();
        row.layout = Some(layout.into());
        row.fields
            .iter_mut()
            .find(|f| f.name == "PayloadBytes")
            .unwrap()
            .value = length.into();
        assert!(previewer.observe(&row).is_none());
    }
    assert!(previewer.observe(&setting_row()).is_some());
}

#[test]
fn unknown_stage_and_job_type_report_reason_without_claiming_prediction() {
    let mut previewer = WavePreviewer::default();
    let mut row = setting_row();
    row.fields
        .iter_mut()
        .find(|f| f.name == "StageResource")
        .unwrap()
        .value = "Cop_Unknown".into();
    let preview = previewer.observe(&row).unwrap();
    assert_eq!(preview.status, "unavailable");
    assert!(preview.reason.unwrap().contains("stage data"));

    let mut row = setting_row();
    row.fields.retain(|field| field.name != "StageResource");
    let preview = previewer.observe(&row).unwrap();
    assert_eq!(preview.status, "unavailable");
    assert!(preview.reason.unwrap().contains("StageResource missing"));

    let mut row = setting_row();
    row.fields
        .iter_mut()
        .find(|f| f.name == "JobType")
        .unwrap()
        .value = "7".into();
    let preview = previewer.observe(&row).unwrap();
    assert_eq!(preview.status, "unavailable");
    assert!(preview.reason.unwrap().contains("JobType 7"));
}
