use ocean_calc::{
    SeedWords,
    levels::QueueCounts,
    preview::{CountOrigin, NominalScheduleStatus, QueueCountSource, predict_nominal_wave_preview},
    schedule::{EntryKind, ScheduleBranch, ScheduleError},
    stage_data::stage_spawns,
    stage_wave::{DifficultySource, StageWaveInput, WaveWeightSource, WaveWeights},
};

const NO_EVENT: WaveWeights = WaveWeights {
    event: [1, 0, 0, 0, 0, 0, 0, 0, 0],
    tide: [1, 3, 1],
};

fn input<'a>(weights: &'a [WaveWeights; 5], cutoff: Option<u8>) -> StageWaveInput<'a> {
    StageWaveInput {
        seed: SeedWords::from_scene([1, 2, 3, 4]),
        stage: stage_spawns("Cop_Shakehighway").unwrap(),
        difficulty: DifficultySource::JobOne {
            match_mode: 11,
            scene_difficulty: 1665,
        },
        is_big_run: false,
        global_word: 0x10100,
        cutoff,
        special: None,
        weights: WaveWeightSource::Explicit(weights),
    }
}

fn predicted(
    status: &NominalScheduleStatus,
) -> (QueueCounts, ScheduleBranch, Vec<(u32, EntryKind)>) {
    match status {
        NominalScheduleStatus::Predicted {
            counts,
            assumed_branch,
            entries,
        } => (
            *counts,
            *assumed_branch,
            entries
                .iter()
                .map(|entry| (entry.tick, entry.kind))
                .collect(),
        ),
        other => panic!("expected a nominal schedule, got {other:?}"),
    }
}

#[test]
fn source_rounds_follow_current_index_and_clamp_after_third_round() {
    let weights = [NO_EVENT; 5];
    let result = predict_nominal_wave_preview(
        input(&weights, None),
        QueueCountSource::RomfsClampedRoundByWave,
        [ScheduleBranch::Normal; 5],
    )
    .unwrap();
    assert_eq!(result.count_origin, CountOrigin::RomfsClampedRoundByWave);
    assert_eq!(result.waves.len(), 5);
    for (wave_index, expected_rare) in [30, 32, 35, 35, 35].into_iter().enumerate() {
        let wave = &result.waves[wave_index];
        assert_eq!(usize::from(wave.selection.wave_index), wave_index);
        assert_eq!(wave.selection.event_id, 0);
        let (counts, branch, entries) = predicted(&wave.schedule);
        assert_eq!(branch, ScheduleBranch::Normal);
        assert_eq!(counts.wave_change_num, 10);
        assert_eq!(counts.rare_appear_num, expected_rare);
        assert_eq!(entries.len(), expected_rare as usize);
        assert_eq!(entries.first(), Some(&(0, EntryKind::Base)));
        assert_eq!(entries.last(), Some(&(4320, EntryKind::Base)));
    }
    let (_, _, first) = predicted(&result.waves[0].schedule);
    let extra_ticks: Vec<_> = first
        .into_iter()
        .filter_map(|(tick, kind)| (kind == EntryKind::Extra).then_some(tick))
        .collect();
    assert_eq!(
        extra_ticks,
        [
            19, 296, 502, 776, 1031, 1155, 1500, 1889, 2220, 2290, 2458, 2688, 2904, 2990, 3646,
            3798, 3813, 4067, 4146, 4176
        ]
    );
}

#[test]
fn cutoff_and_unsupported_generated_event_keep_other_slots() {
    let mut weights = [NO_EVENT; 5];
    weights[2].event = [0, 1, 0, 0, 0, 0, 0, 0, 0];
    let counts = [QueueCounts {
        wave_change_num: 10,
        rare_appear_num: 12,
    }; 5];
    let result = predict_nominal_wave_preview(
        input(&weights, Some(1)),
        QueueCountSource::Explicit(&counts),
        [ScheduleBranch::Normal; 5],
    )
    .unwrap();
    assert_eq!(result.count_origin, CountOrigin::Explicit);
    assert_eq!(
        result
            .waves
            .iter()
            .map(|wave| wave.selection.wave_index)
            .collect::<Vec<_>>(),
        [2, 3, 4]
    );
    assert_eq!(
        result.waves[0].schedule,
        NominalScheduleStatus::UnsupportedGeneratedEvent(1)
    );
    assert!(matches!(
        result.waves[1].schedule,
        NominalScheduleStatus::Predicted { .. }
    ));
    let regenerated = predict_nominal_wave_preview(
        input(&weights, Some(2)),
        QueueCountSource::Explicit(&counts),
        [ScheduleBranch::Normal; 5],
    )
    .unwrap();
    assert_eq!(regenerated.waves.len(), 2);
    assert_eq!(regenerated.waves[0].selection.wave_index, 3);
}

#[test]
fn fog_and_missile_preview_nominal_rare_queue_only() {
    let counts = [QueueCounts {
        wave_change_num: 10,
        rare_appear_num: 12,
    }; 5];
    for event_type in [5, 6] {
        let mut weights = [NO_EVENT; 5];
        weights[0].event = [0; 9];
        weights[0].event[event_type] = 1;
        let result = predict_nominal_wave_preview(
            input(&weights, None),
            QueueCountSource::Explicit(&counts),
            [ScheduleBranch::Normal; 5],
        )
        .unwrap();
        assert_eq!(result.waves[0].selection.event_id, event_type as u8);
        let (source, _, entries) = predicted(&result.waves[0].schedule);
        assert_eq!(source, counts[0]);
        assert_eq!(entries.len(), 12);
        assert_eq!(entries.first(), Some(&(0, EntryKind::Base)));
    }
}

#[test]
fn explicit_counts_and_branch_change_only_the_selected_wave() {
    let weights = [NO_EVENT; 5];
    let mut counts = [QueueCounts {
        wave_change_num: 10,
        rare_appear_num: 12,
    }; 5];
    counts[1].rare_appear_num = 11;
    let mut branches = [ScheduleBranch::Normal; 5];
    branches[0] = ScheduleBranch::Special;
    let result = predict_nominal_wave_preview(
        input(&weights, None),
        QueueCountSource::Explicit(&counts),
        branches,
    )
    .unwrap();
    let (_, branch_zero, wave_zero) = predicted(&result.waves[0].schedule);
    assert_eq!(branch_zero, ScheduleBranch::Special);
    let extras: Vec<_> = wave_zero
        .into_iter()
        .filter_map(|(tick, kind)| (kind == EntryKind::Extra).then_some(tick))
        .collect();
    assert_eq!(extras, [0, 816]);
    let (wave_one_counts, branch_one, wave_one) = predicted(&result.waves[1].schedule);
    assert_eq!(wave_one_counts.rare_appear_num, 11);
    assert_eq!(branch_one, ScheduleBranch::Normal);
    assert_eq!(wave_one.len(), 11);
}

#[test]
fn fewer_rare_items_keep_base_queue_and_invalid_counts_are_reported_per_wave() {
    let weights = [NO_EVENT; 5];
    let mut counts = [QueueCounts {
        wave_change_num: 10,
        rare_appear_num: 12,
    }; 5];
    counts[1].rare_appear_num = 9;
    counts[2].wave_change_num = 1;
    let result = predict_nominal_wave_preview(
        input(&weights, None),
        QueueCountSource::Explicit(&counts),
        [ScheduleBranch::Normal; 5],
    )
    .unwrap();
    assert!(matches!(
        result.waves[0].schedule,
        NominalScheduleStatus::Predicted { .. }
    ));
    let (_, _, base_only) = predicted(&result.waves[1].schedule);
    assert_eq!(base_only.len(), 10);
    assert!(base_only.iter().all(|(_, kind)| *kind == EntryKind::Base));
    assert_eq!(
        result.waves[2].schedule,
        NominalScheduleStatus::Rejected(ScheduleError::InvalidCounts)
    );
}
