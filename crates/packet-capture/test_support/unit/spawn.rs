use super::*;
use crate::decode::Field;

fn test_row(kind: &str, fields: &[(&str, &str)]) -> Row {
    Row {
        record: 1,
        timestamp_ns: 0,
        source: "192.0.2.1:49152".into(),
        destination: "192.0.2.2:49152".into(),
        source_mac: "02:00:00:00:00:01".into(),
        destination_mac: "02:00:00:00:00:02".into(),
        direction: None,
        kind: kind.into(),
        status: "authenticated".into(),
        layout: Some("confirmed_fields".into()),
        protocol: Some(8),
        port: None,
        chunk: Some(0),
        fields: fields
            .iter()
            .map(|(name, value)| Field {
                name: (*name).into(),
                value: (*value).into(),
                bits: None,
            })
            .collect(),
        authenticated_payload_hex: None,
        wave_preview: None,
    }
}

fn setting(stage: &str, seed0: &str) -> Row {
    test_row(
        "OnlineCoopSetting",
        &[
            ("TypeId", "0x9a1e036d"),
            ("PayloadBytes", "211"),
            ("StageResource", stage),
            ("GameSeed[0]", seed0),
            ("GameSeed[1]", "2"),
            ("GameSeed[2]", "3"),
            ("GameSeed[3]", "4"),
        ],
    )
}

fn pillar() -> Row {
    test_row(
        "CoopEnemySpawnNetEvent",
        &[
            ("TypeId", "0x0caefc01"),
            ("PayloadBytes", "89"),
            ("SalmonId", "11"),
            ("SpawnX", "-23.493"),
            ("SpawnY", "0.000"),
            ("SpawnZ", "-57.173"),
        ],
    )
}

fn value<'a>(row: &'a Row, name: &str) -> Option<&'a str> {
    row.fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.value.as_str())
}

#[test]
fn captured_pillar_matches_the_unique_mid_tide_anchor_without_claiming_a_spawn() {
    let mut tracker = SpawnTracker::default();
    tracker.observe(&mut setting("Cop_Shakeship", "1"));
    let mut row = pillar();
    tracker.observe(&mut row);
    assert_eq!(value(&row, "BossRequestObservationOrder"), Some("1"));
    assert_eq!(value(&row, "StaticDirectionCandidate"), Some("0"));
    assert_eq!(value(&row, "StaticTideCandidate"), Some("1"));
    assert_eq!(value(&row, "StaticPointSource"), Some("PillarAnchor"));
    let distance: f32 = value(&row, "StaticPointDistance")
        .expect("distance")
        .parse()
        .unwrap();
    assert!(distance < 0.01);
}

#[test]
fn order_ignores_untrusted_rows_and_resets_only_for_a_new_setting() {
    let mut tracker = SpawnTracker::default();
    tracker.observe(&mut setting("Cop_Shakeship", "1"));
    let mut first = pillar();
    tracker.observe(&mut first);
    assert_eq!(value(&first, "BossRequestObservationOrder"), Some("1"));

    let mut untrusted = pillar();
    untrusted.status = "authentication_failed".into();
    tracker.observe(&mut untrusted);
    assert!(value(&untrusted, "BossRequestObservationOrder").is_none());

    tracker.observe(&mut setting("Cop_Shakeship", "1"));
    let mut second = pillar();
    tracker.observe(&mut second);
    assert_eq!(value(&second, "BossRequestObservationOrder"), Some("2"));

    tracker.observe(&mut setting("Cop_Shakeship", "5"));
    let mut new_game = pillar();
    tracker.observe(&mut new_game);
    assert_eq!(value(&new_game, "BossRequestObservationOrder"), Some("1"));
}

#[test]
fn unknown_stage_and_non_boss_events_do_not_gain_direction_candidates() {
    let mut tracker = SpawnTracker::default();
    tracker.observe(&mut setting("Cop_Unknown", "1"));
    let mut boss = pillar();
    tracker.observe(&mut boss);
    assert_eq!(value(&boss, "BossRequestObservationOrder"), Some("1"));
    assert!(value(&boss, "StaticDirectionCandidate").is_none());

    let mut non_boss = pillar();
    non_boss
        .fields
        .iter_mut()
        .find(|field| field.name == "SalmonId")
        .unwrap()
        .value = "0".into();
    tracker.observe(&mut non_boss);
    assert!(value(&non_boss, "BossRequestObservationOrder").is_none());

    let mut next_boss = pillar();
    tracker.observe(&mut next_boss);
    assert_eq!(value(&next_boss, "BossRequestObservationOrder"), Some("2"));
}

#[test]
fn imprecise_position_is_counted_but_has_no_static_direction() {
    let mut tracker = SpawnTracker::default();
    tracker.observe(&mut setting("Cop_Shakeship", "1"));
    let mut row = pillar();
    row.fields
        .iter_mut()
        .find(|field| field.name == "SpawnX")
        .unwrap()
        .value = "-23.400".into();
    tracker.observe(&mut row);
    assert_eq!(value(&row, "BossRequestObservationOrder"), Some("1"));
    assert!(value(&row, "StaticDirectionCandidate").is_none());
}
