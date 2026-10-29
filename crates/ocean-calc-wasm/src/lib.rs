//! Deterministic bounded JSON facade; no packet/key/host-RNG dependencies.

use ocean_calc::{
    SeedWords,
    applied_preview::JobOneSchedulerState,
    applied_wave::{JobOneApplyState, apply_job_one_direct_writes},
    first_rare::FirstSpotRoute,
    levels::{common_weights, queue_counts},
    preview::QueueCountSource,
    private_weapons::{PrivateMainRing, PrivateWeaponInput, predict_private_weapon_rows},
    records::{NormalWaveInput, WaveSelection, predict_selections},
    schedule::{ScheduleBranch, ScheduleInput, nominal_schedule},
    selected_wave_rare::{
        AppliedValueEvidence, ConditionBasis, FirstRareRuntimeEvidence, FirstRareStatus,
        SelectedWaveFirstRareInput, predict_selected_job_one_first_rare,
    },
    specials::{PrivateSpecialInput, SpecialLookup, SpecialReorder, predict_private_specials},
    species::{PeriodicGoldieInput, UNSPECIFIED_BOSS_ID},
    stage_data::{StageSpawns, stage_spawns},
    stage_wave::{DifficultySource, StageWaveInput, WaveWeightSource},
    weapon_catalog::resolve_normal_pool,
};
use serde::{Deserialize, Serialize};

const MAX_INPUT_BYTES: usize = 8192;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    version: u8,
    stage_resource: String,
    job_type: u8,
    difficulty: u32,
    seed: [u32; 4],
    unlocked_season: i16,
    main_weapon_ids: Vec<i32>,
    scenario_app_version: u32,
    scenario_competition_type: u8,
    original_main_weapon_ids: Vec<i32>,
    player_count: Option<u8>,
}

impl Input {
    fn valid(&self) -> bool {
        let stage = self.stage_resource.as_bytes();
        let ring = |ids: &[i32]| ids.len() <= 4 && ids.iter().all(|id| (-3..=65532).contains(id));
        !stage.is_empty()
            && stage.len() <= 64
            && stage[0].is_ascii_alphabetic()
            && stage
                .iter()
                .all(|c| c.is_ascii_alphanumeric() || *c == b'_')
            && self.difficulty <= 1665
            && (-1..=1022).contains(&self.unlocked_season)
            && ring(&self.main_weapon_ids)
            && ring(&self.original_main_weapon_ids)
            && self.scenario_competition_type <= 1
            && self.player_count.is_none_or(|n| (1..=4).contains(&n))
    }
}

#[derive(Serialize)]
struct Response {
    version: u8,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    wave_preview: Option<WavePreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Error>,
}
#[derive(Serialize)]
struct Error {
    code: &'static str,
    message: &'static str,
}
#[derive(Serialize)]
struct WavePreview {
    status: &'static str,
    source: &'static str,
    assumptions: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    scenarios: Vec<Scenario>,
    equipment: Equipment,
}
#[derive(Serialize)]
struct Scenario {
    is_big_run: bool,
    global_word_class: &'static str,
    waves: Vec<Wave>,
}
#[derive(Serialize)]
struct Wave {
    wave_index: u8,
    event_id: u8,
    tide_id: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    nominal_rare_ticks: Option<Vec<u32>>,
    first_boss: FirstBoss,
}
#[derive(Serialize)]
struct FirstBoss {
    status: &'static str,
    basis: &'static str,
    species_id: Option<u8>,
    nominal_tick: Option<u32>,
    conditions: Vec<&'static str>,
    reason: Option<String>,
}
#[derive(Serialize)]
struct Equipment {
    status: &'static str,
    source: &'static str,
    assumptions: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    main_weapons: [[Option<u16>; 4]; 5],
    special_weapons: [Option<u16>; 4],
}

fn error(code: &'static str, message: &'static str) -> Response {
    Response {
        version: 1,
        status: "error",
        wave_preview: None,
        error: Some(Error { code, message }),
    }
}

/// Shared native/WASM boundary. Errors never echo untrusted input values.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn calculate_obs_json(input: &str) -> String {
    let response = if input.len() > MAX_INPUT_BYTES {
        error("input_too_large", "Input exceeds the 8192-byte UTF-8 limit")
    } else {
        match serde_json::from_str::<Input>(input) {
            Err(_) => error("invalid_input", "Input is not a supported JSON object"),
            Ok(value) if value.version != 1 => {
                error("unsupported_version", "Only input version 1 is supported")
            },
            Ok(value) if !value.valid() => error(
                "invalid_input",
                "An input field is outside the supported bounds",
            ),
            Ok(value) => Response {
                version: 1,
                status: "ok",
                wave_preview: Some(calculate(&value)),
                error: None,
            },
        }
    };
    serde_json::to_string(&response).unwrap_or_else(|_| {
        r#"{"version":1,"status":"error","error":{"code":"serialization_failed","message":"Output serialization failed"}}"#.into()
    })
}

fn equipment(input: &Input) -> Equipment {
    let unavailable = |reason| Equipment {
        status: "unavailable",
        source: "OnlineCoopSetting",
        assumptions: vec![],
        reason: Some(reason),
        main_weapons: [[None; 4]; 5],
        special_weapons: [None; 4],
    };
    if input.job_type != 1 {
        return unavailable("equipment generation is integrated only for JobType 1");
    }
    if input.main_weapon_ids.len() != 4 {
        return unavailable("equipment generation requires four logical main-weapon slots");
    }
    let Ok(pool) = resolve_normal_pool(i32::from(input.unlocked_season), 256, |row| {
        Ok::<_, ()>(Some(i32::from(row.candidate_id)))
    }) else {
        return unavailable("conditional stock weapon lookup failed");
    };
    let Ok(special) = predict_private_specials(
        PrivateSpecialInput {
            seed: SeedWords::from_scene(input.seed),
            effective_g: input.scenario_app_version,
            low_candidates: &[20006, 20012, 20010, 20009, 20014, 20013, 20007],
            prior_candidates: &[20006, 20012, 20010, 20009, 20014],
            v6_candidates: &[20013, 20007, 20017, 20018],
            reorder: SpecialReorder::Inactive,
        },
        SpecialLookup::Known,
    ) else {
        return unavailable("conditional special selection could not produce the main RNG");
    };
    let predict = |mode_allows_direct| {
        predict_private_weapon_rows(PrivateWeaponInput {
            rng_after_specials: special.rng.state,
            builder_ready_confirmed: true,
            scene_main_ring: PrivateMainRing {
                physical_ids: &input.main_weapon_ids,
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
    let main_weapons = std::array::from_fn(|wave| {
        std::array::from_fn(|index| {
            let id = direct.packed_rows[wave][index] as u16;
            (id == other.packed_rows[wave][index] as u16).then_some(id)
        })
    });
    Equipment {
        status: "conditional",
        source: "OnlineCoopSetting",
        reason: None,
        main_weapons,
        special_weapons: special.final_specials.map(Some),
        assumptions: vec![
            "packet GameSeed, UnlockedSeason, and ordered main ring were adopted unchanged by the scene (unverified)",
            "effective G uses the packet ScenarioAppVersion branch (unverified scene/global gate)",
            "stock 11.3.0 special candidates and all candidate asset lookups succeed",
            "both runtime Type0 pools equal the stock pool at the packet season cap, with capacity 256",
            "the shared RNG and successful JobType 1 five-row builder are ready, with no later equipment override",
            "columns are internal equipment indices; only main IDs agreeing across both direct-mode branches are emitted",
            "each main ID agrees with both unresolved runtime mode-gate branches",
            "special remapping consumes no draws; only the post-special RNG state is used",
            "five generated rows are forecasts, not observed held equipment",
            "special_weapons assumes the runtime special-reorder gate is inactive and no later special override occurs",
            "special_weapons columns are internal equipment indices; room display additionally requires the normal adopted-scene raw-index mapping",
        ],
    }
}

fn big_run_modes(stage: &str) -> &'static [bool] {
    match stage {
        "Cop_Shakeup" | "Cop_Shakespiral" | "Cop_Shakelift" | "Cop_Shakeship" | "Cop_Shakedent"
        | "Cop_Shakehighway" | "Cop_Shakerail" => &[false],
        "Cop_Carousel" | "Cop_Upland" | "Cop_Temple" | "Cop_Factory" | "Cop_Section"
        | "Cop_District00" | "Cop_Manbou00" => &[true],
        _ => &[false, true],
    }
}

fn calculate(input: &Input) -> WavePreview {
    let mut result = WavePreview {
        status: "unavailable",
        source: "OnlineCoopSetting.GameSeed",
        assumptions: vec![],
        reason: None,
        scenarios: vec![],
        equipment: equipment(input),
    };
    if !matches!(input.job_type, 0 | 1) {
        result.reason = Some("JobType has no integrated initial WAVE preview".into());
        return result;
    }
    let Some(stage) = stage_spawns(&input.stage_resource) else {
        result.reason = Some("No owned 11.3.0 stage data for this resource".into());
        return result;
    };
    let seed = SeedWords::from_scene(input.seed);
    let common = common_weights(input.difficulty);
    for &is_big_run in big_run_modes(&input.stage_resource) {
        for (global_word, global_word_class) in [
            (0, "global_word >> 8 <= 256"),
            (0x10100, "global_word >> 8 > 256"),
        ] {
            let wave_input = NormalWaveInput {
                event_weights: common.event,
                tide_weights: stage.water_override.unwrap_or(common.water),
                mode_one: is_big_run,
                ban_event_mask: stage.ban_event_mask,
                global_word,
            };
            let Ok(outcome) = predict_selections(seed, None, [wave_input; 5], input.job_type, None)
            else {
                result.reason = Some("Conditional WAVE generation failed".into());
                result.scenarios.clear();
                return result;
            };
            let waves = outcome
                .selections
                .into_iter()
                .map(|selection| {
                    let nominal_rare_ticks = if matches!(selection.event_id, 0 | 5 | 6) {
                        queue_counts(input.difficulty, selection.wave_index.min(2))
                            .ok()
                            .and_then(|counts| {
                                nominal_schedule(ScheduleInput {
                                    seed,
                                    event_type: selection.event_id,
                                    wave_change_num: counts.wave_change_num,
                                    rare_appear_num: counts.rare_appear_num,
                                    assumed_branch: ScheduleBranch::Normal,
                                })
                                .ok()
                            })
                            .map(|entries| entries.into_iter().map(|entry| entry.tick).collect())
                    } else {
                        None
                    };
                    Wave {
                        wave_index: selection.wave_index,
                        event_id: selection.event_id,
                        tide_id: selection.tide_id,
                        nominal_rare_ticks,
                        first_boss: first_boss(input, stage, is_big_run, global_word, selection),
                    }
                })
                .collect();
            result.scenarios.push(Scenario {
                is_big_run,
                global_word_class,
                waves,
            });
        }
    }
    result.status = "conditional";
    result.assumptions = vec![
        "GameSeed was adopted unchanged as the scene seed (unverified)",
        "setting Difficulty equals effective difficulty (unverified)",
        "stock 11.3.0 event, tide, stage, and level weights apply",
        "initial special tide is inactive and nominal queue uses the normal branch",
        "generated values only; later application and actual spawns are unknown",
    ];
    if big_run_modes(&input.stage_resource).len() == 1 {
        result
            .assumptions
            .push("IsBigRun follows the stock 11.3.0 CoopMapInfo stage class");
    }
    if input.job_type == 1 {
        result.assumptions.push("raw MatchMode is 11 (unverified)");
    } else {
        result
            .assumptions
            .push("JobType 0 participant average and special score are unknown");
    }
    if input.seed == [0; 4] {
        result
            .assumptions
            .push("all-zero scene seed is normalized to [1, 0, 0, 0]");
    }
    result
}

fn first_boss(
    input: &Input,
    stage: &StageSpawns,
    is_big_run: bool,
    global_word: u32,
    generated: WaveSelection,
) -> FirstBoss {
    let mut result = FirstBoss {
        status: "unavailable",
        basis: "assumed_idealized",
        species_id: None,
        nominal_tick: None,
        conditions: vec![],
        reason: None,
    };
    let fail = |mut result: FirstBoss, reason: &str| {
        result.reason = Some(reason.into());
        result
    };
    if input.job_type != 1 {
        return fail(result, "First species composition supports only JobType 1");
    }
    let Some(players) = input.player_count else {
        return fail(result, "Active player count is unknown");
    };
    let state = JobOneApplyState {
        previous_applied_tide: 0,
        active_player_count: i32::from(players),
        mode_flag_591: false,
        special_flag_624: false,
        special_target_index_620: 0,
        runtime_mode_forces_mid_tide: false,
    };
    let Ok(applied) = apply_job_one_direct_writes(generated, state) else {
        return fail(result, "Direct WAVE application is unsupported");
    };
    // No Fog-cycle value exists in the input contract. Preserve unknown.
    if applied.event_id == 5 {
        return fail(result, "Fog periodic cycle is unknown");
    }
    result.conditions = vec![
        "packet scene seed and stock stage/level inputs apply unchanged",
        "provided player_count equals the active count at WAVE application",
        "normal JobType 1 MatchMode 11; special flags and runtime mode overrides are inactive",
        "direct event and tide survive all later overrides",
        "normal Rare queue branch; first scheduler call processes tick zero at elapsed tick zero",
        "zero active and reserved Rare population; no earlier spawn or shared-RNG draw",
        "periodic global cycle flag is false and accepted Rare counter starts at zero",
        "normal stage registration and initial candidate callbacks; no prequeue side effects",
        "species draw only; spot callback, actor registration, and later items are not predicted",
    ];
    let evidence = FirstRareRuntimeEvidence {
        scene_seed: ConditionBasis::AssumedIdealized,
        applied_values: AppliedValueEvidence::AssumedNoLaterOverride,
        zero_rare_population: ConditionBasis::AssumedIdealized,
        no_prequeue_effects: ConditionBasis::AssumedIdealized,
        normal_stage_registration: ConditionBasis::AssumedIdealized,
        unfiltered_actor_group: ConditionBasis::AssumedIdealized,
    };
    let report = predict_selected_job_one_first_rare(
        SelectedWaveFirstRareInput {
            stage_wave: StageWaveInput {
                seed: SeedWords::from_scene(input.seed),
                stage,
                difficulty: DifficultySource::JobOne {
                    match_mode: 11,
                    scene_difficulty: input.difficulty as i32,
                },
                is_big_run,
                global_word,
                cutoff: None,
                special: None,
                weights: WaveWeightSource::RomfsDerived,
            },
            counts: QueueCountSource::RomfsClampedRoundByWave,
            branches: [ScheduleBranch::Normal; 5],
            states: [state; 5],
            schedulers: [JobOneSchedulerState {
                runtime_mode_bypasses_rare_queue: false,
            }; 5],
            wave_index: generated.wave_index,
            first_scheduler_elapsed_tick: 0,
            periodic: PeriodicGoldieInput {
                event: Some(applied.event_id),
                global_cycle_flag: false,
                accepted_rare_counter: 0,
                fog_cycle: None,
                event_none_cycle: None,
            },
            runtime_evidence: evidence,
        },
        |_| FirstSpotRoute::Unknown,
    );
    match report {
        Ok(report) => match report.status {
            FirstRareStatus::Normal { rare, .. } if rare.species_id != UNSPECIFIED_BOSS_ID => {
                result.status = "conditional";
                result.species_id = Some(rare.species_id);
                result.nominal_tick = Some(rare.nominal_first_entry.tick);
            },
            FirstRareStatus::ForcedGoldie {
                nominal_first_entry,
                ..
            } => {
                result.status = "conditional";
                result.species_id = Some(15);
                result.nominal_tick = Some(nominal_first_entry.tick);
            },
            _ => {
                result.reason =
                    Some("Applied event/queue has no supported first species prediction".into())
            },
        },
        Err(_) => {
            result.reason =
                Some("Required first-species stage/runtime condition is unresolved".into())
        },
    }
    result
}
