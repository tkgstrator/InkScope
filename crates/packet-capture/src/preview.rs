//! Conditional WAVE calculations from a confirmed OnlineCoopSetting row.
//!
//! A network GameSeed is only a candidate scene seed. This module does not
//! claim that it was adopted by the scene or that generated records were
//! applied without later overrides.

use crate::decode::Row;
use ocean_calc::{
    SeedWords,
    levels::{common_weights, queue_counts},
    records::{NormalWaveInput, predict_selections},
    schedule::{ScheduleBranch, ScheduleInput, nominal_schedule},
    stage_data::stage_spawns,
};
use serde::Serialize;
use std::collections::VecDeque;

const MAX_SEEN_SETTINGS: usize = 32;

#[derive(Clone, Serialize)]
pub struct WavePreview {
    pub status: &'static str,
    pub source: &'static str,
    pub assumptions: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub scenarios: Vec<PreviewScenario>,
}

#[derive(Clone, Serialize)]
pub struct PreviewScenario {
    pub is_big_run: bool,
    pub global_word_class: &'static str,
    pub waves: Vec<PreviewWave>,
}

#[derive(Clone, Serialize)]
pub struct PreviewWave {
    pub wave_index: u8,
    pub event_id: u8,
    pub tide_id: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nominal_rare_ticks: Option<Vec<u32>>,
}

#[derive(Clone, Eq, PartialEq)]
struct Setting {
    stage: String,
    job_type: u8,
    difficulty: Option<u32>,
    seed: [u32; 4],
}

#[derive(Default)]
pub struct WavePreviewer {
    seen: VecDeque<Setting>,
}

fn value<'a>(row: &'a Row, name: &str) -> Option<&'a str> {
    row.fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.value.as_str())
}

fn unavailable(reason: impl Into<String>) -> WavePreview {
    WavePreview {
        status: "unavailable",
        source: "OnlineCoopSetting.GameSeed",
        assumptions: vec![],
        reason: Some(reason.into()),
        scenarios: vec![],
    }
}

impl WavePreviewer {
    /// Returns a preview only for the first confirmed occurrence of each
    /// stage, job type, difficulty, and four-word GameSeed combination.
    pub fn observe(&mut self, row: &Row) -> Option<WavePreview> {
        if row.status != "authenticated"
            || row.protocol != Some(8)
            || row.kind != "OnlineCoopSetting"
            || row.layout.as_deref() != Some("confirmed_fields")
            || value(row, "TypeId") != Some("0x9a1e036d")
            || value(row, "PayloadBytes") != Some("211")
        {
            return None;
        }
        let stage = value(row, "StageResource").unwrap_or("").to_owned();
        let job_type = value(row, "JobType")?.parse().ok()?;
        let difficulty = value(row, "Difficulty").and_then(|v| v.parse().ok());
        let mut seed = [0u32; 4];
        for (index, word) in seed.iter_mut().enumerate() {
            *word = value(row, &format!("GameSeed[{index}]"))?.parse().ok()?;
        }
        let setting = Setting {
            stage,
            job_type,
            difficulty,
            seed,
        };
        if self.seen.contains(&setting) {
            return None;
        }
        if self.seen.len() == MAX_SEEN_SETTINGS {
            self.seen.pop_front();
        }
        self.seen.push_back(setting.clone());
        Some(calculate(&setting))
    }
}

fn calculate(setting: &Setting) -> WavePreview {
    if setting.stage.is_empty() {
        return unavailable("StageResource missing or invalid");
    }
    if !matches!(setting.job_type, 0 | 1) {
        return unavailable(format!(
            "JobType {} has no integrated initial WAVE preview",
            setting.job_type
        ));
    }
    let Some(difficulty) = setting.difficulty.filter(|value| *value <= 1665) else {
        return unavailable("DifficultyWire is absent or outside the supported range");
    };
    let Some(stage) = stage_spawns(&setting.stage) else {
        return unavailable(format!("no 11.3.0 stage data for {}", setting.stage));
    };
    let seed = SeedWords::from_scene(setting.seed);
    let common = common_weights(difficulty);
    let mut scenarios = Vec::with_capacity(4);
    for is_big_run in [false, true] {
        for (global_word, global_word_class) in [
            (0u32, "global_word >> 8 <= 256"),
            (0x10100u32, "global_word >> 8 > 256"),
        ] {
            let input = NormalWaveInput {
                event_weights: common.event,
                tide_weights: stage.water_override.unwrap_or(common.water),
                mode_one: is_big_run,
                ban_event_mask: stage.ban_event_mask,
                global_word,
            };
            let outcome = match predict_selections(seed, None, [input; 5], setting.job_type, None) {
                Ok(value) => value,
                Err(error) => return unavailable(format!("WAVE generation failed: {error:?}")),
            };
            let waves = outcome
                .selections
                .into_iter()
                .map(|selection| {
                    let nominal_rare_ticks = if matches!(selection.event_id, 0 | 5 | 6) {
                        let counts = queue_counts(difficulty, selection.wave_index.min(2))
                            .expect("source Round is clamped to 0..=2");
                        nominal_schedule(ScheduleInput {
                            seed,
                            event_type: selection.event_id,
                            wave_change_num: counts.wave_change_num,
                            rare_appear_num: counts.rare_appear_num,
                            assumed_branch: ScheduleBranch::Normal,
                        })
                        .ok()
                        .map(|entries| entries.into_iter().map(|entry| entry.tick).collect())
                    } else {
                        None
                    };
                    PreviewWave {
                        wave_index: selection.wave_index,
                        event_id: selection.event_id,
                        tide_id: selection.tide_id,
                        nominal_rare_ticks,
                    }
                })
                .collect();
            scenarios.push(PreviewScenario {
                is_big_run,
                global_word_class,
                waves,
            });
        }
    }
    let mut assumptions = vec![
        "GameSeed was adopted unchanged as the scene seed (unverified)",
        "setting Difficulty equals effective difficulty (unverified)",
        "stock 11.3.0 event, tide, stage, and level weights apply",
        "initial special tide is inactive and nominal queue uses the normal branch",
        "generated values only; later application and actual spawns are unknown",
    ];
    if setting.job_type == 1 {
        assumptions.push("raw MatchMode is 11 (unverified)");
    } else {
        assumptions.push("JobType 0 participant average and special score are unknown");
    }
    if setting.seed == [0; 4] {
        assumptions.push("all-zero scene seed is normalized to [1, 0, 0, 0]");
    }
    WavePreview {
        status: "conditional",
        source: "OnlineCoopSetting.GameSeed",
        assumptions,
        reason: None,
        scenarios,
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn first_confirmed_setting_has_four_conditional_scenarios_and_deduplicates() {
        let mut previewer = WavePreviewer::default();
        let row = setting_row();
        let preview = previewer.observe(&row).expect("first setting");
        assert_eq!(preview.status, "conditional");
        assert_eq!(preview.scenarios.len(), 4);
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
        assert_eq!(json["scenarios"].as_array().unwrap().len(), 4);
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
}
