use salmon_wave_lookahead::difficulty::{
    ParticipantRating, effective_difficulty_job_one, effective_difficulty_job_zero,
};

#[test]
fn job_zero_averages_only_included_participant_records() {
    let records = [
        ParticipantRating {
            status_word: 0,
            difficulty: 100,
        },
        ParticipantRating {
            status_word: 1,
            difficulty: 1_000,
        },
        ParticipantRating {
            status_word: 0,
            difficulty: 401,
        },
    ];
    assert_eq!(effective_difficulty_job_zero(&records), 250);
    assert_eq!(effective_difficulty_job_zero(&[]), 0);
    assert_eq!(
        effective_difficulty_job_zero(&[ParticipantRating {
            status_word: 7,
            difficulty: 500,
        }]),
        0
    );
}

#[test]
fn job_zero_uses_i32_wrap_and_clamps_after_division() {
    let wrapped = [
        ParticipantRating {
            status_word: 0,
            difficulty: i32::MAX,
        },
        ParticipantRating {
            status_word: 0,
            difficulty: 1,
        },
    ];
    assert_eq!(effective_difficulty_job_zero(&wrapped), 0);
    assert_eq!(
        effective_difficulty_job_zero(&[ParticipantRating {
            status_word: 0,
            difficulty: 2_000,
        }]),
        1_665
    );
    assert_eq!(
        effective_difficulty_job_zero(&[ParticipantRating {
            status_word: 0,
            difficulty: -5,
        }]),
        0
    );
}

#[test]
fn job_one_uses_scene_difficulty_only_for_raw_match_mode_eleven() {
    assert_eq!(effective_difficulty_job_one(11, 500), 500);
    assert_eq!(effective_difficulty_job_one(11, 1_700), 1_665);
    assert_eq!(effective_difficulty_job_one(11, -9), 0);
    for mode in [0, 1, 10, 12, 20, 21, u32::MAX] {
        assert_eq!(effective_difficulty_job_one(mode, 500), 0);
    }
}
