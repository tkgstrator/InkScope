use salmon_wave_lookahead::{
    SeedWords,
    applied_preview::{
        AppliedPreviewError, AppliedScheduleStatus, JobOneSchedulerState,
        predict_job_one_applied_preview,
    },
    applied_wave::JobOneApplyState,
    levels::QueueCounts,
    preview::{CountOrigin, QueueCountSource},
    schedule::{ScheduleBranch, ScheduleError},
    stage_data::stage_spawns,
    stage_wave::{DifficultySource, StageWaveInput, WaveWeightSource, WaveWeights},
};

const NO_EVENT: WaveWeights = WaveWeights {
    event: [1, 0, 0, 0, 0, 0, 0, 0, 0],
    tide: [1, 3, 1],
};

const COUNTS: QueueCounts = QueueCounts {
    wave_change_num: 10,
    rare_appear_num: 12,
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

fn state() -> JobOneApplyState {
    JobOneApplyState {
        previous_applied_tide: 1,
        active_player_count: 4,
        mode_flag_591: false,
        special_flag_624: false,
        special_target_index_620: 0,
        runtime_mode_forces_mid_tide: false,
    }
}

fn scheduler_states() -> [JobOneSchedulerState; 5] {
    [JobOneSchedulerState {
        runtime_mode_bypasses_rare_queue: false,
    }; 5]
}

fn only_event(weights: &mut [WaveWeights; 5], index: usize, event: usize) {
    weights[index].event = [0; 9];
    weights[index].event[event] = 1;
}

#[test]
fn solo_application_uses_event_zero_queue_even_when_record_generated_event_one() {
    let mut weights = [NO_EVENT; 5];
    only_event(&mut weights, 0, 1);
    let mut states = [state(); 5];
    states[0].active_player_count = 1;
    let result = predict_job_one_applied_preview(
        input(&weights, None),
        QueueCountSource::Explicit(&[COUNTS; 5]),
        [ScheduleBranch::Normal; 5],
        states,
        scheduler_states(),
    )
    .unwrap();
    assert_eq!(result.count_origin, CountOrigin::Explicit);
    assert_eq!(result.waves[0].generated.event_id, 1);
    assert_eq!(result.waves[0].direct.event_id, 0);
    assert_eq!(result.waves[0].direct.applied_word_594, 0);
    assert!(matches!(
        result.waves[0].schedule,
        AppliedScheduleStatus::Predicted { counts: COUNTS, .. }
    ));
}

#[test]
fn fog_and_missile_use_applied_event_and_direct_tide_writes() {
    let mut weights = [NO_EVENT; 5];
    only_event(&mut weights, 0, 5);
    only_event(&mut weights, 1, 6);
    let result = predict_job_one_applied_preview(
        input(&weights, None),
        QueueCountSource::Explicit(&[COUNTS; 5]),
        [ScheduleBranch::Normal; 5],
        [state(); 5],
        scheduler_states(),
    )
    .unwrap();
    assert_eq!(result.waves[0].direct.applied_word_594, 2);
    assert!(!result.waves[0].direct.requires_handler_observation);
    assert!(matches!(
        result.waves[0].schedule,
        AppliedScheduleStatus::Predicted { .. }
    ));
    assert_eq!(result.waves[1].direct.event_id, 6);
    assert_eq!(result.waves[1].direct.tide_id, 0);
    assert!(matches!(
        result.waves[1].schedule,
        AppliedScheduleStatus::Predicted { .. }
    ));
}

#[test]
fn event_eight_handler_and_other_unmodeled_event_are_distinct() {
    let mut weights = [NO_EVENT; 5];
    only_event(&mut weights, 0, 8);
    only_event(&mut weights, 1, 7);
    let result = predict_job_one_applied_preview(
        input(&weights, None),
        QueueCountSource::Explicit(&[COUNTS; 5]),
        [ScheduleBranch::Normal; 5],
        [state(); 5],
        scheduler_states(),
    )
    .unwrap();
    assert_eq!(result.waves[0].generated.event_id, 8);
    assert_eq!(
        result.waves[0].schedule,
        AppliedScheduleStatus::RequiresHandlerObservation(8)
    );
    assert_eq!(result.waves[1].generated.event_id, 7);
    assert_eq!(
        result.waves[1].schedule,
        AppliedScheduleStatus::UnsupportedAppliedEvent(7)
    );
}

#[test]
fn cutoff_preserves_original_indices_for_state_counts_and_branch() {
    let mut weights = [NO_EVENT; 5];
    only_event(&mut weights, 2, 6);
    let mut states = [state(); 5];
    states[2].mode_flag_591 = true;
    states[2].special_flag_624 = true;
    states[2].special_target_index_620 = 2;
    states[2].previous_applied_tide = 2;
    let mut counts = [COUNTS; 5];
    counts[2].rare_appear_num = 13;
    let mut branches = [ScheduleBranch::Normal; 5];
    branches[2] = ScheduleBranch::Special;
    let result = predict_job_one_applied_preview(
        input(&weights, Some(1)),
        QueueCountSource::Explicit(&counts),
        branches,
        states,
        scheduler_states(),
    )
    .unwrap();
    assert_eq!(
        result
            .waves
            .iter()
            .map(|wave| wave.generated.wave_index)
            .collect::<Vec<_>>(),
        [2, 3, 4]
    );
    assert_eq!(result.waves[0].direct.tide_id, 2);
    assert_eq!(result.waves[0].direct.applied_word_594, 3);
    assert!(matches!(
        result.waves[0].schedule,
        AppliedScheduleStatus::Predicted {
            counts: QueueCounts {
                rare_appear_num: 13,
                ..
            },
            assumed_branch: ScheduleBranch::Special,
            ..
        }
    ));
    assert_eq!(
        result.waves[1].direct.tide_id,
        u32::from(result.waves[1].generated.tide_id)
    );
}

#[test]
fn per_wave_runtime_mode_can_force_mid_tide_after_generated_or_event_tide() {
    let mut weights = [NO_EVENT; 5];
    only_event(&mut weights, 1, 6);
    let mut states = [state(); 5];
    states[1].runtime_mode_forces_mid_tide = true;
    let mut schedulers = scheduler_states();
    schedulers[1].runtime_mode_bypasses_rare_queue = true;
    let result = predict_job_one_applied_preview(
        input(&weights, None),
        QueueCountSource::Explicit(&[COUNTS; 5]),
        [ScheduleBranch::Normal; 5],
        states,
        schedulers,
    )
    .unwrap();
    assert_eq!(result.waves[1].direct.event_id, 6);
    assert_eq!(result.waves[1].direct.tide_id, 1);
    assert_eq!(
        result.waves[1].schedule,
        AppliedScheduleStatus::QueueBypassedByRuntimeMode
    );
    assert!(matches!(
        result.waves[0].schedule,
        AppliedScheduleStatus::Predicted { .. }
    ));
    assert_eq!(
        result.waves[2].direct.tide_id,
        u32::from(result.waves[2].generated.tide_id)
    );
}

#[test]
fn runtime_mode_bypass_takes_precedence_over_event_eight_uncertainty() {
    let mut weights = [NO_EVENT; 5];
    only_event(&mut weights, 0, 8);
    let mut states = [state(); 5];
    states[0].runtime_mode_forces_mid_tide = true;
    let mut schedulers = scheduler_states();
    schedulers[0].runtime_mode_bypasses_rare_queue = true;
    let result = predict_job_one_applied_preview(
        input(&weights, None),
        QueueCountSource::Explicit(&[COUNTS; 5]),
        [ScheduleBranch::Normal; 5],
        states,
        schedulers,
    )
    .unwrap();
    assert_eq!(result.waves[0].generated.event_id, 8);
    assert_eq!(result.waves[0].direct.tide_id, 1);
    assert_eq!(
        result.waves[0].schedule,
        AppliedScheduleStatus::QueueBypassedByRuntimeMode
    );
}

#[test]
fn application_and_later_scheduler_mode_are_independent_inputs() {
    let weights = [NO_EVENT; 5];
    let mut states = [state(); 5];
    states[0].runtime_mode_forces_mid_tide = true;
    let mut schedulers = scheduler_states();
    schedulers[1].runtime_mode_bypasses_rare_queue = true;
    let result = predict_job_one_applied_preview(
        input(&weights, None),
        QueueCountSource::Explicit(&[COUNTS; 5]),
        [ScheduleBranch::Normal; 5],
        states,
        schedulers,
    )
    .unwrap();
    assert_eq!(result.waves[0].direct.tide_id, 1);
    assert!(matches!(
        result.waves[0].schedule,
        AppliedScheduleStatus::Predicted { .. }
    ));
    assert_eq!(
        result.waves[1].direct.tide_id,
        u32::from(result.waves[1].generated.tide_id)
    );
    assert_eq!(
        result.waves[1].schedule,
        AppliedScheduleStatus::QueueBypassedByRuntimeMode
    );
}

#[test]
fn job_zero_is_rejected_before_generation_and_invalid_counts_are_per_wave() {
    let weights = [NO_EVENT; 5];
    let mut job_zero = input(&weights, None);
    job_zero.difficulty = DifficultySource::JobZero(&[]);
    assert_eq!(
        predict_job_one_applied_preview(
            job_zero,
            QueueCountSource::RomfsClampedRoundByWave,
            [ScheduleBranch::Normal; 5],
            [state(); 5],
            scheduler_states(),
        ),
        Err(AppliedPreviewError::RequiresJobTypeOne)
    );
    let mut counts = [COUNTS; 5];
    counts[1].wave_change_num = 1;
    let result = predict_job_one_applied_preview(
        input(&weights, None),
        QueueCountSource::Explicit(&counts),
        [ScheduleBranch::Normal; 5],
        [state(); 5],
        scheduler_states(),
    )
    .unwrap();
    assert_eq!(
        result.waves[1].schedule,
        AppliedScheduleStatus::Rejected(ScheduleError::InvalidCounts)
    );
    assert!(matches!(
        result.waves[0].schedule,
        AppliedScheduleStatus::Predicted { .. }
    ));
}
