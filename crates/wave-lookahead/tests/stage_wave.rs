use salmon_wave_lookahead::{
    SeedWords,
    difficulty::ParticipantRating,
    records::SpecialWaveState,
    stage_data::stage_spawns,
    stage_wave::{
        DifficultySource, StageWaveInput, WaveWeightSource, WaveWeights, WeightOrigin,
        predict_stage_wave_selections,
    },
};

fn derived_input<'a>(stage_name: &str) -> StageWaveInput<'a> {
    StageWaveInput {
        seed: SeedWords::from_scene([4, 2, 3, 4]),
        stage: stage_spawns(stage_name).unwrap(),
        difficulty: DifficultySource::JobOne {
            match_mode: 11,
            scene_difficulty: 0,
        },
        is_big_run: false,
        global_word: 0x10100,
        cutoff: None,
        special: None,
        weights: WaveWeightSource::RomfsDerived,
    }
}

#[test]
fn owned_stage_water_changes_the_independently_calculated_tide_vector() {
    let carousel = predict_stage_wave_selections(derived_input("Cop_Carousel")).unwrap();
    let temple = predict_stage_wave_selections(derived_input("Cop_Temple")).unwrap();
    assert_eq!(carousel.effective_difficulty, 0);
    assert_eq!(carousel.weight_origin, WeightOrigin::RomfsDerived);
    assert_eq!(carousel.weights_used[0].event, [1, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(carousel.weights_used[0].tide, [2, 2, 1]);
    assert_eq!(temple.weights_used[0].tide, [1, 3, 1]);
    assert_eq!(
        carousel
            .generation
            .selections
            .iter()
            .map(|selection| (selection.event_id, selection.tide_id))
            .collect::<Vec<_>>(),
        [(0, 0), (0, 2), (0, 2), (0, 2), (0, 1)]
    );
    assert_eq!(
        temple
            .generation
            .selections
            .iter()
            .map(|selection| selection.tide_id)
            .collect::<Vec<_>>(),
        [1, 2, 2, 2, 1]
    );
}

#[test]
fn explicit_adjacent_wave_weights_override_water_but_preserve_stage_ban() {
    let mut weights = [WaveWeights {
        event: [1, 0, 0, 0, 0, 0, 0, 0, 0],
        tide: [1, 3, 1],
    }; 5];
    weights[0] = WaveWeights {
        event: [0, 0, 0, 1, 0, 0, 0, 0, 0],
        tide: [0, 0, 1],
    };
    weights[1] = WaveWeights {
        event: [0, 0, 0, 0, 1, 0, 0, 0, 0],
        tide: [1, 0, 0],
    };
    let ratings = [ParticipantRating {
        status_word: 0,
        difficulty: 100,
    }];
    let mut input = derived_input("Cop_Temple");
    input.difficulty = DifficultySource::JobZero(&ratings);
    input.weights = WaveWeightSource::Explicit(&weights);
    let result = predict_stage_wave_selections(input).unwrap();
    assert_eq!(result.effective_difficulty, 100);
    assert_eq!(result.weight_origin, WeightOrigin::Explicit);
    assert_eq!(result.weights_used, weights);
    assert_eq!(result.generation.selections[0].event_id, 0);
    assert_eq!(result.generation.selections[0].tide_id, 2);
    assert_eq!(result.generation.selections[1].event_id, 4);
    assert_eq!(result.generation.selections[1].tide_id, 0);
}

#[test]
fn cutoff_and_job_one_special_state_are_passed_through() {
    let mut input = derived_input("Cop_Carousel");
    input.cutoff = Some(2);
    let regenerated = predict_stage_wave_selections(input).unwrap();
    assert_eq!(
        regenerated
            .generation
            .selections
            .iter()
            .map(|selection| selection.wave_index)
            .collect::<Vec<_>>(),
        [3, 4]
    );

    input.cutoff = None;
    input.special = Some(SpecialWaveState {
        target_index: 3,
        stored_tide: 1,
    });
    let special = predict_stage_wave_selections(input).unwrap();
    assert_eq!(special.generation.selections[3].event_id, 0);
    assert_eq!(special.generation.selections[3].tide_id, 1);

    let ratings = [ParticipantRating {
        status_word: 0,
        difficulty: 100,
    }];
    input.difficulty = DifficultySource::JobZero(&ratings);
    let job_zero_special = predict_stage_wave_selections(input).unwrap();
    assert_eq!(job_zero_special.generation.selections[3].event_id, 0);
    assert_eq!(job_zero_special.generation.selections[3].tide_id, 1);
}
