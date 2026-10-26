use ocean_calc::{
    SeedWords,
    schedule::{EntryKind, ScheduleBranch, ScheduleError, ScheduleInput, nominal_schedule},
};

fn input(rare_appear_num: u32, branch: ScheduleBranch) -> ScheduleInput {
    ScheduleInput {
        seed: SeedWords::from_scene([1, 2, 3, 4]),
        event_type: 0,
        wave_change_num: 10,
        rare_appear_num,
        assumed_branch: branch,
    }
}

#[test]
fn normal_branch_has_fixed_boundaries_and_seeded_extra_ticks() {
    let entries = nominal_schedule(input(12, ScheduleBranch::Normal)).unwrap();
    let expected = [
        0, 480, 960, 1440, 1920, 2400, 2880, 3360, 3646, 3840, 4176, 4320,
    ];
    assert_eq!(
        entries.iter().map(|entry| entry.tick).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::Base)
            .count(),
        10
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::Extra)
            .count(),
        2
    );
}

#[test]
fn special_branch_does_not_consume_randomness_at_zero_interval() {
    let entries = nominal_schedule(input(12, ScheduleBranch::Special)).unwrap();
    let extras: Vec<_> = entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Extra)
        .map(|entry| entry.tick)
        .collect();
    assert_eq!(extras, [0, 816]);
}

#[test]
fn same_tick_entries_follow_the_games_unstable_heap_order() {
    let mut value = input(4, ScheduleBranch::Special);
    value.wave_change_num = 3;
    let entries = nominal_schedule(value).unwrap();
    assert_eq!(
        entries,
        vec![
            ocean_calc::schedule::NominalEntry {
                tick: 0,
                kind: EntryKind::Extra
            },
            ocean_calc::schedule::NominalEntry {
                tick: 0,
                kind: EntryKind::Base
            },
            ocean_calc::schedule::NominalEntry {
                tick: 2160,
                kind: EntryKind::Base
            },
            ocean_calc::schedule::NominalEntry {
                tick: 4320,
                kind: EntryKind::Base
            },
        ]
    );
}

#[test]
fn fewer_requested_rare_items_leave_the_base_queue_intact() {
    let mut value = input(1, ScheduleBranch::Normal);
    value.wave_change_num = 3;
    let entries = nominal_schedule(value).unwrap();
    assert_eq!(
        entries.iter().map(|entry| entry.tick).collect::<Vec<_>>(),
        [0, 2160, 4320]
    );
    assert!(entries.iter().all(|entry| entry.kind == EntryKind::Base));
}

#[test]
fn difficulty_1665_example_fits_thirty_nominal_entries() {
    let entries = nominal_schedule(input(30, ScheduleBranch::Normal)).unwrap();
    let extras: Vec<_> = entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Extra)
        .map(|entry| entry.tick)
        .collect();
    assert_eq!(entries.len(), 30);
    assert_eq!(
        extras,
        [
            19, 296, 502, 776, 1031, 1155, 1500, 1889, 2220, 2290, 2458, 2688, 2904, 2990, 3646,
            3798, 3813, 4067, 4146, 4176
        ]
    );
}

#[test]
fn unsupported_event_is_rejected_instead_of_predicting() {
    let mut value = input(30, ScheduleBranch::Normal);
    value.event_type = 1;
    assert_eq!(
        nominal_schedule(value),
        Err(ScheduleError::UnsupportedEvent(1))
    );
}

#[test]
fn fog_and_missile_use_the_same_nominal_rare_queue_ticks_for_job_zero_or_one() {
    let ordinary = nominal_schedule(input(12, ScheduleBranch::Normal)).unwrap();
    for event_type in [5, 6] {
        let mut value = input(12, ScheduleBranch::Normal);
        value.event_type = event_type;
        assert_eq!(nominal_schedule(value), Ok(ordinary.clone()));

        value.assumed_branch = ScheduleBranch::Special;
        let extras: Vec<_> = nominal_schedule(value)
            .unwrap()
            .into_iter()
            .filter_map(|entry| (entry.kind == EntryKind::Extra).then_some(entry.tick))
            .collect();
        assert_eq!(extras, [0, 816]);
    }
}

#[test]
fn invalid_counts_and_capacity_are_rejected() {
    let mut value = input(30, ScheduleBranch::Normal);
    value.wave_change_num = 1;
    assert_eq!(nominal_schedule(value), Err(ScheduleError::InvalidCounts));
    value.wave_change_num = 49;
    value.rare_appear_num = 1;
    assert_eq!(
        nominal_schedule(value),
        Err(ScheduleError::CapacityExceeded)
    );
    value.wave_change_num = 10;
    value.rare_appear_num = 49;
    assert_eq!(
        nominal_schedule(value),
        Err(ScheduleError::CapacityExceeded)
    );
}
