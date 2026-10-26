use ocean_calc::{
    SeedWords,
    applied_wave::{JobOneApplyState, apply_job_one_direct_writes},
    levels::{common_weights, queue_counts},
    records::{NormalWaveInput, initial_job_type_zero_special_state, predict_selections},
    schedule::{EntryKind, ScheduleBranch, ScheduleError, ScheduleInput, nominal_schedule},
    stage_data::stage_spawns,
};

const USAGE: &str = "usage: ocean-calc schedule <scene_s0> <scene_s1> <scene_s2> <scene_s3> <event_id> <N> <R> <normal|special>\n       ocean-calc schedule-difficulty <scene_s0> <scene_s1> <scene_s2> <scene_s3> <event_id> <effective_difficulty> <round_0_to_2> <normal|special>\n       ocean-calc waves <scene_s0> <scene_s1> <scene_s2> <scene_s3> <Cop_stage> <job_type_0_or_1> <effective_difficulty> <is_big_run_0_or_1> <global_word> [job0-score=<signed_decimal_i32> | players=<1..4>]";

fn parse_number(text: &str) -> Result<u32, String> {
    let parsed = if let Some(digits) = text.strip_prefix("0x") {
        u32::from_str_radix(digits, 16)
    } else {
        text.parse::<u32>()
    };
    parsed.map_err(|_| format!("invalid 32-bit number: {text}"))
}

fn scene_seed(args: &[String]) -> Result<SeedWords, String> {
    Ok(SeedWords::from_scene([
        parse_number(&args[1])?,
        parse_number(&args[2])?,
        parse_number(&args[3])?,
        parse_number(&args[4])?,
    ]))
}

fn run_schedule(args: &[String]) -> Result<(), String> {
    if args.len() != 9 {
        return Err(USAGE.into());
    }
    let seed = scene_seed(args)?;
    let event_type = u8::try_from(parse_number(&args[5])?)
        .map_err(|_| "event_id must fit in 8 bits".to_owned())?;
    let branch = match args[8].as_str() {
        "normal" => ScheduleBranch::Normal,
        "special" => ScheduleBranch::Special,
        _ => return Err("branch must be normal or special".into()),
    };
    let (wave_change_num, rare_appear_num, difficulty_note) = if args[0] == "schedule-difficulty" {
        let difficulty = parse_number(&args[6])?;
        let round = u8::try_from(parse_number(&args[7])?)
            .map_err(|_| "round must be 0, 1, or 2".to_owned())?;
        let counts =
            queue_counts(difficulty, round).map_err(|_| "round must be 0, 1, or 2".to_owned())?;
        (
            counts.wave_change_num,
            counts.rare_appear_num,
            Some(format!(
                "effective difficulty supplied: {difficulty}; round: {round}; N: {}; R: {}",
                counts.wave_change_num, counts.rare_appear_num
            )),
        )
    } else {
        (parse_number(&args[6])?, parse_number(&args[7])?, None)
    };
    let input = ScheduleInput {
        seed,
        event_type,
        wave_change_num,
        rare_appear_num,
        assumed_branch: branch,
    };
    let entries = nominal_schedule(input).map_err(|error| match error {
        ScheduleError::UnsupportedEvent(id) => {
            format!("event {id} is unsupported; ordered Rare queues are modeled for events 0, 5, and 6 on JobType 0/1")
        }
        ScheduleError::InvalidCounts => "require N >= 2".into(),
        ScheduleError::CapacityExceeded => "nominal queue capacity is 48".into(),
    })?;

    if let Some(note) = difficulty_note {
        println!("{note}");
    }
    println!(
        "nominal ticks; assumed branch: {}; actual spawns are not inferred",
        args[8]
    );
    for entry in entries {
        let kind = match entry.kind {
            EntryKind::Base => "base",
            EntryKind::Extra => "extra",
        };
        println!("{}\t{kind}", entry.tick);
    }
    Ok(())
}

fn run_waves(args: &[String]) -> Result<(), String> {
    if args.len() < 10 {
        return Err(USAGE.into());
    }
    let seed = scene_seed(args)?;
    let stage = stage_spawns(&args[5]).ok_or_else(|| format!("unknown stage: {}", args[5]))?;
    let job_type = parse_number(&args[6])?;
    if job_type > 1 {
        return Err("waves requires JobType 0 or 1; other JobTypes have different paths".into());
    }
    let mut job_zero_score = None;
    let mut players = None;
    for option in &args[10..] {
        if let Some(value) = option.strip_prefix("job0-score=") {
            if job_zero_score.is_some() {
                return Err("duplicate job0-score".into());
            }
            if job_type != 0 {
                return Err("job0-score requires JobType 0".into());
            }
            job_zero_score = Some(
                value
                    .parse::<i32>()
                    .map_err(|_| "job0-score must be a signed decimal i32".to_owned())?,
            );
        } else if let Some(value) = option.strip_prefix("players=") {
            if players.is_some() {
                return Err("duplicate players".into());
            }
            if job_type != 1 {
                return Err("players requires JobType 1".into());
            }
            let parsed = value
                .parse::<i32>()
                .map_err(|_| "players must be 1..=4".to_owned())?;
            if !(1..=4).contains(&parsed) {
                return Err("players must be 1..=4".into());
            }
            players = Some(parsed);
        } else {
            return Err(format!("unknown waves option: {option}"));
        }
    }
    let difficulty = parse_number(&args[7])?;
    if difficulty > 1665 {
        return Err("effective difficulty must be in 0..=1665".into());
    }
    let is_big_run = parse_number(&args[8])?;
    if is_big_run > 1 {
        return Err("is_big_run must be 0 or 1".into());
    }
    let global_word = parse_number(&args[9])?;
    let common = common_weights(difficulty);
    let inputs = [NormalWaveInput {
        event_weights: common.event,
        tide_weights: stage.water_override.unwrap_or(common.water),
        mode_one: is_big_run != 0,
        ban_event_mask: stage.ban_event_mask,
        global_word,
    }; 5];
    let special_decision = job_zero_score
        .map(|score| initial_job_type_zero_special_state(Some(seed.as_array()), score));
    let initial_special = special_decision.and_then(|decision| decision.special);
    let outcome = predict_selections(seed, None, inputs, job_type as u8, initial_special)
        .map_err(|error| format!("generated WAVE calculation failed: {error:?}"))?;

    println!(
        "conditional generated WAVE preview: scene seed words supplied; JobType={job_type} (supplied); effective difficulty={difficulty} (supplied); stage={} (assumed loaded); is_big_run={is_big_run} (supplied); global_word=0x{global_word:08x} (supplied)",
        stage.name
    );
    if let Some(count) = players {
        println!(
            "JobType 1 direct application: active-player helper returns {count} at every WAVE (assumed); special-tide flags inactive (assumed); application-time runtime mid-tide override inactive (assumed); scheduler-time mode does not bypass Rare queue (assumed)"
        );
    }
    if let (Some(score), Some(decision)) = (job_zero_score, special_decision) {
        println!(
            "ROMFS source weights/counts (assumed); stage Water/BanEvent from owned ROMFS (assumed); queue count source=min(wave,2)"
        );
        let state_label = if decision.special.is_some() {
            "target 3"
        } else {
            "inactive"
        };
        println!(
            "job0 effective special score={score} (supplied); live JobType 0 mode object and ordinary initial shared RNG setup (assumed); lottery_draw=0x{:08x}; threshold_bits=0x{:08x}; uniform_bits=0x{:08x}; special tide state={state_label} (calculated)",
            decision.draw, decision.threshold_bits, decision.uniform_bits
        );
        if decision.special.is_some() {
            println!(
                "special queue branch at target 3 (conditional): runtime +0x624 must remain set and current index must match +0x620; other waves use normal branch"
            );
        } else {
            println!(
                "queue branch=normal (conditional on inactive special state at queue construction)"
            );
        }
    } else {
        println!(
            "ROMFS source weights/counts (assumed); stage Water/BanEvent from owned ROMFS (assumed); queue count source=min(wave,2); special tide state=inactive (assumed); queue branch=normal (assumed)"
        );
    }
    for selection in outcome.selections {
        let wave_index = selection.wave_index;
        let event_id = selection.event_id;
        let tide_id = selection.tide_id;
        let direct = players
            .map(|count| {
                apply_job_one_direct_writes(
                    selection,
                    JobOneApplyState {
                        previous_applied_tide: 0,
                        active_player_count: count,
                        mode_flag_591: false,
                        special_flag_624: false,
                        special_target_index_620: 0,
                        runtime_mode_forces_mid_tide: false,
                    },
                )
            })
            .transpose()
            .map_err(|error| format!("WAVE {wave_index} direct application failed: {error:?}"))?;
        let applied_event = direct.map_or(event_id, |value| value.event_id);
        let mut prefix = format!("WAVE {wave_index} generated event={event_id} tide={tide_id}");
        if let Some(value) = direct {
            prefix.push_str(&format!(
                " applied-direct event={} tide={}",
                value.event_id, value.tide_id
            ));
            if value.requires_handler_observation {
                println!("{prefix} handler=unresolved queue=unavailable");
                continue;
            }
        }
        let queue_branch = if initial_special.is_some_and(|state| state.target_index == wave_index)
        {
            ScheduleBranch::Special
        } else {
            ScheduleBranch::Normal
        };
        if !matches!(applied_event, 0 | 5 | 6) {
            println!("{prefix} queue=unsupported event {applied_event}");
            continue;
        }
        let source_round = wave_index.min(2);
        let counts = queue_counts(difficulty, source_round)
            .map_err(|error| format!("WAVE {wave_index} count calculation failed: {error:?}"))?;
        let entries = nominal_schedule(ScheduleInput {
            seed,
            event_type: applied_event,
            wave_change_num: counts.wave_change_num,
            rare_appear_num: counts.rare_appear_num,
            assumed_branch: queue_branch,
        })
        .map_err(|error| format!("WAVE {wave_index} queue calculation failed: {error:?}"))?;
        let ticks = entries
            .iter()
            .map(|entry| entry.tick.to_string())
            .collect::<Vec<_>>()
            .join(",");
        if job_zero_score.is_some() {
            let branch_label = match queue_branch {
                ScheduleBranch::Normal => "normal",
                ScheduleBranch::Special => "special",
            };
            println!(
                "{prefix} N={} R={} nominal Rare ticks=[{ticks}] source_round={source_round} queue_branch={branch_label}",
                counts.wave_change_num, counts.rare_appear_num
            );
        } else {
            println!(
                "{prefix} N={} R={} nominal Rare ticks=[{ticks}] source_round={source_round}",
                counts.wave_change_num, counts.rare_appear_num
            );
        }
    }
    if players.is_some() {
        println!(
            "scenario-to-scene seed transfer is not inferred; later overrides and event handlers are not inferred; callbacks, actual species/coordinates, and later regeneration are not inferred"
        );
    } else {
        println!(
            "scenario-to-scene seed transfer is not inferred; applied overrides, callbacks, actual species/coordinates, and later regeneration are not inferred"
        );
    }
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("schedule" | "schedule-difficulty") => run_schedule(args),
        Some("waves") => run_waves(args),
        _ => Err(USAGE.into()),
    }
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if let Err(error) = run(&args) {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
