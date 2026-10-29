use ocean_calc_wasm::calculate_obs_json;
use serde_json::{Value, json};

fn input() -> Value {
    json!({"version":1,"stage_resource":"Cop_Shakehighway","job_type":1,"difficulty":1665,
        "seed":[1,2,3,4],"unlocked_season":4,"main_weapon_ids":[-1,-1,-1,-1],
        "scenario_app_version":0,"scenario_competition_type":0,"original_main_weapon_ids":[],
        "player_count":2})
}

fn calculate(input: &Value) -> Value {
    serde_json::from_str(&calculate_obs_json(&input.to_string())).unwrap()
}

#[test]
fn equipment_matches_the_existing_native_twenty_cell_vectors() {
    // Literal vectors from packet-capture's independently checked synthetic
    // special-RNG/stock-pool regression; no packet-capture dependency is linked.
    for (version, expected) in [
        (
            0,
            json!([
                [24030, 20250, 25010, 20200],
                [20050, 20080, 21110, 20020],
                [27020, 20040, 20090, 24000],
                [20080, 20030, 22050, 27020],
                [22070, 21110, 25040, 26010]
            ]),
        ),
        (
            3 << 17,
            json!([
                [20020, 27020, 20040, 20090],
                [24000, 20080, 20030, 22050],
                [27020, 22070, 21110, 25040],
                [26010, 23030, 20250, 20230],
                [20400, 20260, 20210, 21110]
            ]),
        ),
    ] {
        let mut request = input();
        request["scenario_app_version"] = json!(version);
        let response = calculate(&request);
        assert_eq!(response["status"], "ok");
        assert_eq!(
            response["wave_preview"]["equipment"]["status"],
            "conditional"
        );
        assert_eq!(
            response["wave_preview"]["equipment"]["main_weapons"],
            expected
        );
        assert!(response.get("seed").is_none());
        assert!(!response.to_string().contains("GameSeed[0]"));
    }
}

#[test]
fn supplied_specials_reuse_the_core_vectors_with_explicit_remap_conditions() {
    // Literal independent core vectors: ocean-calc/tests/specials.rs.
    for (version, expected) in [
        (0, json!([20013, 20010, 20007, 20014])),
        (3 << 17, json!([20009, 20007, 20014, 20012])),
    ] {
        let mut request = input();
        request["scenario_app_version"] = json!(version);
        let response = calculate(&request);
        assert_eq!(
            response["wave_preview"]["equipment"]["special_weapons"],
            expected
        );
        let assumptions = response["wave_preview"]["equipment"]["assumptions"]
            .as_array()
            .unwrap();
        assert!(assumptions.contains(&json!("special_weapons assumes the runtime special-reorder gate is inactive and no later special override occurs")));
        assert!(assumptions.contains(&json!("special_weapons columns are internal equipment indices; room display additionally requires the normal adopted-scene raw-index mapping")));
    }
    let mut request = input();
    request["stage_resource"] = json!("Cop_Unknown");
    assert_eq!(
        calculate(&request)["wave_preview"]["equipment"]["special_weapons"],
        json!([20013, 20010, 20007, 20014])
    );
    for job in [0, 255] {
        request["job_type"] = json!(job);
        let response = calculate(&request);
        assert_eq!(
            response["wave_preview"]["equipment"]["status"],
            "unavailable"
        );
        assert_eq!(
            response["wave_preview"]["equipment"]["special_weapons"],
            json!([null, null, null, null])
        );
    }
}

#[test]
fn independent_equipment_survives_an_unsupported_stage_and_uncertain_cells_remain_null() {
    let mut request = input();
    request["stage_resource"] = json!("Cop_Unknown");
    request["main_weapon_ids"] = json!([65532, 22030, -1, 20040]);
    let response = calculate(&request);
    assert_eq!(response["status"], "ok");
    assert_eq!(response["wave_preview"]["status"], "unavailable");
    assert_eq!(
        response["wave_preview"]["equipment"]["status"],
        "conditional"
    );
    let assumptions = response["wave_preview"]["equipment"]["assumptions"]
        .as_array()
        .unwrap();
    assert!(assumptions.contains(&json!("the shared RNG and successful JobType 1 five-row builder are ready, with no later equipment override")));
    assert!(assumptions.contains(&json!("columns are internal equipment indices; only main IDs agreeing across both direct-mode branches are emitted")));
    for row in response["wave_preview"]["equipment"]["main_weapons"]
        .as_array()
        .unwrap()
    {
        assert!(row[0].is_null());
        assert_eq!(row[1], 22030);
        assert!(row[2].is_number());
        assert_eq!(row[3], 20040);
    }
}

#[test]
fn first_species_is_explicitly_conditional_and_missing_population_context_is_unavailable() {
    let response = calculate(&input());
    let scenarios = response["wave_preview"]["scenarios"].as_array().unwrap();
    assert_eq!(scenarios.len(), 2);
    let mut species = 0;
    for scenario in scenarios {
        let waves = scenario["waves"].as_array().unwrap();
        assert_eq!(waves.len(), 5);
        for wave in waves {
            let first = &wave["first_boss"];
            assert_eq!(first["basis"], "assumed_idealized");
            if first["status"] == "conditional" {
                species += 1;
                assert!((4..=15).contains(&first["species_id"].as_u64().unwrap()));
                assert_eq!(first["nominal_tick"], 0);
                assert!(!first["conditions"].as_array().unwrap().is_empty());
            } else {
                assert!(first["species_id"].is_null());
                assert!(first["reason"].is_string());
            }
        }
    }
    assert!(species > 0);
    // This synthetic low-global branch generates Fog first. Its periodic
    // cycle is not in our input contract and must not silently become zero.
    assert_eq!(scenarios[0]["waves"][0]["event_id"], 5);
    assert_eq!(
        scenarios[0]["waves"][0]["first_boss"]["status"],
        "unavailable"
    );
    assert!(scenarios[0]["waves"][0]["first_boss"]["species_id"].is_null());
    // Existing core first-Rare vectors give ID6 for this synthetic scene.
    assert_eq!(scenarios[0]["waves"][1]["first_boss"]["species_id"], 6);
    let mut unknown = input();
    unknown.as_object_mut().unwrap().remove("player_count");
    let unknown = calculate(&unknown);
    for scenario in unknown["wave_preview"]["scenarios"].as_array().unwrap() {
        for wave in scenario["waves"].as_array().unwrap() {
            assert_eq!(wave["first_boss"]["status"], "unavailable");
            assert!(wave["first_boss"]["species_id"].is_null());
        }
    }
}

#[test]
fn json_boundary_rejects_unknown_fields_ranges_shapes_and_utf8_byte_overflow() {
    let cases = [
        ("version", json!(2)),
        ("stage_resource", json!("../private")),
        ("job_type", json!(256)),
        ("difficulty", json!(1666)),
        ("seed", json!([1, 2, 3])),
        ("seed", json!([1, 2, 3, 4294967296u64])),
        ("unlocked_season", json!(-2)),
        ("main_weapon_ids", json!([-1, -1, -1, -1, -1])),
        ("main_weapon_ids", json!([-4])),
        ("original_main_weapon_ids", json!([65533])),
        ("scenario_app_version", json!(-1)),
        ("scenario_competition_type", json!(2)),
        ("player_count", json!(0)),
        ("player_count", json!(5)),
        ("private_extra", json!("do not echo")),
    ];
    for (key, value) in cases {
        let mut request = input();
        request[key] = value;
        let response = calculate(&request);
        assert_eq!(response["status"], "error", "{key}");
        assert!(response.get("wave_preview").is_none());
        assert!(!response.to_string().contains("do not echo"));
    }
    for raw in ["{", "[]", "null", "{\"version\":1,\"version\":1}"] {
        let response: Value = serde_json::from_str(&calculate_obs_json(raw)).unwrap();
        assert_eq!(response["status"], "error");
    }
    let raw = format!("\"{}\"", "あ".repeat(2800));
    assert!(raw.chars().count() < 8192 && raw.len() > 8192);
    let response: Value = serde_json::from_str(&calculate_obs_json(&raw)).unwrap();
    assert_eq!(response["error"]["code"], "input_too_large");
    let base = input().to_string();
    let exact = format!("{base}{}", " ".repeat(8192 - base.len()));
    let response: Value = serde_json::from_str(&calculate_obs_json(&exact)).unwrap();
    assert_eq!(response["status"], "ok");
    let response: Value = serde_json::from_str(&calculate_obs_json(&(exact + " "))).unwrap();
    assert_eq!(response["error"]["code"], "input_too_large");
}
