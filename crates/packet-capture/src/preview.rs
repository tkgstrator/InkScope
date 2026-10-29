//! Conditional WAVE calculations from a confirmed OnlineCoopSetting row.
//!
//! A network GameSeed is only a candidate scene seed. This module does not
//! claim that it was adopted by the scene or that generated records were
//! applied without later overrides.

use crate::decode::Row;
use crate::equipment::{self, EquipmentPreview, EquipmentSetting};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub equipment: Option<EquipmentPreview>,
}

#[derive(Clone, Serialize)]
pub struct PreviewScenario {
    pub is_big_run: bool,
    pub global_word_class: &'static str,
    pub waves: Vec<PreviewWave>,
}

#[derive(Clone, Serialize, PartialEq, Eq)]
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
    equipment: Option<EquipmentSetting>,
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
        equipment: None,
    }
}

// The stock 11.3.0 CoopMapInfo stage class is keyed by exact StageResource.
// Keep both branches for an unclassified resource rather than guessing.
fn big_run_modes(stage: &str) -> &'static [bool] {
    match stage {
        "Cop_Shakeup" | "Cop_Shakespiral" | "Cop_Shakelift" | "Cop_Shakeship" | "Cop_Shakedent"
        | "Cop_Shakehighway" | "Cop_Shakerail" => &[false],
        "Cop_Carousel" | "Cop_Upland" | "Cop_Temple" | "Cop_Factory" | "Cop_Section"
        | "Cop_District00" | "Cop_Manbou00" => &[true],
        _ => &[false, true],
    }
}

impl WavePreviewer {
    /// Returns a preview only for the first confirmed occurrence of each
    /// stage, job type, difficulty, GameSeed, and equipment input combination.
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
            equipment: EquipmentSetting::from_fields(&row.fields),
        };
        if self.seen.contains(&setting) {
            return None;
        }
        if self.seen.len() == MAX_SEEN_SETTINGS {
            self.seen.pop_front();
        }
        self.seen.push_back(setting.clone());
        let mut preview = calculate(&setting);
        preview.equipment = setting
            .equipment
            .as_ref()
            .map(|input| equipment::calculate(input, setting.job_type, setting.seed));
        Some(preview)
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
    let mut scenarios = Vec::with_capacity(big_run_modes(&setting.stage).len() * 2);
    for &is_big_run in big_run_modes(&setting.stage) {
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
    if big_run_modes(&setting.stage).len() == 1 {
        assumptions.push("IsBigRun follows the stock 11.3.0 CoopMapInfo stage class");
    }
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
        equipment: None,
    }
}

#[cfg(test)]
#[path = "../test_support/unit/preview.rs"]
mod tests;
