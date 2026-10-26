use salmon_wave_lookahead::{
    SeedWords,
    rng::XorShift128,
    weapon_catalog::{EffectivePoolSource, EffectiveWeaponPool, SourcePoolKind},
    weapon_preprocess::{PreprocessError, PreprocessShape},
    weapon_preview::{WeaponPreviewError, predict_weapon_grid},
    weapons::{
        ThirdPoolError, WeaponCandidateRing, WeaponGridBuildError, WeaponGridBuildInput,
        WeaponGridSources,
    },
};

fn pool(kind: SourcePoolKind, ids: &[i32]) -> EffectiveWeaponPool {
    EffectiveWeaponPool {
        kind,
        stored_ids: ids.to_vec(),
        draw_ids: ids.iter().map(|&id| id as u16).collect(),
        sources: vec![EffectivePoolSource::Fallback; ids.len()],
        source_sha256: "synthetic-fixture",
    }
}

fn build_input<'a>(
    ring: &'a [i32],
    normal: &'a EffectiveWeaponPool,
    third: &'a EffectiveWeaponPool,
    override_three: bool,
    ready: bool,
) -> WeaponGridBuildInput<'a> {
    WeaponGridBuildInput {
        ready,
        candidates: WeaponCandidateRing {
            physical_ids: ring,
            start: 0,
            count: ring.len(),
        },
        wave_three_override: override_three,
        normal,
        third,
        threshold: 0.0,
        scene_rare: None,
    }
}

#[test]
fn five_wave_weapon_preview_keeps_one_rng_across_all_phases() {
    let seed = SeedWords::from_scene([1, 2, 3, 4]);
    let normal = pool(SourcePoolKind::Normal, &[10, 20, 30, 40]);
    let third = pool(SourcePoolKind::ThirdRare, &[]);
    let outcome = predict_weapon_grid(
        seed,
        PreprocessShape::Low { n: 4 },
        build_input(&[10, 20, 30, 40], &normal, &third, false, true),
        |_| panic!("direct members need no asset callback"),
    )
    .unwrap();
    assert_eq!(outcome.preprocess.draws, 3);
    assert_eq!(
        outcome.preprocess.state.as_array(),
        [0xa4ee_fb34, 0x2c93_7a4a, 0xdac7_f347, 0xd48a_c007]
    );
    let built = outcome.grid.unwrap();
    assert_eq!(
        built.pregrid,
        [
            [10, 20, 30, 40],
            [20, 30, 40, 10],
            [30, 40, 10, 20],
            [40, 10, 20, 30],
            [10, 20, 30, 40],
        ]
    );
    assert_eq!(built.rng_after_build, outcome.preprocess.state);
    assert_eq!(
        built.grid,
        [
            [20, 40, 30, 10],
            [40, 20, 10, 30],
            [30, 10, 40, 20],
            [10, 30, 20, 40],
            [40, 20, 10, 30],
        ]
    );
    assert_eq!(
        built.sources,
        WeaponGridSources {
            slot_sources: [1, 3, 2, 0],
            wave_sources: [0, 2, 1, 3, 2],
        }
    );
    assert_eq!(
        built.slot_sequences,
        [
            vec![20, 40, 30, 10],
            vec![40, 20, 10, 30],
            vec![30, 10, 40, 20],
            vec![10, 30, 20, 40],
        ]
    );
    assert_eq!(
        built.final_rng.as_array(),
        [0xd728_25b0, 0xa583_98d2, 0xd386_826b, 0x841e_52f5]
    );
}

#[test]
fn wave_three_override_uses_third_nodes_and_skips_swap_but_consumes_draw() {
    let normal = pool(SourcePoolKind::Normal, &[10, 20, 30, 40]);
    let third = pool(SourcePoolKind::ThirdRare, &[20900]);
    let outcome = predict_weapon_grid(
        SeedWords::from_scene([1, 2, 3, 4]),
        PreprocessShape::Low { n: 4 },
        build_input(&[10, 20, 30, 40], &normal, &third, true, true),
        |_| panic!("direct members need no asset callback"),
    )
    .unwrap();
    let built = outcome.grid.unwrap();
    assert_eq!(built.pregrid[3], [0x0101_51a4; 4]);
    assert_eq!(
        built.rng_after_build.as_array(),
        [0x076e_b6f9, 0xb098_dd74, 0x5520_5deb, 0xd728_25b0]
    );
    assert_eq!(
        built.sources,
        WeaponGridSources {
            slot_sources: [0, 1, 3, 2],
            wave_sources: [1, 0, 2, 3, 1],
        }
    );
    assert_eq!(
        built.grid,
        [
            [20, 30, 10, 40],
            [10, 20, 40, 30],
            [30, 40, 20, 10],
            [0x0101_51a4; 4],
            [20, 30, 10, 40],
        ]
    );
    assert_eq!(
        built.final_rng.as_array(),
        [0x128d_e263, 0xab71_ade8, 0x4c03_f737, 0x3afc_8db8]
    );
}

#[test]
fn skip_and_errors_preserve_the_reached_rng_state() {
    let normal = pool(SourcePoolKind::Normal, &[10, 20, 30, 40]);
    let third = pool(SourcePoolKind::ThirdRare, &[]);
    let seed = SeedWords::from_scene([1, 2, 3, 4]);
    let skipped = predict_weapon_grid(
        seed,
        PreprocessShape::Low { n: 4 },
        build_input(&[10, 20, 30, 40], &normal, &third, false, false),
        |_| panic!("not ready"),
    )
    .unwrap();
    assert_eq!(skipped.preprocess.draws, 3);
    assert_eq!(skipped.grid, None);

    let invalid = predict_weapon_grid(
        seed,
        PreprocessShape::Low { n: 9 },
        build_input(&[10, 20, 30, 40], &normal, &third, false, true),
        |_| panic!("invalid shape"),
    );
    assert_eq!(
        invalid,
        Err(WeaponPreviewError::Preprocess(
            PreprocessError::LowCountTooLarge(9)
        ))
    );

    let failure = predict_weapon_grid(
        seed,
        PreprocessShape::Low { n: 4 },
        build_input(&[-2, 20, 30, 40], &normal, &third, false, true),
        |_| panic!("third path fails first"),
    );
    assert_eq!(
        failure,
        Err(WeaponPreviewError::Build {
            error: WeaponGridBuildError::Third(ThirdPoolError::MissingEntry(0)),
            preprocess: skipped.preprocess,
            rng_at_error: XorShift128::from_words([
                0x2c93_7a4a,
                0xdac7_f347,
                0xd48a_c007,
                0x076e_b6f9
            ]),
        })
    );
}
