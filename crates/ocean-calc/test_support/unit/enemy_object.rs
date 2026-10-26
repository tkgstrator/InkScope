use super::artillery_reservoir_replace;

#[test]
fn artillery_float_threshold_can_differ_from_integer_multiply_high() {
    // At n=3, high32(0x55555556 * 3) is 1, but the game's f32
    // bit-sample rounds below f32(1/3) and replaces the selection.
    assert!(artillery_reservoir_replace(0x5555_5556, 3));
}
