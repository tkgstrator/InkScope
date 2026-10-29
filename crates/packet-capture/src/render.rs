use crate::Result;
use crate::decode::Row;
use chrono::{DateTime, SecondsFormat};
use std::io::{self, Write};

fn display_value(name: &str, value: &str) -> String {
    if name == "StageId" && value.is_empty() {
        return "\"\"".into();
    }
    if name.starts_with("GameSeed[")
        && name.ends_with(']')
        && let Ok(word) = value.parse::<u32>()
    {
        return format!("0x{word:08x}");
    }
    value.into()
}

pub fn print_row(row: &Row, jsonl: bool, details: bool) -> Result<()> {
    let mut out = io::stdout().lock();
    if jsonl {
        serde_json::to_writer(&mut out, row).map_err(|e| e.to_string())?;
        writeln!(out).map_err(|e| e.to_string())?;
        return Ok(());
    }
    let seconds = (row.timestamp_ns / 1_000_000_000) as i64;
    let nanos = (row.timestamp_ns % 1_000_000_000) as u32;
    let timestamp = DateTime::from_timestamp(seconds, nanos)
        .ok_or("PCAP timestamp outside supported calendar range")?
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let mut highlights = row
        .fields
        .iter()
        .filter(|f| {
            if row.protocol == Some(5) && f.name == "GroupId" {
                return true;
            }
            if row.kind == "RTTProtocol" && f.name == "Type" {
                return true;
            }
            if f.name == "LifeNumber" {
                return f.value != "0";
            }
            matches!(
                f.name.as_str(),
                "StageResource"
                    | "StageId"
                    | "StageName"
                    | "JobType"
                    | "Difficulty"
                    | "SalmonId"
                    | "SalmonType"
                    | "BossRequestObservationOrder"
                    | "SpawnX"
                    | "SpawnY"
                    | "SpawnZ"
                    | "Quota"
                    | "Sequence"
                    | "CaptureResponseDelayMs"
                    | "Reason"
                    | "Detail"
                    | "ValuePlusOne"
                    | "GameFrame"
            )
        })
        .collect::<Vec<_>>();
    highlights.sort_by_key(|f| match f.name.as_str() {
        "GroupId" if row.protocol == Some(5) => 0,
        "Type" if row.kind == "RTTProtocol" => 0,
        "CaptureResponseDelayMs" if row.kind == "RTTProtocol" => 1,
        "GameFrame" => 0,
        "LifeNumber" => 1,
        _ => 2,
    });
    let online_setting_normal = row.kind == "OnlineCoopSetting" && !details;
    if online_setting_normal {
        highlights.clear();
    }
    let highlights = highlights
        .into_iter()
        .take(if row.kind == "CoopEnemySpawnNetEvent" {
            7
        } else {
            6
        })
        .map(|f| format!("{}={}", f.name, display_value(&f.name, &f.value)))
        .collect::<Vec<_>>()
        .join(" ");
    let mut summary = row.kind.clone();
    if !highlights.is_empty() {
        summary.push(' ');
        summary.push_str(&highlights);
    }
    if row.status != "authenticated" {
        summary.push_str(" status=");
        summary.push_str(&row.status);
    }
    let direction = row
        .direction
        .as_deref()
        .map(|value| format!(" {value}"))
        .unwrap_or_default();
    writeln!(
        out,
        "#{:<7} {}{} {}",
        row.record, timestamp, direction, summary
    )
    .map_err(|e| e.to_string())?;
    if row.kind == "CoopEnemySpawnNetEvent" {
        let candidate = |name| {
            row.fields
                .iter()
                .find(|field| field.name == name)
                .map(|field| field.value.as_str())
        };
        if let (Some(direction), Some(tide), Some(source), Some(distance)) = (
            candidate("StaticDirectionCandidate"),
            candidate("StaticTideCandidate"),
            candidate("StaticPointSource"),
            candidate("StaticPointDistance"),
        ) {
            writeln!(
                out,
                "    static position candidate: direction-group={direction} tide={tide} source={source} distance={distance} (geometry only)"
            )
            .map_err(|e| e.to_string())?;
        }
    }
    if online_setting_normal {
        let mut fields = row
            .fields
            .iter()
            .filter(|field| {
                !matches!(
                    field.name.as_str(),
                    "HeaderPacketId"
                        | "HeaderCompressedFlag"
                        | "PayloadBytes"
                        | "TypeId"
                        | "DifficultyWire"
                        | "UnlockedSeason"
                        | "WeaponIdCount"
                        | "WeaponId[0]"
                        | "WeaponId[1]"
                        | "WeaponId[2]"
                        | "WeaponId[3]"
                        | "ScenarioAppVersion"
                        | "ScenarioCompetitionType"
                        | "ScenarioOriginalWeaponIdCount"
                        | "ScenarioOriginalWeaponId[0]"
                        | "ScenarioOriginalWeaponId[1]"
                        | "ScenarioOriginalWeaponId[2]"
                        | "ScenarioOriginalWeaponId[3]"
                ) && !(field.name == "LifeNumber" && field.value == "0")
            })
            .collect::<Vec<_>>();
        fields.sort_by_key(|field| usize::from(field.name != "GameFrame"));
        for field in fields {
            writeln!(
                out,
                "    {}={}",
                field.name,
                display_value(&field.name, &field.value)
            )
            .map_err(|e| e.to_string())?;
        }
    }
    if let Some(preview) = &row.wave_preview {
        writeln!(
            out,
            "    WAVE preview: {} from {}",
            preview.status, preview.source
        )
        .map_err(|e| e.to_string())?;
        if let Some(reason) = &preview.reason {
            writeln!(out, "      {reason}").map_err(|e| e.to_string())?;
        }
        let mut merged_modes_shown = [false; 2];
        for scenario in &preview.scenarios {
            let mode_index = usize::from(scenario.is_big_run);
            let same_in_mode = preview
                .scenarios
                .iter()
                .filter(|other| other.is_big_run == scenario.is_big_run)
                .all(|other| other.waves == scenario.waves);
            if same_in_mode && merged_modes_shown[mode_index] {
                continue;
            }
            merged_modes_shown[mode_index] = same_in_mode;
            let waves = scenario
                .waves
                .iter()
                .map(|wave| {
                    format!(
                        "W{}=event{}/tide{}",
                        wave.wave_index, wave.event_id, wave.tide_id
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            let condition = if same_in_mode {
                String::new()
            } else {
                format!(" {}", scenario.global_word_class)
            };
            writeln!(
                out,
                "      IsBigRun={}{condition}: {waves}",
                u8::from(scenario.is_big_run)
            )
            .map_err(|e| e.to_string())?;
            if details {
                for wave in &scenario.waves {
                    if let Some(ticks) = &wave.nominal_rare_ticks {
                        writeln!(
                            out,
                            "        W{} nominal Rare ticks={ticks:?}",
                            wave.wave_index
                        )
                        .map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        for assumption in &preview.assumptions {
            writeln!(out, "      assumption: {assumption}").map_err(|e| e.to_string())?;
        }
    }
    if details {
        if let Some(index) = row.chunk {
            writeln!(
                out,
                "    chunk={index} protocol={:?} port={:?}",
                row.protocol, row.port
            )
            .map_err(|e| e.to_string())?;
        }
        for value in &row.fields {
            let range = value
                .bits
                .map(|bits| format!(" bits {}..{}", bits[0], bits[1]))
                .unwrap_or_default();
            writeln!(
                out,
                "    {}{}: {}",
                value.name,
                range,
                display_value(&value.name, &value.value)
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(raw) = &row.authenticated_payload_hex {
            writeln!(out, "    authenticated payload hex: {raw}").map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
