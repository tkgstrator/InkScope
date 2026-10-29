use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ocean-calc"))
        .args(args)
        .output()
        .expect("CLI should launch")
}

#[test]
fn cli_labels_branch_as_an_assumption_and_prints_nominal_ticks() {
    let output = run(&["schedule", "1", "2", "3", "4", "0", "10", "12", "normal"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("assumed branch: normal"));
    assert!(stdout.contains("3646\textra"));
    assert!(stdout.contains("4320\tbase"));
    assert_eq!(stdout.lines().count(), 13);
}

#[test]
fn cli_accepts_hex_words_and_rejects_unknown_branch() {
    let hex = run(&[
        "schedule", "0x1", "0x2", "0x3", "0x4", "0", "10", "12", "normal",
    ]);
    let decimal = run(&["schedule", "1", "2", "3", "4", "0", "10", "12", "normal"]);
    assert!(hex.status.success());
    assert_eq!(hex.stdout, decimal.stdout);

    let invalid = run(&["schedule", "1", "2", "3", "4", "0", "10", "12", "unknown"]);
    assert!(!invalid.status.success());
    assert!(
        String::from_utf8(invalid.stderr)
            .unwrap()
            .contains("branch must be normal or special")
    );
}

#[test]
fn cli_can_derive_queue_counts_from_effective_difficulty() {
    let from_difficulty = run(&[
        "schedule-difficulty",
        "1",
        "2",
        "3",
        "4",
        "0",
        "1665",
        "2",
        "normal",
    ]);
    let explicit = run(&["schedule", "1", "2", "3", "4", "0", "10", "35", "normal"]);
    assert!(
        from_difficulty.status.success(),
        "{}",
        String::from_utf8_lossy(&from_difficulty.stderr)
    );
    assert!(explicit.status.success());
    let derived = String::from_utf8(from_difficulty.stdout).unwrap();
    let explicit = String::from_utf8(explicit.stdout).unwrap();
    assert!(derived.starts_with("effective difficulty supplied: 1665; round: 2; N: 10; R: 35\n"));
    assert_eq!(
        derived.lines().skip(1).collect::<Vec<_>>(),
        explicit.lines().collect::<Vec<_>>()
    );
}

fn wave_rows(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter(|line| line.starts_with("WAVE "))
        .collect()
}

#[test]
fn waves_cli_shows_five_generated_records_and_nominal_queues_for_both_ordinary_job_types() {
    let job_one = run(&[
        "waves",
        "4",
        "2",
        "3",
        "4",
        "Cop_Carousel",
        "1",
        "0",
        "0",
        "0x10100",
    ]);
    assert!(
        job_one.status.success(),
        "{}",
        String::from_utf8_lossy(&job_one.stderr)
    );
    let stdout = String::from_utf8(job_one.stdout).unwrap();
    assert!(stdout.contains("scene seed words"));
    assert!(stdout.contains("JobType=1 (supplied)"));
    assert!(stdout.contains("effective difficulty=0 (supplied)"));
    assert!(stdout.contains("ROMFS source weights/counts (assumed)"));
    assert!(stdout.contains("stage Water/BanEvent from owned ROMFS (assumed)"));
    assert!(stdout.contains("queue count source=min(wave,2)"));
    assert!(stdout.contains("special tide state=inactive (assumed)"));
    assert!(stdout.contains("queue branch=normal (assumed)"));
    assert!(stdout.contains("scenario-to-scene seed transfer is not inferred"));
    let rows = wave_rows(&stdout);
    assert_eq!(rows.len(), 5);
    assert_eq!(
        rows.iter()
            .map(|row| row.split(' ').take(5).collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>(),
        [
            "WAVE 0 generated event=0 tide=0",
            "WAVE 1 generated event=0 tide=0",
            "WAVE 2 generated event=0 tide=2",
            "WAVE 3 generated event=0 tide=1",
            "WAVE 4 generated event=0 tide=1",
        ]
    );
    assert!(rows[0].contains("N=3 R=3 nominal Rare ticks=[0,2160,4320]"));
    assert!(rows[3].contains("source_round=2"));
    assert!(rows[4].contains("source_round=2"));

    let job_zero = run(&[
        "waves",
        "4",
        "2",
        "3",
        "4",
        "Cop_Carousel",
        "0",
        "0",
        "0",
        "0x10100",
    ]);
    assert!(job_zero.status.success());
    let job_zero_stdout = String::from_utf8(job_zero.stdout).unwrap();
    assert!(job_zero_stdout.contains("JobType=0 (supplied)"));
    assert_eq!(wave_rows(&job_zero_stdout), rows);
}

#[test]
fn waves_cli_keeps_generated_values_when_the_queue_event_is_unsupported() {
    let output = run(&[
        "waves",
        "0",
        "2",
        "3",
        "4",
        "Cop_Carousel",
        "1",
        "100",
        "0",
        "0x10100",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(wave_rows(&stdout).len(), 5);
    assert!(
        wave_rows(&stdout)[0].contains("WAVE 0 generated event=3 tide=1 queue=unsupported event 3")
    );
}

#[test]
fn waves_cli_shows_direct_applied_event_and_tide_from_supplied_active_players() {
    // Native event/tide draws give event 6, tide 2 in the first record for
    // this synthetic seed, preserving coverage of the direct tide-zero write.
    let output = run(&[
        "waves",
        "22",
        "2",
        "3",
        "4",
        "Cop_Shakehighway",
        "1",
        "1665",
        "0",
        "0x10100",
        "players=4",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("active-player helper returns 4 at every WAVE (assumed)"));
    assert!(stdout.contains("special-tide flags inactive (assumed)"));
    assert!(stdout.contains("scheduler-time mode does not bypass Rare queue (assumed)"));
    assert!(
        wave_rows(&stdout)[0].contains("generated event=6 tide=2 applied-direct event=6 tide=0")
    );
    assert!(wave_rows(&stdout)[0].contains("nominal Rare ticks="));
    assert!(stdout.contains("later overrides and event handlers are not inferred"));
}

#[test]
fn waves_cli_uses_solo_applied_event_for_queue_and_preserves_generated_tide() {
    let nonzero = run(&[
        "waves",
        "1",
        "2",
        "3",
        "4",
        "Cop_Shakehighway",
        "1",
        "1665",
        "0",
        "0x10100",
        "players=1",
    ]);
    assert!(nonzero.status.success());
    let stdout = String::from_utf8(nonzero.stdout).unwrap();
    assert!(
        wave_rows(&stdout)[0].contains("generated event=4 tide=0 applied-direct event=0 tide=0")
    );
    assert!(wave_rows(&stdout)[0].contains("nominal Rare ticks="));

    let event_six = run(&[
        "waves",
        "22",
        "2",
        "3",
        "4",
        "Cop_Shakehighway",
        "1",
        "1665",
        "0",
        "0x10100",
        "players=1",
    ]);
    assert!(event_six.status.success());
    let stdout = String::from_utf8(event_six.stdout).unwrap();
    assert!(
        wave_rows(&stdout)[0].contains("generated event=6 tide=2 applied-direct event=0 tide=2")
    );
    assert!(wave_rows(&stdout)[0].contains("nominal Rare ticks="));
}

#[test]
fn waves_cli_marks_event_eight_handler_unresolved_unless_solo_forces_event_zero() {
    let base = [
        "waves",
        "25",
        "2",
        "3",
        "4",
        "Cop_Shakehighway",
        "1",
        "1665",
        "0",
        "0x10100",
    ];
    let group = run(&[base.as_slice(), &["players=4"]].concat());
    assert!(group.status.success());
    let stdout = String::from_utf8(group.stdout).unwrap();
    assert!(
        wave_rows(&stdout)[0].contains("generated event=8 tide=1 applied-direct event=8 tide=1")
    );
    assert!(wave_rows(&stdout)[0].contains("handler=unresolved"));
    assert!(!wave_rows(&stdout)[0].contains("nominal Rare ticks="));

    let solo = run(&[base.as_slice(), &["players=1"]].concat());
    assert!(solo.status.success());
    let stdout = String::from_utf8(solo.stdout).unwrap();
    assert!(
        wave_rows(&stdout)[0].contains("generated event=8 tide=1 applied-direct event=0 tide=1")
    );
    assert!(!wave_rows(&stdout)[0].contains("handler=unresolved"));
    assert!(wave_rows(&stdout)[0].contains("nominal Rare ticks="));
}

#[test]
fn waves_cli_rejects_bad_player_suffixes_without_changing_job_zero_score() {
    let base = [
        "waves",
        "1",
        "2",
        "3",
        "4",
        "Cop_Shakehighway",
        "1",
        "1665",
        "0",
        "0x10100",
    ];
    for (suffixes, expected) in [
        (vec!["players=0"], "players must be 1..=4"),
        (vec!["players=5"], "players must be 1..=4"),
        (vec!["players=x"], "players must be 1..=4"),
        (vec!["players=4", "players=4"], "duplicate players"),
        (vec!["unexpected=4"], "unknown waves option"),
    ] {
        let output = run(&[base.as_slice(), suffixes.as_slice()].concat());
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr).unwrap().contains(expected));
    }
    let mut job_zero = base;
    job_zero[6] = "0";
    let output = run(&[job_zero.as_slice(), &["players=4"]].concat());
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("players requires JobType 1")
    );
}

#[test]
fn waves_cli_rejects_unproven_jobtype_and_invalid_live_inputs() {
    let mut args = [
        "waves",
        "4",
        "2",
        "3",
        "4",
        "Cop_Carousel",
        "1",
        "0",
        "0",
        "0x10100",
    ];
    for (index, replacement, expected) in [
        (6, "2", "JobType 0 or 1"),
        (5, "Cop_Unknown", "unknown stage"),
        (7, "1666", "effective difficulty"),
        (8, "2", "is_big_run must be 0 or 1"),
    ] {
        let previous = args[index];
        args[index] = replacement;
        let output = run(&args);
        assert!(
            !output.status.success(),
            "input {replacement} unexpectedly accepted"
        );
        assert!(
            String::from_utf8(output.stderr).unwrap().contains(expected),
            "input {replacement} did not report {expected}"
        );
        args[index] = previous;
    }
}

#[test]
fn waves_cli_uses_supplied_job_zero_score_for_target_three_and_queue_branch() {
    let base = [
        "waves",
        "1",
        "2",
        "3",
        "4",
        "Cop_Carousel",
        "0",
        "1665",
        "0",
        "0x10100",
    ];
    let active = run(&[base.as_slice(), &["job0-score=20"]].concat());
    assert!(
        active.status.success(),
        "{}",
        String::from_utf8_lossy(&active.stderr)
    );
    let active_stdout = String::from_utf8(active.stdout).unwrap();
    assert!(active_stdout.contains("job0 effective special score=20 (supplied)"));
    assert!(active_stdout.contains("lottery_draw=0xf1a9ae74"));
    assert!(active_stdout.contains("special tide state=target 3 (calculated)"));
    assert!(active_stdout.contains("special queue branch at target 3 (conditional)"));
    let active_rows = wave_rows(&active_stdout);
    assert_eq!(active_rows.len(), 5);
    assert!(active_rows[3].contains("WAVE 3 generated event=0 tide=1"));
    assert!(active_rows[3].contains("queue_branch=special"));
    assert!(active_rows[3].contains("nominal Rare ticks=[0,0,0,0,480,776"));
    assert!(active_rows[1].contains("queue_branch=normal"));
    assert!(active_rows[2].contains("queue=unsupported event 3"));
    assert!(active_rows[4].contains("queue=unsupported event 4"));

    let inactive = run(&[base.as_slice(), &["job0-score=5"]].concat());
    assert!(inactive.status.success());
    let inactive_stdout = String::from_utf8(inactive.stdout).unwrap();
    assert!(inactive_stdout.contains("special tide state=inactive (calculated)"));
    assert!(inactive_stdout.contains("lottery_draw=0xf1a9ae74"));
    assert!(wave_rows(&inactive_stdout)[3].contains("WAVE 3 generated event=5 tide=1"));
    assert!(wave_rows(&inactive_stdout)[3].contains("queue_branch=normal"));
    assert!(wave_rows(&inactive_stdout)[3].contains("nominal Rare ticks=[0,19,296,480"));
}

#[test]
fn waves_cli_normalizes_zero_scene_seed_before_job_zero_lottery() {
    let output = run(&[
        "waves",
        "0",
        "0",
        "0",
        "0",
        "Cop_Carousel",
        "0",
        "0",
        "0",
        "0x10100",
        "job0-score=20",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("lottery_draw=0x951d958b"));
    assert!(stdout.contains("special tide state=target 3 (calculated)"));
}

#[test]
fn waves_cli_validates_signed_job_zero_score_and_job_type() {
    let base = [
        "waves",
        "1",
        "2",
        "3",
        "4",
        "Cop_Carousel",
        "0",
        "0",
        "0",
        "0x10100",
    ];
    let negative = run(&[base.as_slice(), &["job0-score=-1"]].concat());
    assert!(negative.status.success());
    assert!(
        String::from_utf8(negative.stdout)
            .unwrap()
            .contains("job0 effective special score=-1 (supplied)")
    );

    for (suffix, expected) in [
        ("job0-score=abc", "job0-score must be a signed decimal i32"),
        (
            "job0-score=2147483648",
            "job0-score must be a signed decimal i32",
        ),
        ("score=20", "unknown waves option"),
    ] {
        let output = run(&[base.as_slice(), &[suffix]].concat());
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr).unwrap().contains(expected));
    }
    let job_one = run(&[
        "waves",
        "1",
        "2",
        "3",
        "4",
        "Cop_Carousel",
        "1",
        "0",
        "0",
        "0x10100",
        "job0-score=20",
    ]);
    assert!(!job_one.status.success());
    assert!(
        String::from_utf8(job_one.stderr)
            .unwrap()
            .contains("job0-score requires JobType 0")
    );
}
