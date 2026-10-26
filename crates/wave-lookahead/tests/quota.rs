use salmon_wave_lookahead::{
    levels::{LevelError, common_round_quota_base},
    quota::{QuotaError, StockWaveQuota, job_zero_one_quota, stock_job_zero_one_wave_quota},
};

#[test]
fn owned_quota_rows_match_all_nine_difficulties() {
    let rows: [(u32, [i32; 3]); 9] = [
        (0, [3, 4, 5]),
        (40, [5, 6, 7]),
        (100, [8, 9, 10]),
        (200, [9, 10, 12]),
        (300, [11, 12, 14]),
        (400, [13, 14, 16]),
        (500, [16, 17, 19]),
        (1000, [26, 28, 30]),
        (1665, [30, 32, 35]),
    ];
    for (difficulty, expected) in rows {
        for (round, base) in expected.into_iter().enumerate() {
            assert_eq!(
                common_round_quota_base(difficulty, round as u8),
                Ok(base),
                "difficulty={difficulty}, round={round}"
            );
        }
    }
}

#[test]
fn quota_base_interpolates_and_rejects_invalid_source_round() {
    assert_eq!(common_round_quota_base(150, 2), Ok(11));
    assert_eq!(common_round_quota_base(1666, 0), Ok(30));
    assert_eq!(
        common_round_quota_base(200, 3),
        Err(LevelError::InvalidRound(3))
    );
}

#[test]
fn four_player_counts_scale_stock_difficulty_200_rows() {
    let expected = [[2, 2, 3], [4, 5, 6], [6, 7, 9], [9, 10, 12]];
    for players in 1..=4 {
        for round in 0..3 {
            let got =
                stock_job_zero_one_wave_quota(200, round as u8, players, false, true).unwrap();
            assert_eq!(got.source_round_index, round as u8);
            assert_eq!(got.source_base, [9, 10, 12][round]);
            assert_eq!(got.quota, expected[(players - 1) as usize][round]);
        }
    }
}

#[test]
fn wave_three_and_four_reuse_source_round_two() {
    for wave in [3, 4] {
        assert_eq!(
            stock_job_zero_one_wave_quota(1665, wave, 1, false, true),
            Ok(StockWaveQuota {
                source_round_index: 2,
                source_base: 35,
                quota: 8,
            })
        );
    }
    assert_eq!(
        stock_job_zero_one_wave_quota(200, 5, 4, false, true),
        Err(QuotaError::InvalidWaveIndex(5))
    );
    assert_eq!(
        stock_job_zero_one_wave_quota(200, 1, 4, true, false),
        Err(QuotaError::MissingSceneOrSequence)
    );
}

#[test]
fn zero_special_and_wrapping_product_follow_elf_order() {
    assert_eq!(job_zero_one_quota(0, 4, false), 0);
    assert_eq!(job_zero_one_quota(9, 0, false), 1);
    assert_eq!(job_zero_one_quota(9, 4, true), 0);
    assert_eq!(job_zero_one_quota(0, 4, true), 0);
    assert_eq!(job_zero_one_quota(i32::MAX, 4, false), 1);
    assert_eq!(job_zero_one_quota(i32::MAX, 3, false), 536_870_911);
    assert_eq!(job_zero_one_quota(-8, 4, false), 1);
}
