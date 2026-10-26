use ocean_calc::spawn_position::{GenericRarePositionError, generic_rare_event_position};

const ORIGIN: [f32; 3] = [100.0, 200.0, 300.0];
const DIRECTION: [f32; 3] = [1.0, 0.0, 0.0];

#[test]
fn each_channel_uses_its_own_columns_and_base_distance() {
    let position = |channel, ordinal| {
        generic_rare_event_position(ORIGIN, DIRECTION, channel, [1, 1], ordinal).unwrap()
    };
    assert_eq!(position(0, 0), ORIGIN);
    assert_eq!(position(0, 2), [95.0, 200.0, 300.0]);
    assert_eq!(position(1, 0), [97.5, 200.0, 302.0]);
    assert_eq!(position(1, 1), [97.5, 200.0, 300.0]);
    assert_eq!(position(1, 2), [95.0, 200.0, 302.0]);
    assert_eq!(position(2, 0), [95.0, 200.0, 300.0]);
}

#[test]
fn channel_two_uses_ceil_half_for_odd_counter_and_signed_machine_branch() {
    let at = |c1| generic_rare_event_position(ORIGIN, DIRECTION, 2, [0, c1], 0).unwrap();
    assert_eq!(at(0), ORIGIN);
    assert_eq!(at(1), [97.5, 200.0, 300.0]);
    assert_eq!(at(2), [97.5, 200.0, 300.0]);
    assert_eq!(at(3), [95.0, 200.0, 300.0]);
    assert_eq!(at(-2), ORIGIN);
    assert_eq!(at(-3), [102.5, 200.0, 300.0]);
}

#[test]
fn vertical_direction_uses_horizontal_fallback_axis() {
    assert_eq!(
        generic_rare_event_position([0.0; 3], [0.0, 1.0, 0.0], 1, [0, 0], 0),
        Ok([-2.0, 0.0, 0.0])
    );
}

#[test]
fn unsupported_channel_is_rejected() {
    assert_eq!(
        generic_rare_event_position(ORIGIN, DIRECTION, 3, [0, 0], 0),
        Err(GenericRarePositionError::InvalidChannel(3))
    );
}
