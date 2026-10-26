use ocean_calc::{
    private_weapons::{
        PoolProblem, PrivateMainRing, PrivatePool, PrivateWeaponError, PrivateWeaponInput,
        PrivateWeaponSource, predict_private_weapon_rows,
    },
    rng::{SeedWords, XorShift128},
    specials::{PrivateSpecialInput, SpecialLookup, SpecialReorder, predict_private_specials},
    weapon_catalog::{
        EffectivePoolSource, EffectiveWeaponPool, SourcePoolKind, resolve_normal_pool,
    },
};

fn normal_pool(ids: &[i32]) -> EffectiveWeaponPool {
    EffectiveWeaponPool {
        kind: SourcePoolKind::Normal,
        stored_ids: ids.to_vec(),
        draw_ids: ids.iter().map(|id| *id as u16).collect(),
        sources: vec![EffectivePoolSource::Fallback; ids.len()],
        source_sha256: "synthetic test pool",
    }
}

fn post_low_special_rng() -> XorShift128 {
    XorShift128::from_words([0x30ad_ec15, 0x1872_0128, 0x84a3_ecff, 0x9eaf_0a16])
}

fn input<'a>(
    ring: PrivateMainRing<'a>,
    base: &'a EffectiveWeaponPool,
    alternate: &'a EffectiveWeaponPool,
) -> PrivateWeaponInput<'a> {
    PrivateWeaponInput {
        rng_after_specials: post_low_special_rng(),
        builder_ready_confirmed: true,
        scene_main_ring: ring,
        base_pool: base,
        alternate_pool: alternate,
        mode_allows_direct: false,
    }
}

fn ring(physical_ids: &[i32], start: usize, count: usize) -> PrivateMainRing<'_> {
    PrivateMainRing {
        physical_ids,
        start,
        count,
    }
}

#[test]
fn empty_ring_draws_twenty_private_rows_in_row_major_order() {
    let base = normal_pool(&[20040, 20050, 20060]);
    let alternate = normal_pool(&[20040]);
    let prediction = predict_private_weapon_rows(PrivateWeaponInput {
        rng_after_specials: post_low_special_rng(),
        builder_ready_confirmed: true,
        scene_main_ring: PrivateMainRing {
            physical_ids: &[],
            start: 0,
            count: 0,
        },
        base_pool: &base,
        alternate_pool: &alternate,
        mode_allows_direct: false,
    })
    .unwrap();

    assert_eq!(
        prediction.packed_rows,
        [
            [0x14e5c, 0x14e48, 0x14e5c, 0x14e48],
            [0x14e48, 0x14e48, 0x14e52, 0x14e48],
            [0x14e5c, 0x14e48, 0x14e48, 0x14e5c],
            [0x14e48, 0x14e48, 0x14e52, 0x14e5c],
            [0x14e52, 0x14e52, 0x14e5c, 0x14e5c],
        ]
    );
    assert_eq!(prediction.draws, 20);
    assert_eq!(
        prediction.final_rng.as_array(),
        [0x993a_8605, 0x7b93_4c7c, 0xdfb5_5111, 0xe6b0_9ead]
    );
    assert_eq!(
        prediction.sources[0][0],
        PrivateWeaponSource::BaseDraw { index: 2 }
    );
}

#[test]
fn rotated_ring_repeats_logical_slots_per_row_with_ten_draws() {
    let base = normal_pool(&[20040, 20050, 20060]);
    let alternate = normal_pool(&[20040]);
    let state = input(ring(&[25000, -1, 20040], 2, 3), &base, &alternate);
    let prediction = predict_private_weapon_rows(state).unwrap();
    assert_eq!(
        prediction.packed_rows,
        [
            [20040, 20040, 0x14e5c, 0x14e48],
            [20040, 20040, 0x14e5c, 0x14e48],
            [20040, 20040, 0x14e48, 0x14e48],
            [20040, 20040, 0x14e52, 0x14e48],
            [20040, 20040, 0x14e5c, 0x14e48],
        ]
    );
    assert_eq!(prediction.draws, 10);
    assert_eq!(
        prediction.final_rng.as_array(),
        [0x7ce8_54e4, 0x09b2_c03d, 0xf40c_91eb, 0x1587_9746]
    );
    assert_eq!(
        prediction.sources[0][0],
        PrivateWeaponSource::DirectAlternatePool
    );
    assert_eq!(prediction.sources[0][1], PrivateWeaponSource::BaseFallback);
    assert_eq!(
        prediction.sources[0][2],
        PrivateWeaponSource::BaseDraw { index: 2 }
    );
}

#[test]
fn mode_one_or_two_accepts_positive_ids_without_alternate_membership() {
    let base = normal_pool(&[20040, 20050, 20060]);
    let alternate = normal_pool(&[20040]);
    let mut state = input(ring(&[25000, -1, 20040], 2, 3), &base, &alternate);
    state.mode_allows_direct = true;
    let prediction = predict_private_weapon_rows(state).unwrap();
    for row in prediction.packed_rows {
        assert_eq!(&row[..2], &[20040, 25000]);
    }
    assert_eq!(prediction.sources[0][1], PrivateWeaponSource::DirectMode);
    assert_eq!(prediction.draws, 10);
    assert_eq!(
        prediction.final_rng.as_array(),
        [0x7ce8_54e4, 0x09b2_c03d, 0xf40c_91eb, 0x1587_9746]
    );
}

#[test]
fn alternate_membership_compares_full_thirty_two_bits() {
    let elevated = 0x0001_4e48;
    let base = normal_pool(&[20050]);
    let alternate = normal_pool(&[20040]);
    let ids = [elevated];
    let state = input(ring(&ids, 0, 1), &base, &alternate);
    let miss = predict_private_weapon_rows(state).unwrap();
    assert!(miss.packed_rows.iter().all(|row| row[0] == 20050));
    assert_eq!(miss.sources[0][0], PrivateWeaponSource::BaseFallback);

    let matching_alternate = normal_pool(&[elevated]);
    let hit =
        predict_private_weapon_rows(input(ring(&ids, 0, 1), &base, &matching_alternate)).unwrap();
    assert!(hit.packed_rows.iter().all(|row| row[0] == 20040));
    assert_eq!(hit.sources[0][0], PrivateWeaponSource::DirectAlternatePool);
}

#[test]
fn ring_wrap_uses_physical_capacity_even_when_count_is_shorter() {
    let base = normal_pool(&[20060]);
    let alternate = normal_pool(&[20040, 25000]);
    let prediction = predict_private_weapon_rows(input(
        ring(&[20040, 20050, 20060, 25000], 3, 2),
        &base,
        &alternate,
    ))
    .unwrap();
    assert!(
        prediction
            .packed_rows
            .iter()
            .all(|row| row[0] == 25000 && row[1] == 20040)
    );
    assert_eq!(
        prediction.sources[0][0],
        PrivateWeaponSource::DirectAlternatePool
    );
    assert_eq!(
        prediction.sources[0][1],
        PrivateWeaponSource::DirectAlternatePool
    );
    assert_eq!(prediction.draws, 10);
}

#[test]
fn unconfirmed_builder_and_invalid_ring_are_typed_errors() {
    let base = normal_pool(&[20040]);
    let alternate = normal_pool(&[]);
    let mut state = input(ring(&[], 0, 0), &base, &alternate);
    state.builder_ready_confirmed = false;
    assert_eq!(
        predict_private_weapon_rows(state).unwrap_err(),
        PrivateWeaponError::BuilderNotConfirmed
    );
    state.builder_ready_confirmed = true;
    for malformed in [
        ring(&[1, 2, 3, 4, 5], 0, 4),
        ring(&[1, 2, 3, 4], 4, 4),
        ring(&[1, 2, 3], 0, 4),
        ring(&[], 1, 0),
        ring(&[], 0, 1),
    ] {
        state.scene_main_ring = malformed;
        assert_eq!(
            predict_private_weapon_rows(state).unwrap_err(),
            PrivateWeaponError::InvalidRing {
                capacity: malformed.physical_ids.len(),
                start: malformed.start,
                count: malformed.count,
            }
        );
    }
}

#[test]
fn malformed_effective_pools_are_rejected_before_prediction() {
    let base = normal_pool(&[20040]);
    let alternate = normal_pool(&[]);
    let mut wrong_kind = base.clone();
    wrong_kind.kind = SourcePoolKind::ThirdRare;
    assert_eq!(
        predict_private_weapon_rows(input(ring(&[], 0, 0), &wrong_kind, &alternate)).unwrap_err(),
        PrivateWeaponError::InvalidPool {
            pool: PrivatePool::Base,
            problem: PoolProblem::WrongKind(SourcePoolKind::ThirdRare),
        }
    );

    let mut short_draws = base.clone();
    short_draws.draw_ids.clear();
    assert_eq!(
        predict_private_weapon_rows(input(ring(&[], 0, 0), &short_draws, &alternate)).unwrap_err(),
        PrivateWeaponError::InvalidPool {
            pool: PrivatePool::Base,
            problem: PoolProblem::LengthMismatch,
        }
    );
    let mut short_sources = base.clone();
    short_sources.sources.clear();
    assert_eq!(
        predict_private_weapon_rows(input(ring(&[], 0, 0), &short_sources, &alternate))
            .unwrap_err(),
        PrivateWeaponError::InvalidPool {
            pool: PrivatePool::Base,
            problem: PoolProblem::LengthMismatch,
        }
    );
    let mut wrong_draw = base.clone();
    wrong_draw.draw_ids[0] = 20050;
    assert_eq!(
        predict_private_weapon_rows(input(ring(&[], 0, 0), &wrong_draw, &alternate)).unwrap_err(),
        PrivateWeaponError::InvalidPool {
            pool: PrivatePool::Base,
            problem: PoolProblem::IdMismatch(0),
        }
    );
    let mut too_many = normal_pool(&vec![20040; 257]);
    assert_eq!(
        predict_private_weapon_rows(input(ring(&[], 0, 0), &too_many, &alternate)).unwrap_err(),
        PrivateWeaponError::InvalidPool {
            pool: PrivatePool::Base,
            problem: PoolProblem::TooManyEntries(257),
        }
    );
    too_many = base;
    let mut bad_alternate = alternate;
    bad_alternate.draw_ids.push(20040);
    assert_eq!(
        predict_private_weapon_rows(input(ring(&[], 0, 0), &too_many, &bad_alternate)).unwrap_err(),
        PrivateWeaponError::InvalidPool {
            pool: PrivatePool::Alternate,
            problem: PoolProblem::LengthMismatch,
        }
    );
}

#[test]
fn base_pool_is_required_only_when_taken_branch_reads_it() {
    let empty = normal_pool(&[]);
    let alternate = normal_pool(&[]);
    let direct = predict_private_weapon_rows(PrivateWeaponInput {
        mode_allows_direct: true,
        ..input(ring(&[1, 2, 3, 4], 0, 4), &empty, &alternate)
    })
    .unwrap();
    assert!(direct.packed_rows.iter().all(|row| *row == [1, 2, 3, 4]));
    assert_eq!(direct.draws, 0);

    let fallback =
        predict_private_weapon_rows(input(ring(&[1, 2, 3, 4], 0, 4), &empty, &alternate));
    assert_eq!(
        fallback.unwrap_err(),
        PrivateWeaponError::BaseEntryUnavailable {
            wave_index: 0,
            internal_player_index: 0,
        }
    );
    let draw = predict_private_weapon_rows(input(ring(&[], 0, 0), &empty, &alternate));
    assert_eq!(
        draw.unwrap_err(),
        PrivateWeaponError::BaseEntryUnavailable {
            wave_index: 0,
            internal_player_index: 0,
        }
    );
}

#[test]
fn separately_resolved_type_zero_pools_feed_private_builder() {
    let base = resolve_normal_pool(0, 3, |row| {
        Ok::<_, ()>(match row.source_id {
            0 => Some(20040),
            10 => Some(20050),
            20 => Some(20060),
            _ => None,
        })
    })
    .unwrap();
    let alternate = resolve_normal_pool(-1, 256, |_| Ok::<_, ()>(None)).unwrap();
    assert_eq!(base.stored_ids, [20040, 20050, 20060]);
    assert_eq!(alternate.stored_ids, [20040]);
    let result =
        predict_private_weapon_rows(input(ring(&[25000, -1, 20040], 2, 3), &base, &alternate))
            .unwrap();
    assert_eq!(result.packed_rows[0], [20040, 20040, 0x14e5c, 0x14e48]);
    assert_eq!(result.draws, 10);
}

#[test]
fn special_selection_rng_continues_into_private_main_rows() {
    let seed = SeedWords::from_scene([1, 2, 3, 4]);
    let special = predict_private_specials(
        PrivateSpecialInput {
            seed,
            effective_g: 0,
            low_candidates: &[20006, 20012, 20010, 20009, 20014, 20013, 20007],
            prior_candidates: &[],
            v6_candidates: &[],
            reorder: SpecialReorder::Inactive,
        },
        SpecialLookup::Known,
    )
    .unwrap();
    assert_eq!(special.rng.draws, 6);
    let base = normal_pool(&[20040, 20050, 20060]);
    let alternate = normal_pool(&[20040]);
    let result = predict_private_weapon_rows(PrivateWeaponInput {
        rng_after_specials: special.rng.state,
        ..input(ring(&[25000, -1, 20040], 2, 3), &base, &alternate)
    })
    .unwrap();
    assert_eq!(special.rng.draws + result.draws, 16);
    assert_eq!(result.packed_rows[4], [20040, 20040, 0x14e5c, 0x14e48]);
    assert_eq!(
        result.final_rng.as_array(),
        [0x7ce8_54e4, 0x09b2_c03d, 0xf40c_91eb, 0x1587_9746]
    );
}
