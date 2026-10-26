use crate::Result;
use crate::decode::Row;
use chrono::{DateTime, SecondsFormat};
use std::io::{self, Write};

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
            if f.name == "LifeNumber" {
                return f.value != "0";
            }
            matches!(
                f.name.as_str(),
                "StageResource"
                    | "StageName"
                    | "JobType"
                    | "Difficulty"
                    | "SalmonId"
                    | "SalmonType"
                    | "SpawnX"
                    | "SpawnY"
                    | "SpawnZ"
                    | "Quota"
                    | "Sequence"
                    | "HazardCandidatePercent"
                    | "Reason"
                    | "Detail"
                    | "ValuePlusOne"
                    | "GameFrame"
            )
        })
        .collect::<Vec<_>>();
    highlights.sort_by_key(|f| match f.name.as_str() {
        "GameFrame" => 0,
        "LifeNumber" => 1,
        _ => 2,
    });
    let highlights = highlights
        .into_iter()
        .take(6)
        .map(|f| format!("{}={}", f.name, f.value))
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
        for scenario in &preview.scenarios {
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
            writeln!(
                out,
                "      IsBigRun={} {}: {waves}",
                u8::from(scenario.is_big_run),
                scenario.global_word_class
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
            writeln!(out, "    {}{}: {}", value.name, range, value.value)
                .map_err(|e| e.to_string())?;
        }
        if let Some(raw) = &row.authenticated_payload_hex {
            writeln!(out, "    authenticated payload hex: {raw}").map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
