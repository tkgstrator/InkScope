use salmon_wave_lookahead::rules::{EventContext, event_candidates, tide_candidates};

const WEIGHTS: [u32; 9] = [24, 1, 1, 1, 1, 1, 1, 1, 1];

fn eligible(context: EventContext) -> [bool; 9] {
    event_candidates(WEIGHTS, context).map(|candidate| candidate.eligible)
}

#[test]
fn big_run_and_normal_event_allow_lists_differ() {
    let context = EventContext {
        mode_one: false,
        ban_event_mask: 0,
        global_word: 0x10100,
        previous_event: 0,
    };
    assert_eq!(eligible(context), [true; 9]);
    assert_eq!(
        eligible(EventContext {
            mode_one: true,
            ..context
        }),
        [true, true, false, true, true, true, false, false, false]
    );
}

#[test]
fn stage_ban_skips_zero_and_previous_nonzero_event_is_rejected() {
    let context = EventContext {
        mode_one: false,
        ban_event_mask: (1 << 0) | (1 << 3),
        global_word: 0x10100,
        previous_event: 5,
    };
    assert_eq!(
        eligible(context),
        [true, true, true, false, true, false, true, true, true]
    );
}

#[test]
fn global_word_is_shifted_before_threshold_test() {
    let context = EventContext {
        mode_one: false,
        ban_event_mask: 0,
        global_word: 0x100ff,
        previous_event: 0,
    };
    assert_eq!(
        eligible(context),
        [true, true, true, true, false, true, true, false, false]
    );
    assert_eq!(
        eligible(EventContext {
            global_word: 0x10100,
            ..context
        }),
        [true; 9]
    );
}

#[test]
fn event_dependent_tide_zero_exclusion_uses_mode() {
    let water = [1, 3, 1];
    for event in [1, 3] {
        assert!(!tide_candidates(water, false, event)[0].eligible);
        assert!(tide_candidates(water, true, event)[0].eligible);
    }
    for event in [2, 8] {
        assert!(!tide_candidates(water, false, event)[0].eligible);
        assert!(!tide_candidates(water, true, event)[0].eligible);
    }
    for event in [0, 4, 5, 6, 7] {
        assert!(tide_candidates(water, false, event)[0].eligible);
        assert!(tide_candidates(water, true, event)[0].eligible);
    }
    assert_eq!(
        tide_candidates(water, false, 1).map(|candidate| candidate.weight),
        water
    );
    assert!(tide_candidates(water, false, 1)[1].eligible);
    assert!(tide_candidates(water, false, 1)[2].eligible);
}
