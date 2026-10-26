use std::collections::BTreeSet;

use ocean_calc::weapon_catalog::{
    EffectivePoolSource, PoolResolutionError, SourcePoolKind, normal_source_candidates,
    resolve_normal_pool, resolve_third_pool, third_source_candidates,
};
use ocean_calc::{
    rng::XorShift128,
    weapons::{
        HelperNormalPool, StandardRandomInput, WeaponRandomPool, draw_standard_random_node_value,
        draw_third_pool_node_value,
    },
};

#[test]
fn signed_season_caps_filter_each_builder_source_set() {
    for (cap, normal, third) in [(-1, 0, 0), (0, 51, 0), (1, 52, 5), (8, 63, 8)] {
        assert_eq!(
            normal_source_candidates(cap).entries.len(),
            normal,
            "normal cap {cap}"
        );
        assert_eq!(
            third_source_candidates(cap).entries.len(),
            third,
            "third cap {cap}"
        );
    }
    let normal = normal_source_candidates(0);
    let third = third_source_candidates(1);
    assert_ne!(normal.entries.len(), third.entries.len());
}

#[test]
fn source_order_and_provenance_do_not_claim_effective_runtime_pool_order() {
    let normal = normal_source_candidates(8);
    assert_eq!(normal.kind, SourcePoolKind::Normal);
    assert_eq!(normal.conditional_empty_fallback_id, 20040);
    assert_eq!(
        normal.source_sha256,
        "fdfddc6bce0ac83460eba37cbafcbf0e4afd2548faf5664dc8f3a47c65ac7681"
    );
    assert_eq!(
        normal
            .entries
            .iter()
            .take(6)
            .map(|entry| entry.candidate_id)
            .collect::<Vec<_>>(),
        [20250, 20230, 20240, 20220, 20210, 20260]
    );
    let first = normal.entries[0];
    assert_eq!(first.source_index, 2);
    assert_eq!(first.source_row_id, "Blaster_LightLong_00");
    assert_eq!(first.source_id, 250);
    assert_eq!(first.target_row_id, Some("Blaster_LightLong_Coop"));
    assert_eq!(first.candidate_id, 20250);
    assert!(
        normal
            .entries
            .windows(2)
            .all(|pair| pair[0].source_index < pair[1].source_index)
    );
    assert_eq!(
        normal
            .entries
            .iter()
            .map(|entry| entry.candidate_id)
            .collect::<BTreeSet<_>>()
            .len(),
        63
    );
}

#[test]
fn third_pool_sources_remain_distinct_from_conditional_fallback() {
    let cap_zero = third_source_candidates(0);
    assert_eq!(cap_zero.kind, SourcePoolKind::ThirdRare);
    assert!(cap_zero.entries.is_empty());
    assert_eq!(cap_zero.conditional_empty_fallback_id, 20900);
    let cap_eight = third_source_candidates(8);
    assert_eq!(
        cap_eight
            .entries
            .iter()
            .map(|entry| entry.candidate_id)
            .collect::<Vec<_>>(),
        [20900, 22900, 25900, 21900, 28900, 26900, 23900, 27900]
    );
    assert!(
        cap_eight
            .entries
            .iter()
            .all(|entry| entry.target_row_id.is_none())
    );
    assert!(
        cap_eight
            .entries
            .windows(2)
            .all(|pair| pair[0].source_index < pair[1].source_index)
    );
}

#[test]
fn resolved_normal_pool_uses_signed_source_id_order_and_lookup_ids() {
    let mut seen = Vec::new();
    let pool = resolve_normal_pool(0, 3, |row| {
        seen.push((row.source_id, row.source_index));
        Ok::<_, ()>(Some(i32::from(row.candidate_id + 1)))
    })
    .unwrap();
    assert_eq!(pool.kind, SourcePoolKind::Normal);
    assert_eq!(
        seen[..5],
        [(0, 245), (10, 194), (20, 231), (30, 182), (40, 219)]
    );
    assert_eq!(seen.len(), 51, "lookups continue after the capacity fills");
    assert_eq!(pool.draw_ids, [20001, 20011, 20021]);
    assert_eq!(pool.stored_ids, [20001, 20011, 20021]);
    assert!(matches!(pool.sources[0], EffectivePoolSource::Source(row) if row.source_index == 245));
    assert_eq!(pool.draw_ids.len(), pool.sources.len());
    assert_eq!(pool.stored_ids.len(), pool.sources.len());
}

#[test]
fn normal_pool_preserves_full_lookup_id_but_exposes_low16_for_draw() {
    let pool = resolve_normal_pool(0, 2, |row| {
        Ok::<_, ()>(Some(if row.source_id == 0 {
            0x1_0001
        } else {
            0x2_0001
        }))
    })
    .unwrap();
    assert_eq!(pool.stored_ids, [0x1_0001, 0x2_0001]);
    assert_eq!(pool.draw_ids, [1, 1]);
    assert!(matches!(pool.sources[0], EffectivePoolSource::Source(row) if row.source_id == 0));
    assert!(matches!(pool.sources[1], EffectivePoolSource::Source(row) if row.source_id == 10));
}

#[test]
fn normal_lookup_runs_before_zero_and_one_capacity_checks() {
    let mut cap_zero_calls = Vec::new();
    let zero = resolve_normal_pool(0, 0, |row| {
        cap_zero_calls.push(row.source_id);
        Ok::<_, ()>(Some(i32::from(row.candidate_id)))
    })
    .unwrap();
    assert!(zero.draw_ids.is_empty());
    assert_eq!(cap_zero_calls.len(), 51);
    assert_eq!(cap_zero_calls[..3], [0, 10, 20]);

    let mut cap_one_calls = Vec::new();
    let one = resolve_normal_pool(0, 1, |row| {
        cap_one_calls.push(row.source_id);
        Ok::<_, ()>((row.source_id != 0).then_some(555_i32))
    })
    .unwrap();
    assert_eq!(one.draw_ids, [555]);
    assert!(matches!(one.sources[0], EffectivePoolSource::Source(row) if row.source_id == 10));
    assert_eq!(cap_one_calls.len(), 51);
}

#[test]
fn known_null_skips_but_unknown_lookup_stops_with_source_context() {
    let mut seen = Vec::new();
    let result = resolve_normal_pool(0, 2, |row| {
        seen.push(row.source_id);
        match row.source_id {
            0 => Ok(None),
            10 => Err("lookup unknown"),
            _ => Ok(Some(i32::from(row.candidate_id))),
        }
    });
    assert!(
        matches!(result, Err(PoolResolutionError::UnknownLookup { source, error: "lookup unknown" }) if source.source_id == 10 && source.source_index == 194)
    );
    assert_eq!(seen, [0, 10]);
}

#[test]
fn duplicate_resolved_ids_keep_separate_source_provenance() {
    let pool = resolve_normal_pool(0, 2, |_| Ok::<_, ()>(Some(777))).unwrap();
    assert_eq!(pool.draw_ids, [777, 777]);
    assert_eq!(pool.stored_ids, [777, 777]);
    assert!(matches!(pool.sources[0], EffectivePoolSource::Source(row) if row.source_id == 0));
    assert!(matches!(pool.sources[1], EffectivePoolSource::Source(row) if row.source_id == 10));
}

#[test]
fn third_pool_uses_call_time_signed_cap_capacity_and_sorted_order() {
    let pool = resolve_third_pool(8, 3);
    assert_eq!(pool.kind, SourcePoolKind::ThirdRare);
    assert_eq!(pool.draw_ids, [20900, 21900, 22900]);
    assert_eq!(pool.stored_ids, [20900, 21900, 22900]);
    assert!(matches!(pool.sources[1], EffectivePoolSource::Source(row) if row.source_index == 118));
    assert_eq!(
        resolve_third_pool(1, 8).draw_ids,
        [20900, 22900, 23900, 26900, 27900]
    );
}

#[test]
fn empty_positive_capacity_uses_fallback_and_nonpositive_capacity_does_not() {
    let normal = resolve_normal_pool(-1, 1, |_| -> Result<Option<i32>, ()> {
        panic!("no season eligible rows")
    })
    .unwrap();
    assert_eq!(normal.draw_ids, [20040]);
    assert_eq!(normal.stored_ids, [20040]);
    assert_eq!(normal.sources, [EffectivePoolSource::Fallback]);
    let all_null = resolve_normal_pool(0, 2, |_| Ok::<_, ()>(None)).unwrap();
    assert_eq!(all_null.draw_ids, [20040]);
    assert_eq!(all_null.sources, [EffectivePoolSource::Fallback]);
    assert_eq!(resolve_third_pool(0, 1).draw_ids, [20900]);
    assert_eq!(resolve_third_pool(0, 1).stored_ids, [20900]);
    assert_eq!(
        resolve_third_pool(0, 1).sources,
        [EffectivePoolSource::Fallback]
    );
    for capacity in [0, -1] {
        assert!(resolve_third_pool(0, capacity).draw_ids.is_empty());
        assert!(
            resolve_normal_pool(-1, capacity, |_| -> Result<Option<i32>, ()> {
                panic!("no eligible rows")
            })
            .unwrap()
            .draw_ids
            .is_empty()
        );
    }
}

#[test]
fn resolved_pools_feed_existing_weapon_draw_helpers() {
    let normal =
        resolve_normal_pool(0, 1, |row| Ok::<_, ()>(Some(i32::from(row.candidate_id)))).unwrap();
    let third = resolve_third_pool(1, 1);
    let mut normal_rng = XorShift128::from_words([1, 2, 3, 4]);
    let selected = draw_standard_random_node_value(
        &mut normal_rng,
        StandardRandomInput {
            source_id: -1,
            threshold: 0.0,
            normal: HelperNormalPool {
                signed_count: normal.draw_ids.len() as i32,
                ids: &normal.draw_ids,
            },
            scene_rare: None,
        },
        |_| panic!("normal draw has no scene-rare lookup"),
    )
    .unwrap();
    assert_eq!(selected.pool, WeaponRandomPool::HelperNormal);
    assert_eq!(selected.resolved_id, i32::from(normal.draw_ids[0]));

    let mut third_rng = XorShift128::from_words([1, 2, 3, 4]);
    let selected_third =
        draw_third_pool_node_value(&mut third_rng, third.draw_ids.len() as i32, &third.draw_ids)
            .unwrap();
    assert_eq!(selected_third.resolved_id, 20900);
}
