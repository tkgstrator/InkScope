use salmon_wave_lookahead::{
    applied_wave::{
        ApplyJobOneError, EventEightAppliedFields, EventEightFieldsError, JobOneApplyState,
        apply_job_one_direct_writes, project_job_one_event_eight_fields,
    },
    records::WaveSelection,
};

fn selection(event_id: u8, tide_id: u8) -> WaveSelection {
    WaveSelection {
        wave_index: 3,
        event_id,
        tide_id,
    }
}

fn state() -> JobOneApplyState {
    JobOneApplyState {
        previous_applied_tide: 1,
        active_player_count: 4,
        mode_flag_591: false,
        special_flag_624: false,
        special_target_index_620: 3,
        runtime_mode_forces_mid_tide: false,
    }
}

#[test]
fn ordinary_job_one_applies_generated_event_and_tide() {
    let result = apply_job_one_direct_writes(selection(3, 2), state()).unwrap();
    assert_eq!((result.event_id, result.tide_id), (3, 2));
    assert_eq!(result.applied_word_594, 1);
    assert!(!result.special_tide_used);
    assert!(!result.requires_handler_observation);
}

#[test]
fn one_or_zero_active_players_force_event_zero_before_event_tide_rule() {
    for active_player_count in [0, 1] {
        let result = apply_job_one_direct_writes(
            selection(6, 2),
            JobOneApplyState {
                active_player_count,
                ..state()
            },
        )
        .unwrap();
        assert_eq!((result.event_id, result.tide_id), (0, 2));
        assert_eq!(result.applied_word_594, 0);
    }
    let two = apply_job_one_direct_writes(
        selection(6, 2),
        JobOneApplyState {
            active_player_count: 2,
            ..state()
        },
    )
    .unwrap();
    assert_eq!((two.event_id, two.tide_id), (6, 0));
    assert_eq!(two.applied_word_594, 1);
}

#[test]
fn special_tide_overrides_event_six_and_seven_low_tide_direct_write() {
    for event_id in [6, 7] {
        let result = apply_job_one_direct_writes(
            selection(event_id, 2),
            JobOneApplyState {
                mode_flag_591: true,
                special_flag_624: true,
                ..state()
            },
        )
        .unwrap();
        assert_eq!((result.event_id, result.tide_id), (event_id, 1));
        assert!(result.special_tide_used);
        assert_eq!(result.applied_word_594, 3);
    }
    let wrong_target = apply_job_one_direct_writes(
        selection(7, 2),
        JobOneApplyState {
            mode_flag_591: true,
            special_flag_624: true,
            special_target_index_620: 2,
            ..state()
        },
    )
    .unwrap();
    assert_eq!((wrong_target.event_id, wrong_target.tide_id), (7, 0));
    assert!(!wrong_target.special_tide_used);
}

#[test]
fn both_runtime_flags_are_required_for_special_tide() {
    for (mode_flag_591, special_flag_624) in [(false, true), (true, false)] {
        let result = apply_job_one_direct_writes(
            selection(6, 2),
            JobOneApplyState {
                mode_flag_591,
                special_flag_624,
                ..state()
            },
        )
        .unwrap();
        assert_eq!(result.tide_id, 0);
        assert!(!result.special_tide_used);
    }
}

#[test]
fn runtime_mode_class_two_or_three_forces_mid_tide_without_special_flag() {
    for (event_id, generated_tide) in [(0, 0), (0, 2), (6, 2), (7, 0)] {
        let result = apply_job_one_direct_writes(
            selection(event_id, generated_tide),
            JobOneApplyState {
                runtime_mode_forces_mid_tide: true,
                ..state()
            },
        )
        .unwrap();
        assert_eq!(result.tide_id, 1);
        assert!(!result.special_tide_used);
    }
    let special = apply_job_one_direct_writes(
        selection(6, 2),
        JobOneApplyState {
            previous_applied_tide: 2,
            mode_flag_591: true,
            special_flag_624: true,
            runtime_mode_forces_mid_tide: true,
            ..state()
        },
    )
    .unwrap();
    assert_eq!(special.tide_id, 2);
    assert!(special.special_tide_used);
}

#[test]
fn fog_has_direct_writes_while_event_eight_remains_opaque() {
    let fog = apply_job_one_direct_writes(selection(5, 1), state()).unwrap();
    assert_eq!(fog.applied_word_594, 2);
    assert!(!fog.requires_handler_observation);
    let event_eight = apply_job_one_direct_writes(selection(8, 1), state()).unwrap();
    assert_eq!(event_eight.applied_word_594, 1);
    assert!(event_eight.requires_handler_observation);
    for event_id in [5, 8] {
        let solo = apply_job_one_direct_writes(
            selection(event_id, 1),
            JobOneApplyState {
                active_player_count: 1,
                ..state()
            },
        )
        .unwrap();
        assert_eq!(solo.event_id, 0);
        assert_eq!(solo.applied_word_594, 0);
        assert!(!solo.requires_handler_observation);
    }
}

#[test]
fn invalid_generated_record_fields_are_rejected() {
    let mut generated = selection(0, 1);
    generated.wave_index = 5;
    assert_eq!(
        apply_job_one_direct_writes(generated, state()),
        Err(ApplyJobOneError::InvalidWaveIndex(5))
    );
    generated = selection(9, 1);
    assert_eq!(
        apply_job_one_direct_writes(generated, state()),
        Err(ApplyJobOneError::InvalidGeneratedEvent(9))
    );
    generated = selection(0, 3);
    assert_eq!(
        apply_job_one_direct_writes(generated, state()),
        Err(ApplyJobOneError::InvalidGeneratedTide(3))
    );
}

#[test]
fn event_eight_false_config_overwrites_only_its_two_raw_fields() {
    let direct = apply_job_one_direct_writes(selection(8, 2), state()).unwrap();
    let fields =
        project_job_one_event_eight_fields(direct, 0x89ab_cdef, 0x8000_0005, Some(false)).unwrap();
    assert_eq!(
        fields,
        EventEightAppliedFields {
            word_59c: 4,
            word_5a0: u32::MAX,
            event_eight_override_applied: true,
        }
    );
    assert_eq!((direct.event_id, direct.tide_id), (8, 2));
    assert!(direct.requires_handler_observation);
}

#[test]
fn event_eight_true_config_preserves_record_words() {
    let direct = apply_job_one_direct_writes(selection(8, 1), state()).unwrap();
    let fields =
        project_job_one_event_eight_fields(direct, 0x89ab_cdef, 0x8000_0005, Some(true)).unwrap();
    assert_eq!(fields.word_59c, 0x89ab_cdef);
    assert_eq!(fields.word_5a0, 0x8000_0005);
    assert!(!fields.event_eight_override_applied);
}

#[test]
fn event_eight_unknown_config_is_not_replaced_with_owned_romfs_value() {
    let direct = apply_job_one_direct_writes(selection(8, 1), state()).unwrap();
    assert_eq!(
        project_job_one_event_eight_fields(direct, 0x89ab_cdef, 0x8000_0005, None),
        Err(EventEightFieldsError::UnknownEffectiveConfig)
    );
}

#[test]
fn solo_override_to_event_zero_does_not_require_event_eight_config() {
    let direct = apply_job_one_direct_writes(
        selection(8, 1),
        JobOneApplyState {
            active_player_count: 1,
            ..state()
        },
    )
    .unwrap();
    let fields =
        project_job_one_event_eight_fields(direct, 0x89ab_cdef, 0x8000_0005, None).unwrap();
    assert_eq!((direct.event_id, direct.tide_id), (0, 1));
    assert_eq!(fields.word_59c, 0x89ab_cdef);
    assert_eq!(fields.word_5a0, 0x8000_0005);
    assert!(!fields.event_eight_override_applied);
}
