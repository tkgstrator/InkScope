use salmon_wave_lookahead::{
    SeedWords,
    wave::{Candidate, WaveRng, draw_event, draw_tide},
};

const OFF: Candidate = Candidate {
    weight: 0,
    eligible: false,
};

#[test]
fn event_then_tide_share_state_and_tide_keeps_fourth_word() {
    let mut rng = WaveRng::from_seed(SeedWords::from_scene([1, 2, 3, 4]));
    let mut events = [OFF; 9];
    events[0] = Candidate {
        weight: 24,
        eligible: true,
    };
    events[1] = Candidate {
        weight: 1,
        eligible: true,
    };
    events[5] = Candidate {
        weight: 1,
        eligible: true,
    };
    assert_eq!(draw_event(&mut rng, &events), 0);
    assert_eq!(
        rng.as_array(),
        [0xca68_7696, 0x0fce_42db, 0x8e60_d8bb, 0xf04c_af6a]
    );

    let tides = [
        Candidate {
            weight: 1,
            eligible: true,
        },
        Candidate {
            weight: 3,
            eligible: true,
        },
        Candidate {
            weight: 1,
            eligible: true,
        },
    ];
    assert_eq!(draw_tide(&mut rng, &tides), 1);
    assert_eq!(rng.as_array(), [0xf04c_af6a; 4]);
}

#[test]
fn ineligible_candidates_do_not_advance_rng() {
    let mut rng = WaveRng::from_seed(SeedWords::from_scene([1, 2, 3, 4]));
    let mut events = [OFF; 9];
    events[0] = Candidate {
        weight: 24,
        eligible: true,
    };
    events[1] = Candidate {
        weight: 100,
        eligible: false,
    };
    assert_eq!(draw_event(&mut rng, &events), 0);
    assert_eq!(
        rng.as_array(),
        [0xf7ce_dc2d, 0xe5b3_7c29, 0xca68_7696, 0x0fce_42db]
    );
}

#[test]
fn no_positive_candidates_keep_defaults_and_state() {
    let mut rng = WaveRng::from_seed(SeedWords::from_scene([1, 2, 3, 4]));
    let before = rng.as_array();
    assert_eq!(draw_event(&mut rng, &[OFF; 9]), 0);
    assert_eq!(draw_tide(&mut rng, &[OFF; 3]), 1);
    assert_eq!(rng.as_array(), before);
}

#[test]
fn event_result_can_set_tide_eligibility_before_tide_draw() {
    let mut rng = WaveRng::from_seed(SeedWords::from_scene([1, 2, 3, 4]));
    let mut events = [OFF; 9];
    events[1] = Candidate {
        weight: 1,
        eligible: true,
    };
    let event = draw_event(&mut rng, &events);
    assert_eq!(event, 1);
    let tides = if event == 1 {
        [
            OFF,
            Candidate {
                weight: 3,
                eligible: true,
            },
            Candidate {
                weight: 1,
                eligible: true,
            },
        ]
    } else {
        [
            Candidate {
                weight: 1,
                eligible: true,
            },
            OFF,
            OFF,
        ]
    };
    assert_eq!(draw_tide(&mut rng, &tides), 1);
    assert_eq!(
        rng.as_array(),
        [0xca68_7696, 0x0fce_42db, 0x0fce_42db, 0x0fce_42db]
    );
}
