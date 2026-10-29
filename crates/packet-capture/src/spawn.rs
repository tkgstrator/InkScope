//! Observed authenticated boss requests and conditional static point matches.
//!
//! Network notifications are requests, not proof that an actor was created.
//! Stage coordinates are raw 11.3.0 Banc points, not live enemy positions.

use crate::decode::{Field, Row};
use ocean_calc::stage_data::stage_spawns;

#[derive(PartialEq, Eq)]
struct Setting {
    stage: String,
    seed: [u32; 4],
}

#[derive(Default)]
pub struct SpawnTracker {
    setting: Option<Setting>,
    boss_requests: u64,
}

#[derive(Clone, Copy)]
struct PointMatch {
    distance: f32,
    key: usize,
    source: &'static str,
}

fn value<'a>(row: &'a Row, name: &str) -> Option<&'a str> {
    row.fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.value.as_str())
}

fn derived(row: &mut Row, name: &str, value: impl ToString) {
    row.fields.push(Field {
        name: name.into(),
        value: value.to_string(),
        bits: None,
    });
}

/// A unique geometric match is evidence for a static group candidate only.
fn static_point_match(stage_name: &str, salmon_id: u8, xyz: [f32; 3]) -> Option<PointMatch> {
    let stage = stage_spawns(stage_name)?;
    let mut best: Option<PointMatch> = None;
    let mut second: Option<PointMatch> = None;
    let mut consider = |key: usize, point: [f32; 3], source: &'static str| {
        let distance = point
            .iter()
            .zip(xyz)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>()
            .sqrt();
        if !distance.is_finite() {
            return;
        }
        let candidate = PointMatch {
            distance,
            key,
            source,
        };
        if best.is_none_or(|current| distance < current.distance) {
            second = best;
            best = Some(candidate);
        } else if second.is_none_or(|current| distance < current.distance) {
            second = Some(candidate);
        }
    };
    if salmon_id == 11 {
        for (key, anchors) in stage.pillar_anchors.iter().enumerate() {
            for anchor in *anchors {
                consider(key, anchor.translate, "PillarAnchor");
            }
        }
    } else {
        for (key, actors) in stage.groups.iter().enumerate() {
            for actor in *actors {
                consider(key, actor.translate, "GenericBase");
            }
        }
    }
    best.filter(|candidate| {
        candidate.distance <= 0.02 && second.is_none_or(|next| next.distance > 1.0)
    })
}

impl SpawnTracker {
    pub fn observe(&mut self, row: &mut Row) {
        if row.status != "authenticated"
            || row.protocol != Some(8)
            || row.layout.as_deref() != Some("confirmed_fields")
        {
            return;
        }
        if row.kind == "OnlineCoopSetting"
            && value(row, "TypeId") == Some("0x9a1e036d")
            && value(row, "PayloadBytes") == Some("211")
        {
            let Some(stage) = value(row, "StageResource") else {
                return;
            };
            let mut seed = [0u32; 4];
            for (index, word) in seed.iter_mut().enumerate() {
                let Some(parsed) =
                    value(row, &format!("GameSeed[{index}]")).and_then(|v| v.parse().ok())
                else {
                    return;
                };
                *word = parsed;
            }
            let setting = Setting {
                stage: stage.into(),
                seed,
            };
            if self.setting.as_ref() != Some(&setting) {
                self.boss_requests = 0;
                self.setting = Some(setting);
            }
            return;
        }
        if row.kind != "CoopEnemySpawnNetEvent"
            || value(row, "TypeId") != Some("0x0caefc01")
            || value(row, "PayloadBytes") != Some("89")
        {
            return;
        }
        let Some(salmon_id) = value(row, "SalmonId").and_then(|v| v.parse::<u8>().ok()) else {
            return;
        };
        if !(4..=14).contains(&salmon_id) {
            return;
        }
        self.boss_requests = self.boss_requests.saturating_add(1);
        derived(row, "BossRequestObservationOrder", self.boss_requests);

        let xyz = ["SpawnX", "SpawnY", "SpawnZ"]
            .map(|name| value(row, name).and_then(|value| value.parse::<f32>().ok()));
        let [Some(x), Some(y), Some(z)] = xyz else {
            return;
        };
        let Some(stage) = self.setting.as_ref() else {
            return;
        };
        let Some(candidate) = static_point_match(&stage.stage, salmon_id, [x, y, z]) else {
            return;
        };
        derived(row, "StaticDirectionCandidate", candidate.key / 3);
        derived(row, "StaticTideCandidate", candidate.key % 3);
        derived(row, "StaticPointSource", candidate.source);
        derived(
            row,
            "StaticPointDistance",
            format!("{:.3}", candidate.distance),
        );
    }
}

#[cfg(test)]
#[path = "../test_support/unit/spawn.rs"]
mod tests;
