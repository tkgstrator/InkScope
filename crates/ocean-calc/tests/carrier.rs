use ocean_calc::{
    carrier::{
        CarrierChoice, CarrierRandomInput, CarrierSelectionError, predict_stage_carrier_candidate,
        select_carrier_candidate,
    },
    rng::XorShift128,
    stage_data::stage_spawns,
};

#[test]
fn tick_zero_reseeds_and_scans_occupied_slots() {
    let candidates = [[0.0, 0.0, 0.0]; 5];
    let slots = [-1, 17, -1, -1, -1];
    let selected =
        select_carrier_candidate(&candidates, &slots, CarrierRandomInput::FreshTickLow32(0))
            .unwrap();
    assert_eq!(selected.draw, 0x4807_714d);
    assert_eq!(selected.start_index, Some(1));
    assert_eq!(
        selected.choice,
        CarrierChoice::Selected {
            index: 2,
            position: [0.0; 3]
        }
    );
    assert_eq!(
        selected.random_state_after.as_array(),
        [0x6c07_8967, 0x714a_cb41, 0x4807_7044, 0x4807_714d]
    );
}

#[test]
fn retained_state_and_empty_or_full_lists_have_one_draw() {
    let state = CarrierRandomInput::RetainedState(XorShift128::from_words([1, 2, 3, 4]));
    let empty = select_carrier_candidate(&[], &[], state).unwrap();
    assert_eq!(empty.draw, 0x80d);
    assert_eq!(empty.start_index, None);
    assert_eq!(empty.choice, CarrierChoice::NoCandidates);

    let occupied = select_carrier_candidate(&[[0.0; 3]; 2], &[0, 5], state).unwrap();
    assert_eq!(occupied.draw, 0x80d);
    assert_eq!(occupied.start_index, Some(0));
    assert_eq!(occupied.choice, CarrierChoice::AllOccupied);
    assert_eq!(occupied.random_state_after.as_array(), [2, 3, 4, 0x80d]);
}

#[test]
fn occupied_tail_wraps_to_the_first_available_slot() {
    let selected = select_carrier_candidate(
        &[[0.0; 3]; 5],
        &[-1, 5, 5, 5, 5],
        CarrierRandomInput::RetainedState(XorShift128::from_words([0, 0, 0, u32::MAX])),
    )
    .unwrap();
    assert_eq!(selected.draw, 0xffff_e000);
    assert_eq!(selected.start_index, Some(4));
    assert_eq!(
        selected.choice,
        CarrierChoice::Selected {
            index: 0,
            position: [0.0; 3]
        }
    );
}

#[test]
fn selected_nan_x_aborts_after_draw_and_length_mismatch_is_rejected() {
    let selected = select_carrier_candidate(
        &[[f32::NAN, 3.0, 4.0]],
        &[-1],
        CarrierRandomInput::RetainedState(XorShift128::from_words([1, 2, 3, 4])),
    )
    .unwrap();
    assert_eq!(
        selected.choice,
        CarrierChoice::InvalidSelectedX { index: 0 }
    );
    assert_eq!(
        select_carrier_candidate(&[[0.0; 3]], &[], CarrierRandomInput::FreshTickLow32(0)),
        Err(CarrierSelectionError::SlotCountMismatch {
            candidates: 1,
            slots: 0
        })
    );
}

#[test]
fn owned_shakedent_carrier_lists_and_tide_fallback_are_explicit() {
    let stage = stage_spawns("Cop_Shakedent").unwrap();
    assert_eq!(stage.carrier_box_sources(0).unwrap().len(), 86);
    assert_eq!(stage.carrier_box_sources(1).unwrap().len(), 90);
    assert_eq!(stage.carrier_box_sources(2).unwrap().len(), 54);
    assert!(stage.carrier_box_sources(3).is_none());

    let mid_slots = vec![-1; 90];
    let mid = predict_stage_carrier_candidate(
        stage,
        Some(1),
        &mid_slots,
        CarrierRandomInput::FreshTickLow32(0),
    )
    .unwrap();
    assert_eq!(mid.effective_tide, 1);
    assert_eq!(mid.selection.start_index, Some(25));
    let CarrierChoice::Selected { index, position } = mid.selection.choice else {
        panic!("tick-zero Mid candidate should be selected");
    };
    assert_eq!(index, 25);
    assert_eq!(
        stage.carrier_box_sources(1).unwrap()[index].actor_index,
        229
    );
    assert_eq!(
        position.map(f32::to_bits),
        [0x4168_6a36, 0x4157_ae14, 0x4256_8a69]
    );

    let low_slots = vec![-1; 86];
    for tide in [None, Some(3)] {
        let low = predict_stage_carrier_candidate(
            stage,
            tide,
            &low_slots,
            CarrierRandomInput::FreshTickLow32(0),
        )
        .unwrap();
        assert_eq!(low.effective_tide, 0);
    }
}
