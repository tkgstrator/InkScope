use salmon_wave_lookahead::relay::{
    BoxTransform, DropPointSource, DropRecordSnapshot, InnerRingSnapshot, OuterRingSnapshot,
    RelayError, RelayRoute, RelaySnapshot, initial_drop_rng_state, route_relay, step_drop,
};

#[test]
fn relay_constructor_distinguishes_missing_shared_state_from_present_zero() {
    assert_eq!(
        initial_drop_rng_state(None),
        [0xf240_2c0c, 0x734e_68ed, 0xe289_b11f, 0xb7de_dc10]
    );
    assert_eq!(
        initial_drop_rng_state(Some([0; 4])),
        [0xa147_7bb1, 0xc84b_98a1, 0xfeef_e9ed, 0xc659_a8ea]
    );
}

#[test]
fn relay_constructor_normal_seed_matches_elf_rounds() {
    assert_eq!(
        initial_drop_rng_state(Some([1, 2, 3, 4])),
        [0xa25e_51a0, 0x1c53_e6ec, 0xf422_671f, 0x5c84_aa10]
    );
}

fn box_at(x: f32, y: f32, z: f32, tag: u32) -> BoxTransform {
    BoxTransform([
        x.to_bits(),
        y.to_bits(),
        z.to_bits(),
        tag,
        tag + 1,
        tag + 2,
        tag + 3,
        tag + 4,
        tag + 5,
        tag + 6,
        tag + 7,
        tag + 8,
    ])
}

fn source(tag: u32) -> DropPointSource {
    DropPointSource([
        0xabc0_0000 + tag,
        0x1234_5678,
        0xdead_0000 + tag,
        0x3f80_0000,
        0x7fc0_0000,
        0x8000_0000,
        tag,
        tag + 1,
        tag + 2,
        tag + 3,
        tag + 4,
        tag + 5,
    ])
}

fn snapshot() -> RelaySnapshot {
    RelaySnapshot {
        tide: 0,
        box_groups: [vec![box_at(0.0, 0.0, 0.0, 10)], vec![], vec![]],
        drop_group_selector: 0,
        outer_cursor: 0,
        outer_groups: [
            Some(OuterRingSnapshot {
                capacity: 1,
                start: 0,
                count: 1,
                records: vec![DropRecordSnapshot { inner_count: 3 }],
            }),
            None,
            None,
        ],
        mode: 0,
        nonzero_gate: 0,
        reference_xyz: None,
    }
}

fn ready(input: &RelaySnapshot) -> salmon_wave_lookahead::relay::DropReady {
    match route_relay(input).unwrap() {
        RelayRoute::DropReady(state) => state,
        other => panic!("expected drop-ready route, got {other:?}"),
    }
}

fn ring(capacity: i32, start: i32) -> InnerRingSnapshot {
    InnerRingSnapshot {
        capacity,
        start,
        sources: (0..capacity as u32).map(source).collect(),
    }
}

#[test]
fn early_guards_skip_unreached_snapshots_and_rng() {
    let mut input = snapshot();
    input.box_groups[0].clear();
    input.outer_groups[0] = None;
    assert_eq!(
        route_relay(&input),
        Ok(RelayRoute::NoAttempt { mode_after: 0 })
    );

    input.box_groups[0].push(box_at(0.0, 0.0, 0.0, 10));
    input.outer_groups[0] = snapshot().outer_groups[0].clone();
    input.outer_groups[0].as_mut().unwrap().records[0].inner_count = 0;
    input.mode = 1;
    assert_eq!(
        route_relay(&input),
        Ok(RelayRoute::NoAttempt { mode_after: 1 })
    );

    input.outer_groups[0].as_mut().unwrap().records[0].inner_count = 1;
    assert_eq!(
        route_relay(&input),
        Ok(RelayRoute::NoAttempt { mode_after: 1 })
    );
}

#[test]
fn outer_cursor_equal_or_greater_than_count_is_an_error() {
    let mut input = snapshot();
    input.outer_cursor = 1;
    assert_eq!(
        route_relay(&input),
        Err(RelayError::OuterCursorOutOfRange {
            cursor: 1,
            count: 1
        })
    );
    input.outer_cursor = 2;
    assert_eq!(
        route_relay(&input),
        Err(RelayError::OuterCursorOutOfRange {
            cursor: 2,
            count: 1
        })
    );
}

#[test]
fn invalid_reached_ring_is_error_but_unreached_ring_is_ignored() {
    let mut input = snapshot();
    input.outer_groups[0].as_mut().unwrap().capacity = 0;
    assert!(matches!(
        route_relay(&input),
        Err(RelayError::InvalidOuterRing(_))
    ));
    input.box_groups[0].clear();
    assert_eq!(
        route_relay(&input),
        Ok(RelayRoute::NoAttempt { mode_after: 0 })
    );
}

#[test]
fn out_of_range_tide_and_group_selector_use_group_zero() {
    let mut input = snapshot();
    input.tide = u32::MAX;
    input.drop_group_selector = u32::MAX;
    let state = ready(&input);
    assert_eq!(state.outer_group_index, 0);
    assert_eq!(state.outer_physical_index, 0);
}

#[test]
fn box_gate_clears_mode_and_singleton_skips_reference() {
    let mut input = snapshot();
    input.mode = 7;
    input.nonzero_gate = 1;
    let want = input.box_groups[0][0];
    assert_eq!(
        route_relay(&input),
        Ok(RelayRoute::BoxCandidate {
            mode_after: 0,
            box_index: 0,
            source: want,
        })
    );
    assert_eq!(input.mode, 7);
}

#[test]
fn box_selection_keeps_first_strict_minimum_and_nan_first() {
    let mut input = snapshot();
    input.mode = 1;
    input.nonzero_gate = 1;
    input.reference_xyz = Some([0.0, 0.0, 0.0]);
    input.box_groups[0] = vec![
        box_at(2.0, 0.0, 0.0, 10),
        box_at(1.0, 0.0, 0.0, 20),
        box_at(-1.0, 0.0, 0.0, 30),
    ];
    assert!(matches!(
        route_relay(&input),
        Ok(RelayRoute::BoxCandidate { box_index: 1, .. })
    ));

    input.box_groups[0] = vec![box_at(f32::NAN, 0.0, 0.0, 10), box_at(0.0, 0.0, 0.0, 20)];
    assert!(matches!(
        route_relay(&input),
        Ok(RelayRoute::BoxCandidate { box_index: 0, .. })
    ));
    input.box_groups[0].swap(0, 1);
    assert!(matches!(
        route_relay(&input),
        Ok(RelayRoute::BoxCandidate { box_index: 0, .. })
    ));
}

#[test]
fn box_selection_uses_fused_f32_distance_at_rounding_boundary() {
    let mut input = snapshot();
    input.mode = 1;
    input.nonzero_gate = 1;
    input.reference_xyz = Some([0.0, 0.0, 0.0]);
    // Both distances round to 0x40248ade with separate mul/add operations.
    // Fused x*x+y*y gives 0x40248adf then 0x40248add, so the second wins.
    input.box_groups[0] = vec![
        box_at(
            f32::from_bits(0x3fb8_9463),
            f32::from_bits(0x3f33_7ac4),
            0.0,
            10,
        ),
        box_at(
            f32::from_bits(0x3f52_b92d),
            f32::from_bits(0x3fb0_2149),
            0.0,
            20,
        ),
    ];
    assert!(matches!(
        route_relay(&input),
        Ok(RelayRoute::BoxCandidate { box_index: 1, .. })
    ));
}

#[test]
fn outer_ring_wrap_selects_live_record_at_physical_zero() {
    let mut input = snapshot();
    input.outer_groups[0] = Some(OuterRingSnapshot {
        capacity: 3,
        start: 2,
        count: 2,
        records: vec![
            DropRecordSnapshot { inner_count: 4 },
            DropRecordSnapshot { inner_count: 0 },
            DropRecordSnapshot { inner_count: 0 },
        ],
    });
    input.outer_cursor = 1;
    let state = ready(&input);
    assert_eq!(state.outer_physical_index, 0);
    assert_eq!(state.remaining, 4);
}

#[test]
fn multi_box_selection_requires_reference_only_after_gate() {
    let mut input = snapshot();
    input.box_groups[0].push(box_at(1.0, 0.0, 0.0, 20));
    input.mode = 1;
    input.nonzero_gate = 0;
    assert_eq!(
        route_relay(&input),
        Ok(RelayRoute::NoAttempt { mode_after: 1 })
    );
    input.nonzero_gate = 1;
    assert_eq!(route_relay(&input), Err(RelayError::MissingReference));
}

#[test]
fn fixed_two_draw_vector_preserves_pool_and_wraps_inner_ring() {
    let input = snapshot();
    let first = step_drop(ready(&input), [1, 2, 3, 4], &ring(5, 4)).unwrap();
    assert_eq!(first.selected_source_index, 0);
    assert_eq!(first.inner_physical_index, 4);
    assert_eq!(first.source, source(4));
    assert_eq!(first.rng_before_dispatch, [2, 3, 4, 0x0000_080d]);
    assert_eq!(first.next.remaining, 2);
    assert_eq!(first.target_slot, 0);

    let second = step_drop(first.next, first.rng_before_dispatch, &ring(5, 4)).unwrap();
    assert_eq!(second.selected_source_index, 2);
    assert_eq!(second.inner_physical_index, 1);
    assert_eq!(second.source, source(1));
    assert_eq!(second.rng_before_dispatch, [3, 4, 0x0000_080d, 0x0000_181f]);
    assert_eq!(second.target_slot, 1);
}

#[test]
fn exhausted_count_falls_back_to_zero_and_can_repeat() {
    let mut input = snapshot();
    input.outer_groups[0].as_mut().unwrap().records[0].inner_count = 1;
    let first = step_drop(
        ready(&input),
        [0x1234_5678, 0x9abc_def0, 0x1357_9bdf, 0x2468_ace0],
        &ring(4, 2),
    )
    .unwrap();
    assert_eq!(first.selected_source_index, 0);
    assert_eq!(first.next.remaining, 0);
    let second = step_drop(first.next, first.rng_before_dispatch, &ring(4, 2)).unwrap();
    assert_eq!(second.selected_source_index, 0);
    assert_eq!(second.inner_physical_index, 2);
    assert_eq!(
        second.rng_before_dispatch,
        [0x1357_9bdf, 0x2468_ace0, 0x945f_b983, 0xe868_bea6]
    );
    assert_eq!(second.next.remaining, -1);
}

#[test]
fn large_count_clamps_to_32_without_clamping_inner_ring_capacity() {
    let mut input = snapshot();
    input.outer_groups[0].as_mut().unwrap().records[0].inner_count = 40;
    let first = step_drop(
        ready(&input),
        [0xffff_ffff, 0x8000_0000, 0x0123_4567, 0x89ab_cdef],
        &ring(40, 37),
    )
    .unwrap();
    assert_eq!(first.selected_source_index, 17);
    assert_eq!(first.inner_physical_index, 14);
    assert_eq!(first.next.remaining, 31);
    assert_eq!(
        first.rng_before_dispatch,
        [0x8000_0000, 0x0123_4567, 0x89ab_cdef, 0x89ab_db22]
    );
}

#[test]
fn second_step_accepts_refreshed_rng_and_ring_snapshot() {
    let first = step_drop(ready(&snapshot()), [1, 2, 3, 4], &ring(5, 0)).unwrap();
    let mut refreshed = ring(5, 3);
    refreshed.sources[0] = source(99);
    let second = step_drop(first.next, [5, 6, 7, 8], &refreshed).unwrap();
    assert_eq!(second.selected_source_index, 2);
    assert_eq!(second.inner_physical_index, 0);
    assert_eq!(second.source, source(99));
    assert_eq!(second.rng_before_dispatch, [6, 7, 8, 0x0000_2825]);
}

#[test]
fn drop_source_retains_pointer_metadata_and_raw_float_bits() {
    let first = step_drop(ready(&snapshot()), [1, 2, 3, 4], &ring(3, 0)).unwrap();
    assert_eq!(first.source.0[0], 0xabc0_0000);
    assert_eq!(first.source.0[1], 0x1234_5678);
    assert_eq!(first.source.0[2], 0xdead_0000);
    assert_eq!(first.source.0[4], 0x7fc0_0000);
    assert_eq!(first.source.0[5], 0x8000_0000);
}

#[test]
fn reached_invalid_inner_ring_fails_before_consuming_step_state() {
    let state = ready(&snapshot());
    let mut inner = ring(3, 0);
    inner.start = 3;
    assert!(matches!(
        step_drop(state, [1, 2, 3, 4], &inner),
        Err(RelayError::InvalidInnerRing(_))
    ));
    assert_eq!(state.remaining, 3);
}
