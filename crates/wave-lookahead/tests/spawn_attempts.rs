use salmon_wave_lookahead::spawn_attempts::{
    AttemptBlock, AttemptInput, AttemptPlan, AttemptPlanError, plan_ordinary_item_attempts,
};

fn input() -> AttemptInput {
    AttemptInput {
        requested_count: 3,
        selected_rare_id: 7,
        active_rare_total: 0,
        active_total: 0,
        item_total_limit: 15,
        spot_succeeded: true,
        post_spot_allows: true,
        selected_rare_cap: 3,
        selected_rare_active: 0,
        follower_cap: 20,
        follower_active_after_rare: 0,
        follower_budget: 20,
        scene_mode: 0,
    }
}

#[test]
fn ordinary_item_calls_creation_once_for_rare_and_twice_for_followers() {
    assert_eq!(
        plan_ordinary_item_attempts(input()),
        Ok(AttemptPlan::Calls {
            effective_budget: 3,
            rare_create_call: true,
            follower_id: 0,
            follower_create_calls: 2,
        })
    );
}

#[test]
fn pillar_forces_one_effective_slot_and_rare_cap_failure_leaves_it_for_follower() {
    let mut value = input();
    value.selected_rare_id = 11;
    assert_eq!(
        plan_ordinary_item_attempts(value),
        Ok(AttemptPlan::Calls {
            effective_budget: 1,
            rare_create_call: true,
            follower_id: 0,
            follower_create_calls: 0,
        })
    );
    value.selected_rare_active = 3;
    assert_eq!(
        plan_ordinary_item_attempts(value),
        Ok(AttemptPlan::Calls {
            effective_budget: 1,
            rare_create_call: false,
            follower_id: 0,
            follower_create_calls: 1,
        })
    );
}

#[test]
fn rare_cap_failure_keeps_all_three_slots_for_follower_calls() {
    let mut value = input();
    value.selected_rare_active = 3;
    assert_eq!(
        plan_ordinary_item_attempts(value),
        Ok(AttemptPlan::Calls {
            effective_budget: 3,
            rare_create_call: false,
            follower_id: 0,
            follower_create_calls: 3,
        })
    );
}

#[test]
fn scene_mode_seven_chooses_follower_id_one_and_budget_caps_calls() {
    let mut value = input();
    value.scene_mode = 7;
    value.selected_rare_id = 8;
    value.follower_budget = 1;
    assert_eq!(
        plan_ordinary_item_attempts(value),
        Ok(AttemptPlan::Calls {
            effective_budget: 3,
            rare_create_call: true,
            follower_id: 1,
            follower_create_calls: 1,
        })
    );
    value.scene_mode = 263;
    assert_eq!(
        plan_ordinary_item_attempts(value),
        Ok(AttemptPlan::Calls {
            effective_budget: 3,
            rare_create_call: true,
            follower_id: 0,
            follower_create_calls: 1,
        })
    );
}

#[test]
fn follower_active_cap_is_checked_after_the_rare_call() {
    let mut value = input();
    value.follower_cap = 4;
    value.follower_active_after_rare = 3;
    assert_eq!(
        plan_ordinary_item_attempts(value),
        Ok(AttemptPlan::Calls {
            effective_budget: 3,
            rare_create_call: true,
            follower_id: 0,
            follower_create_calls: 1,
        })
    );
    value.follower_active_after_rare = 4;
    assert_eq!(
        plan_ordinary_item_attempts(value),
        Ok(AttemptPlan::Calls {
            effective_budget: 3,
            rare_create_call: true,
            follower_id: 0,
            follower_create_calls: 0,
        })
    );
    value.follower_budget = -1;
    assert_eq!(
        plan_ordinary_item_attempts(value),
        Ok(AttemptPlan::Calls {
            effective_budget: 3,
            rare_create_call: true,
            follower_id: 0,
            follower_create_calls: 0,
        })
    );
}

#[test]
fn global_and_spot_gates_block_creation_calls_but_not_the_prior_draw_trace() {
    let mut value = input();
    value.active_rare_total = 15;
    assert_eq!(
        plan_ordinary_item_attempts(value),
        Ok(AttemptPlan::Skipped(AttemptBlock::RareGlobalLimit))
    );
    value.active_rare_total = 14;
    value.active_total = 16;
    assert_eq!(
        plan_ordinary_item_attempts(value),
        Ok(AttemptPlan::Skipped(AttemptBlock::TotalGlobalLimit))
    );
    value.active_total = 15;
    assert!(matches!(
        plan_ordinary_item_attempts(value),
        Ok(AttemptPlan::Calls { .. })
    ));
    value.spot_succeeded = false;
    assert_eq!(
        plan_ordinary_item_attempts(value),
        Ok(AttemptPlan::Skipped(AttemptBlock::SpotFailed))
    );
    value.spot_succeeded = true;
    value.post_spot_allows = false;
    assert_eq!(
        plan_ordinary_item_attempts(value),
        Ok(AttemptPlan::Skipped(AttemptBlock::PostSpotVeto))
    );
}

#[test]
fn out_of_scope_request_count_and_species_are_rejected() {
    let mut value = input();
    value.requested_count = 0;
    assert_eq!(
        plan_ordinary_item_attempts(value),
        Err(AttemptPlanError::InvalidRequestedCount(0))
    );
    value.requested_count = 3;
    value.selected_rare_id = 3;
    assert_eq!(
        plan_ordinary_item_attempts(value),
        Err(AttemptPlanError::InvalidRareId(3))
    );
}
