//! Conditional JobType 1 equipment from a strictly bounded setting shape.
//!
//! Packet settings are generator inputs, not a replicated current held ID.

use crate::decode::Field;
use ocean_calc::{
    SeedWords,
    private_weapons::{PrivateMainRing, PrivateWeaponInput, predict_private_weapon_rows},
    specials::{PrivateSpecialInput, SpecialLookup, SpecialReorder, predict_private_specials},
    weapon_catalog::resolve_normal_pool,
};
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct EquipmentPreview {
    pub status: &'static str,
    pub source: &'static str,
    pub assumptions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// WAVE rows 0..4, then internal equipment indices 0..3.
    pub main_weapons: [[Option<u16>; 4]; 5],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EquipmentSetting {
    unlocked_season: i32,
    main_ring: Vec<i32>,
    scenario_app_version: u32,
    scenario_competition_type: u8,
    original_main_ring: Vec<i32>,
}

fn bits(payload: &[u8], start: usize, width: usize) -> Option<u64> {
    if width > 64 || start.checked_add(width)? > payload.len().checked_mul(8)? {
        return None;
    }
    Some((0..width).fold(0, |value, offset| {
        value | (u64::from((payload[(start + offset) / 8] >> ((start + offset) % 8)) & 1) << offset)
    }))
}

// Padding spans can be longer than a u64. Read each bit without truncating.
fn zero_bits(payload: &[u8], start: usize, end: usize) -> bool {
    start <= end
        && end <= payload.len().saturating_mul(8)
        && (start..end).all(|bit| payload[bit / 8] & (1 << (bit % 8)) == 0)
}

fn ring(payload: &[u8], count_start: usize) -> Option<Vec<i32>> {
    let count = bits(payload, count_start, 3)? as usize;
    if count > 4 || !zero_bits(payload, count_start + 3 + 16 * count, count_start + 67) {
        return None;
    }
    (0..count)
        .map(|index| bits(payload, count_start + 3 + 16 * index, 16).map(|raw| raw as i32 - 3))
        .collect()
}

impl EquipmentSetting {
    /// Supports the owned 11.3.0 211-byte, n=0 typed envelope only. Empty
    /// intermediate arrays keep the far-field offsets independently bounded.
    pub(crate) fn from_payload(payload: &[u8]) -> Option<Self> {
        if payload.len() != 211
            || payload[0] != 7
            || payload[7] != 0
            || u16::from_be_bytes([payload[1], payload[2]]) != 203
            || u16::from_be_bytes([payload[13], payload[14]]) != 194
            || payload[21..25] != 0x9a1e036du32.to_le_bytes()
            || !zero_bits(payload, 792, 1052)
            || !zero_bits(payload, 1395, 1559)
            || !zero_bits(payload, 1686, 1688)
        {
            return None;
        }
        Some(Self {
            unlocked_season: bits(payload, 715, 10)? as i32 - 1,
            main_ring: ring(payload, 725)?,
            scenario_app_version: bits(payload, 1559, 32)? as u32,
            scenario_competition_type: bits(payload, 1591, 1)? as u8,
            original_main_ring: ring(payload, 1592)?,
        })
    }

    pub(crate) fn fields(&self) -> Vec<Field> {
        let field = |name: &str, value: i64, start: usize, width: usize| Field {
            name: name.into(),
            value: value.to_string(),
            bits: Some([start, start + width]),
        };
        let mut fields = vec![
            field("UnlockedSeason", i64::from(self.unlocked_season), 715, 10),
            field("WeaponIdCount", self.main_ring.len() as i64, 725, 3),
            field(
                "ScenarioAppVersion",
                i64::from(self.scenario_app_version),
                1559,
                32,
            ),
            field(
                "ScenarioCompetitionType",
                i64::from(self.scenario_competition_type),
                1591,
                1,
            ),
            field(
                "ScenarioOriginalWeaponIdCount",
                self.original_main_ring.len() as i64,
                1592,
                3,
            ),
        ];
        for (prefix, start, ids) in [
            ("WeaponId", 728, &self.main_ring),
            ("ScenarioOriginalWeaponId", 1595, &self.original_main_ring),
        ] {
            for (index, &id) in ids.iter().enumerate() {
                fields.push(field(
                    &format!("{prefix}[{index}]"),
                    i64::from(id),
                    start + 16 * index,
                    16,
                ));
            }
        }
        fields
    }

    pub(crate) fn from_fields(fields: &[Field]) -> Option<Self> {
        let value = |name: &str| {
            fields
                .iter()
                .find(|field| field.name == name)?
                .value
                .parse::<i64>()
                .ok()
        };
        let ring = |prefix: &str, count_name: &str| {
            let count = value(count_name)?;
            if !(0..=4).contains(&count) {
                return None;
            }
            (0..count)
                .map(|index| {
                    let id = value(&format!("{prefix}[{index}]"))?;
                    (-3..=65532).contains(&id).then_some(id as i32)
                })
                .collect::<Option<Vec<_>>>()
        };
        let season = value("UnlockedSeason")?;
        let version = u32::try_from(value("ScenarioAppVersion")?).ok()?;
        let competition = u8::try_from(value("ScenarioCompetitionType")?).ok()?;
        if !(-1..=1022).contains(&season) || competition > 1 {
            return None;
        }
        Some(Self {
            unlocked_season: season as i32,
            main_ring: ring("WeaponId", "WeaponIdCount")?,
            scenario_app_version: version,
            scenario_competition_type: competition,
            original_main_ring: ring("ScenarioOriginalWeaponId", "ScenarioOriginalWeaponIdCount")?,
        })
    }
}

fn unavailable(reason: impl Into<String>) -> EquipmentPreview {
    EquipmentPreview {
        status: "unavailable",
        source: "OnlineCoopSetting",
        assumptions: vec![],
        reason: Some(reason.into()),
        main_weapons: [[None; 4]; 5],
    }
}

pub(crate) fn calculate(
    setting: &EquipmentSetting,
    job_type: u8,
    seed: [u32; 4],
) -> EquipmentPreview {
    if job_type != 1 {
        return unavailable("equipment generation is integrated only for JobType 1");
    }
    if setting.main_ring.len() != 4 {
        return unavailable("equipment generation requires four logical main-weapon slots");
    }
    let pool = resolve_normal_pool(setting.unlocked_season, 256, |row| {
        Ok::<_, ()>(Some(i32::from(row.candidate_id)))
    })
    .expect("the conditional stock lookup supplies every candidate");
    let special = match predict_private_specials(
        PrivateSpecialInput {
            seed: SeedWords::from_scene(seed),
            effective_g: setting.scenario_app_version,
            low_candidates: &[20006, 20012, 20010, 20009, 20014, 20013, 20007],
            prior_candidates: &[20006, 20012, 20010, 20009, 20014],
            v6_candidates: &[20013, 20007, 20017, 20018],
            // Remapping consumes no draws. Only the RNG state is used here;
            // this does not assert that the runtime reorder gate is inactive.
            reorder: SpecialReorder::Inactive,
        },
        SpecialLookup::Known,
    ) {
        Ok(prediction) => prediction,
        Err(_) => {
            return unavailable("conditional special selection could not produce the main RNG");
        },
    };
    let predict = |mode_allows_direct| {
        predict_private_weapon_rows(PrivateWeaponInput {
            rng_after_specials: special.rng.state,
            builder_ready_confirmed: true,
            scene_main_ring: PrivateMainRing {
                physical_ids: &setting.main_ring,
                start: 0,
                count: 4,
            },
            base_pool: &pool,
            alternate_pool: &pool,
            mode_allows_direct,
        })
    };
    let (Ok(direct), Ok(other)) = (predict(true), predict(false)) else {
        return unavailable("conditional main-weapon generation failed");
    };
    let mut main_weapons = [[None; 4]; 5];
    for (wave, row) in main_weapons.iter_mut().enumerate() {
        for (index, cell) in row.iter_mut().enumerate() {
            let id = direct.packed_rows[wave][index] as u16;
            *cell = (id == other.packed_rows[wave][index] as u16).then_some(id);
        }
    }
    EquipmentPreview {
        status: "conditional",
        source: "OnlineCoopSetting",
        assumptions: [
            "packet GameSeed, UnlockedSeason, and ordered main ring were adopted unchanged by the scene (unverified)",
            "effective G uses the packet ScenarioAppVersion branch (unverified scene/global gate)",
            "stock 11.3.0 special candidates and all candidate asset lookups succeed",
            "both runtime Type0 pools equal the stock pool at the packet season cap, with capacity 256",
            "the shared RNG and successful JobType 1 five-row builder are ready, with no later equipment override",
            "columns are internal equipment indices; only main IDs agreeing across both direct-mode branches are emitted",
        ].into_iter().map(str::to_owned).collect(),
        reason: None,
        main_weapons,
    }
}

#[cfg(test)]
#[path = "../test_support/unit/equipment.rs"]
mod tests;
