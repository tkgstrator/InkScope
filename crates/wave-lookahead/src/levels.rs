//! Queue counts from the 11.3.0 `spl__CoopLevelsConfig` difficulty rows.
//!
//! `difficulty` must be the effective game difficulty, determined elsewhere.

use crate::tamaire::TamaireTypeWeights;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueueCounts {
    pub wave_change_num: u32,
    pub rare_appear_num: u32,
}

/// Owned common Round caps, before any effective resource replacement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoundPopulationLimits {
    pub first_half: i32,
    pub second_half: i32,
}

/// Live inputs used when the game turns one Rare queue item into its request
/// count and total-active limit. The level follower number must already be
/// interpolated for the effective difficulty.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrdinaryItemLimitInput {
    pub effective_rare_follower_num: i32,
    /// Scene `+0x5a4`; value 7 overrides the follower number to 2.
    pub scene_mode: u32,
    /// Nonzero byte at queue item `+8`; this overrides both values above.
    pub queue_item_flag: bool,
    /// Director `+0x4e8`, normally initialized to `i32::MIN`.
    pub deadline_marker: i32,
    /// Director `+0x30` at the moment this item is built.
    pub director_time: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrdinaryItemLimits {
    pub requested_count: i32,
    pub item_total_limit: i32,
}

/// Derive the two item scalars used by Event 0's live processor.
///
/// ELF `0x3765030..0x37650a4` selects `n`, sets request count to `n+1`,
/// and sets the active-total limit to either `31-n` or `30-n`. The special
/// marker value `INT_MIN` chooses 31 even when it is below director time.
/// This does not establish the runtime marker, level row, actor counts, or
/// successful creation/registration outcomes.
pub fn ordinary_item_runtime_limits(input: OrdinaryItemLimitInput) -> OrdinaryItemLimits {
    let followers = if input.queue_item_flag {
        0
    } else if input.scene_mode == 7 {
        2
    } else {
        input.effective_rare_follower_num
    };
    let base: i32 =
        if input.deadline_marker == i32::MIN || input.deadline_marker >= input.director_time {
            31
        } else {
            30
        };
    OrdinaryItemLimits {
        requested_count: followers.wrapping_add(1),
        item_total_limit: base.wrapping_sub(followers),
    }
}

/// Source weights from the common 11.3.0 level rows, before runtime overrides.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommonWeights {
    pub water: [u32; 3],
    pub event: [u32; 9],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LevelError {
    InvalidRound(u8),
}

#[derive(Clone, Copy)]
struct LevelRow {
    difficulty: u32,
    wave_change_num: u32,
    rare_appear_num: [u32; 3],
    norma_golden_ikura_num: [i32; 3],
    enemy_active_max_1st_half: [i32; 3],
    enemy_active_max_2nd_half: [i32; 3],
    rare_follower_num: u32,
    thanks_stage_rare_cycle: u32,
}

const LEVELS: [LevelRow; 9] = [
    LevelRow {
        difficulty: 0,
        wave_change_num: 3,
        rare_appear_num: [3, 3, 3],
        norma_golden_ikura_num: [3, 4, 5],
        enemy_active_max_1st_half: [3, 3, 4],
        enemy_active_max_2nd_half: [5, 5, 7],
        rare_follower_num: 1,
        thanks_stage_rare_cycle: 3,
    },
    LevelRow {
        difficulty: 40,
        wave_change_num: 3,
        rare_appear_num: [3, 3, 4],
        norma_golden_ikura_num: [5, 6, 7],
        enemy_active_max_1st_half: [3, 4, 5],
        enemy_active_max_2nd_half: [5, 6, 8],
        rare_follower_num: 1,
        thanks_stage_rare_cycle: 3,
    },
    LevelRow {
        difficulty: 100,
        wave_change_num: 4,
        rare_appear_num: [5, 6, 7],
        norma_golden_ikura_num: [8, 9, 10],
        enemy_active_max_1st_half: [7, 8, 9],
        enemy_active_max_2nd_half: [11, 12, 13],
        rare_follower_num: 2,
        thanks_stage_rare_cycle: 3,
    },
    LevelRow {
        difficulty: 200,
        wave_change_num: 4,
        rare_appear_num: [6, 7, 9],
        norma_golden_ikura_num: [9, 10, 12],
        enemy_active_max_1st_half: [8, 9, 10],
        enemy_active_max_2nd_half: [12, 14, 16],
        rare_follower_num: 2,
        thanks_stage_rare_cycle: 3,
    },
    LevelRow {
        difficulty: 300,
        wave_change_num: 5,
        rare_appear_num: [7, 9, 11],
        norma_golden_ikura_num: [11, 12, 14],
        enemy_active_max_1st_half: [9, 10, 12],
        enemy_active_max_2nd_half: [14, 16, 18],
        rare_follower_num: 2,
        thanks_stage_rare_cycle: 3,
    },
    LevelRow {
        difficulty: 400,
        wave_change_num: 5,
        rare_appear_num: [9, 11, 13],
        norma_golden_ikura_num: [13, 14, 16],
        enemy_active_max_1st_half: [10, 12, 14],
        enemy_active_max_2nd_half: [16, 18, 20],
        rare_follower_num: 2,
        thanks_stage_rare_cycle: 4,
    },
    LevelRow {
        difficulty: 500,
        wave_change_num: 6,
        rare_appear_num: [11, 13, 15],
        norma_golden_ikura_num: [16, 17, 19],
        enemy_active_max_1st_half: [12, 14, 16],
        enemy_active_max_2nd_half: [18, 20, 22],
        rare_follower_num: 0,
        thanks_stage_rare_cycle: 5,
    },
    LevelRow {
        difficulty: 1000,
        wave_change_num: 8,
        rare_appear_num: [20, 22, 24],
        norma_golden_ikura_num: [26, 28, 30],
        enemy_active_max_1st_half: [22, 24, 26],
        enemy_active_max_2nd_half: [28, 30, 32],
        rare_follower_num: 0,
        thanks_stage_rare_cycle: 7,
    },
    LevelRow {
        difficulty: 1665,
        wave_change_num: 10,
        rare_appear_num: [30, 32, 35],
        norma_golden_ikura_num: [30, 32, 35],
        enemy_active_max_1st_half: [24, 25, 26],
        enemy_active_max_2nd_half: [30, 31, 32],
        rare_follower_num: 0,
        thanks_stage_rare_cycle: 8,
    },
];

/// Owned 11.3.0 `Levels[].EventFog.RareAppearCycle` source rows, kept separate
/// from the common row's `ThanksStageRareCycle` setting.
const FOG_RARE_CYCLE_ROWS: [(u32, u32); 9] = [
    (0, 3),
    (40, 3),
    (100, 3),
    (200, 4),
    (300, 4),
    (400, 5),
    (500, 5),
    (1000, 5),
    (1665, 5),
];

/// `Levels[].EventTamaire` in the owned 11.3.0 resource. At difficulty 0 and
/// 40, Large and Small are omitted and retain constructor defaults of 1.
const TAMAIRE_TYPE_WEIGHT_ROWS: [(u32, TamaireTypeWeights); 9] = [
    (
        0,
        TamaireTypeWeights {
            large: 1,
            small: 1,
            standard: 8,
        },
    ),
    (
        40,
        TamaireTypeWeights {
            large: 1,
            small: 1,
            standard: 8,
        },
    ),
    (
        100,
        TamaireTypeWeights {
            large: 1,
            small: 2,
            standard: 7,
        },
    ),
    (
        200,
        TamaireTypeWeights {
            large: 2,
            small: 3,
            standard: 10,
        },
    ),
    (
        300,
        TamaireTypeWeights {
            large: 2,
            small: 3,
            standard: 7,
        },
    ),
    (
        400,
        TamaireTypeWeights {
            large: 3,
            small: 4,
            standard: 7,
        },
    ),
    (
        500,
        TamaireTypeWeights {
            large: 4,
            small: 5,
            standard: 6,
        },
    ),
    (
        1000,
        TamaireTypeWeights {
            large: 5,
            small: 6,
            standard: 3,
        },
    ),
    (
        1665,
        TamaireTypeWeights {
            large: 5,
            small: 5,
            standard: 1,
        },
    ),
];

fn interpolate(lower: u32, upper: u32, ratio: f32) -> u32 {
    ((upper as f32 - lower as f32).mul_add(ratio, lower as f32)).trunc() as u32
}

fn interpolate_signed(lower: i32, upper: i32, ratio: f32) -> i32 {
    ((upper as f32 - lower as f32).mul_add(ratio, lower as f32)).trunc() as i32
}

/// Stock Event 8 type weights at an explicit effective difficulty.
///
/// This reads the constructor-resolved 11.3.0 source rows only. The Event 8
/// spawn path reads live `scene+0x4c8` getters instead, so this result is not
/// sufficient to establish the actual type choice after runtime overrides.
pub fn common_tamaire_type_weights(difficulty: u32) -> TamaireTypeWeights {
    let difficulty = difficulty.min(1665);
    let upper_index = TAMAIRE_TYPE_WEIGHT_ROWS
        .iter()
        .position(|row| difficulty <= row.0)
        .expect("last Tamaire row covers capped difficulty");
    let (upper_difficulty, upper) = TAMAIRE_TYPE_WEIGHT_ROWS[upper_index];
    if difficulty == upper_difficulty || upper_index == 0 {
        return upper;
    }
    let (lower_difficulty, lower) = TAMAIRE_TYPE_WEIGHT_ROWS[upper_index - 1];
    let ratio =
        (difficulty - lower_difficulty) as f32 / (upper_difficulty - lower_difficulty) as f32;
    TamaireTypeWeights {
        large: interpolate_signed(lower.large, upper.large, ratio),
        small: interpolate_signed(lower.small, upper.small, ratio),
        standard: interpolate_signed(lower.standard, upper.standard, ratio),
    }
}

/// The common `ThanksStageRareCycle` value read by Event 0's periodic ID 15
/// predicate, after level-row interpolation for an explicit effective difficulty.
///
/// All nine owned 11.3.0 `spl__CoopLevelsConfig.Levels` rows set this field.
/// This source-row calculation does not establish the live difficulty or rule
/// out a later effective-setting override. Supply the resolved live value to
/// `PeriodicGoldieInput` instead when it differs from these common rows.
pub fn common_event_zero_rare_cycle(difficulty: u32) -> i32 {
    let difficulty = difficulty.min(1665);
    let upper_index = LEVELS
        .iter()
        .position(|row| difficulty <= row.difficulty)
        .expect("last level covers capped difficulty");
    let upper = LEVELS[upper_index];
    let cycle = if difficulty == upper.difficulty || upper_index == 0 {
        upper.thanks_stage_rare_cycle
    } else {
        let lower = LEVELS[upper_index - 1];
        let ratio =
            (difficulty - lower.difficulty) as f32 / (upper.difficulty - lower.difficulty) as f32;
        interpolate(
            lower.thanks_stage_rare_cycle,
            upper.thanks_stage_rare_cycle,
            ratio,
        )
    };
    cycle as i32
}

/// Stock Event 5 `RareAppearCycle` under normal level-row interpolation.
///
/// The owned `EventFog` rows map this field to the processor's config `+0x44`.
/// The nested live reflection visitor and possible runtime config replacement
/// are not fully traced, so use an observed effective cycle when available.
/// The separate global flag's optional half-cycle is applied later by
/// `species::periodic_goldie_source`, not by this source-row calculation.
pub fn common_fog_rare_cycle(difficulty: u32) -> i32 {
    let difficulty = difficulty.min(1665);
    let upper_index = FOG_RARE_CYCLE_ROWS
        .iter()
        .position(|row| difficulty <= row.0)
        .expect("last Fog row covers capped difficulty");
    let upper = FOG_RARE_CYCLE_ROWS[upper_index];
    let cycle = if difficulty == upper.0 || upper_index == 0 {
        upper.1
    } else {
        let lower = FOG_RARE_CYCLE_ROWS[upper_index - 1];
        let ratio = (difficulty - lower.0) as f32 / (upper.0 - lower.0) as f32;
        interpolate(lower.1, upper.1, ratio)
    };
    cycle as i32
}

/// Stock Event 1 `EnemyAppearInterval` under the generic signed-int level blend.
///
/// The owned EventRush rows contain -1 at difficulty 0 and 10 at every stored
/// row from 40 through 1665. The intermediate values assume the nested field
/// participates in the ordinary reflective interpolation without a later
/// override. A negative result requests the separate Round half-interval
/// fallback; it is not a resolved timer reset interval.
pub fn common_rush_enemy_appear_interval(difficulty: u32) -> i32 {
    let difficulty = difficulty.min(1665);
    if difficulty >= 40 {
        10
    } else {
        (11.0_f32.mul_add(difficulty as f32 / 40.0, -1.0)).trunc() as i32
    }
}

/// Stock Event 1 `EnemyActiveMax` source value for an explicit difficulty.
///
/// This assumes the nested Rush field uses the ordinary signed f32 level
/// interpolation and no later effective-setting override. A negative result
/// asks the separate budget updater to use director `+0x444`; this value is
/// not the live director `+0x448` budget or a Zako spawn permission.
pub fn common_rush_enemy_active_max(difficulty: u32) -> i32 {
    const ROWS: [(u32, i32); 9] = [
        (0, -1),
        (40, -1),
        (100, 15),
        (200, 17),
        (300, 19),
        (400, 22),
        (500, 25),
        (1000, 32),
        (1665, 32),
    ];
    let difficulty = difficulty.min(1665);
    let upper_index = ROWS
        .iter()
        .position(|row| difficulty <= row.0)
        .expect("last Rush row covers capped difficulty");
    let upper = ROWS[upper_index];
    if upper_index == 0 || difficulty == upper.0 {
        upper.1
    } else {
        let lower = ROWS[upper_index - 1];
        let ratio = (difficulty - lower.0) as f32 / (upper.0 - lower.0) as f32;
        interpolate_signed(lower.1, upper.1, ratio)
    }
}

/// Standard EventType 0 / JobType 0–1 individual request count per ordinary
/// queue item, before the game's live active-count and spawn checks.
///
/// The owned 11.3.0 level rows supply `RareFollowerNum`; the game blends this
/// signed integer field by effective difficulty and requests `1 + value`.
/// At most one selected Rare boss is attempted; further attempts use a
/// separate follower ID 0 or 1. The returned total is not a Rare-boss count.
/// A later species-specific branch forces the effective budget to one for
/// ID11 Pillar, so this is the level-row request before that override.
/// This common-row result assumes no later effective-level override and the
/// ordinary queue entry's `+8` flag is zero. A flagged item requests one.
pub fn common_ordinary_item_request_count(difficulty: u32) -> u32 {
    let difficulty = difficulty.min(1665);
    let upper_index = LEVELS
        .iter()
        .position(|row| difficulty <= row.difficulty)
        .expect("last level covers capped difficulty");
    let upper = LEVELS[upper_index];
    let followers = if difficulty == upper.difficulty || upper_index == 0 {
        upper.rare_follower_num
    } else {
        let lower = LEVELS[upper_index - 1];
        let ratio =
            (difficulty - lower.difficulty) as f32 / (upper.difficulty - lower.difficulty) as f32;
        interpolate(lower.rare_follower_num, upper.rare_follower_num, ratio)
    };
    1 + followers
}

pub fn common_weights(difficulty: u32) -> CommonWeights {
    let difficulty = difficulty.min(1665);
    let (none, other) = if difficulty <= 40 {
        (1, 0)
    } else if difficulty >= 100 {
        (24, 1)
    } else {
        let ratio = (difficulty - 40) as f32 / 60.0;
        (interpolate(1, 24, ratio), interpolate(0, 1, ratio))
    };
    CommonWeights {
        water: [1, 3, 1],
        event: [none, other, other, other, other, other, other, other, other],
    }
}

pub fn queue_counts(difficulty: u32, round_index: u8) -> Result<QueueCounts, LevelError> {
    if round_index >= 3 {
        return Err(LevelError::InvalidRound(round_index));
    }
    let difficulty = difficulty.min(1665);
    let upper_index = LEVELS
        .iter()
        .position(|row| difficulty <= row.difficulty)
        .expect("last level covers capped difficulty");
    let upper = LEVELS[upper_index];
    let round = usize::from(round_index);
    if difficulty == upper.difficulty || upper_index == 0 {
        return Ok(QueueCounts {
            wave_change_num: upper.wave_change_num,
            rare_appear_num: upper.rare_appear_num[round],
        });
    }
    let lower = LEVELS[upper_index - 1];
    let ratio =
        (difficulty - lower.difficulty) as f32 / (upper.difficulty - lower.difficulty) as f32;
    Ok(QueueCounts {
        wave_change_num: interpolate(lower.wave_change_num, upper.wave_change_num, ratio),
        rare_appear_num: interpolate(
            lower.rare_appear_num[round],
            upper.rare_appear_num[round],
            ratio,
        ),
    })
}

/// Stock `Round.NormaGoldenIkuraNum` before live-player scaling.
///
/// `source_round_index` is a Round index 0–2, not an arbitrary WAVE index.
/// WAVE 3/4 use source Round 2 only when the live effective array still has
/// the owned three entries in their owned order. The source is interpolated
/// by effective difficulty; later resource replacement is outside this API.
pub fn common_round_quota_base(difficulty: u32, source_round_index: u8) -> Result<i32, LevelError> {
    if source_round_index >= 3 {
        return Err(LevelError::InvalidRound(source_round_index));
    }
    let difficulty = difficulty.min(1665);
    let upper_index = LEVELS
        .iter()
        .position(|row| difficulty <= row.difficulty)
        .expect("last level covers capped difficulty");
    let upper = LEVELS[upper_index];
    let round = usize::from(source_round_index);
    if difficulty == upper.difficulty || upper_index == 0 {
        return Ok(upper.norma_golden_ikura_num[round]);
    }
    let lower = LEVELS[upper_index - 1];
    let ratio =
        (difficulty - lower.difficulty) as f32 / (upper.difficulty - lower.difficulty) as f32;
    Ok(interpolate_signed(
        lower.norma_golden_ikura_num[round],
        upper.norma_golden_ikura_num[round],
        ratio,
    ))
}

/// Source common Round `EnemyActiveMax1stHalf`/`2ndHalf` for 11.3.0.
///
/// `source_round_index` is 0–2, not the WAVE index. In the owned three-Round
/// setting, later WAVE indices 3/4 select source Round 2. This lookup assumes
/// the live effective level still matches the owned common resource.
pub fn common_round_population_limits(
    difficulty: u32,
    source_round_index: u8,
) -> Result<RoundPopulationLimits, LevelError> {
    if source_round_index >= 3 {
        return Err(LevelError::InvalidRound(source_round_index));
    }
    let difficulty = difficulty.min(1665);
    let upper_index = LEVELS
        .iter()
        .position(|row| difficulty <= row.difficulty)
        .expect("last level covers capped difficulty");
    let upper = LEVELS[upper_index];
    let round = usize::from(source_round_index);
    if difficulty == upper.difficulty || upper_index == 0 {
        return Ok(RoundPopulationLimits {
            first_half: upper.enemy_active_max_1st_half[round],
            second_half: upper.enemy_active_max_2nd_half[round],
        });
    }
    let lower = LEVELS[upper_index - 1];
    let ratio =
        (difficulty - lower.difficulty) as f32 / (upper.difficulty - lower.difficulty) as f32;
    Ok(RoundPopulationLimits {
        first_half: interpolate_signed(
            lower.enemy_active_max_1st_half[round],
            upper.enemy_active_max_1st_half[round],
            ratio,
        ),
        second_half: interpolate_signed(
            lower.enemy_active_max_2nd_half[round],
            upper.enemy_active_max_2nd_half[round],
            ratio,
        ),
    })
}
