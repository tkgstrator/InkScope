//! Conditional XYZ passed to a generic Rare creation event in 11.3.0.
//!
//! The caller supplies a registered spawn actor's runtime transform, the
//! already-selected placement channel, allocation counters, and the attempt's
//! ordinal within that channel. This is not a final enemy-actor position.

const COLUMN_COUNTS: [i32; 3] = [1, 2, 1];
const SPACING: f32 = 2.5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenericRarePositionError {
    InvalidChannel(u8),
}

/// Calculate the XYZ passed to `0x375d2a0` on the generic Rare spot path.
///
/// `origin` and `direction` are the selected actor's *runtime* vectors. The
/// channel is selected by species and a live enemy-info lookup; `allocations`
/// are the local C0/C1 counts assigned before creation calls, not successful
/// spawn counts. `ordinal` is the attempted individual's signed channel count.
/// The FP operations mirror `0x375ec4c..0x375ee5c`, including the fused
/// longitudinal subtraction and separately rounded coordinate operations.
pub fn generic_rare_event_position(
    origin: [f32; 3],
    direction: [f32; 3],
    channel: u8,
    allocations: [i32; 2],
    ordinal: i32,
) -> Result<[f32; 3], GenericRarePositionError> {
    let Some(&columns) = COLUMN_COUNTS.get(usize::from(channel)) else {
        return Err(GenericRarePositionError::InvalidChannel(channel));
    };

    let base_one = (allocations[0] as f32) * SPACING;
    let c1_plus_one = allocations[1].wrapping_add(1);
    let adjusted_c1 = if c1_plus_one < 0 {
        allocations[1].wrapping_add(2)
    } else {
        c1_plus_one
    };
    let half_c1 = adjusted_c1 >> 1;
    let base_two = base_one + (half_c1 as f32) * SPACING;
    let base = match channel {
        0 => 0.0,
        1 => base_one,
        2 => base_two,
        _ => unreachable!("validated channel"),
    };

    let quotient = ordinal / columns;
    let remainder = ordinal % columns;
    // FNMSUB s0, s1, s11, s0 with s11 = -2.5: q * -2.5 - base.
    let longitudinal = (quotient as f32).mul_add(-SPACING, -base);
    let lateral = if columns == 1 {
        0.0
    } else {
        ((remainder as f32) * 4.0) / (columns as f32) - 2.0
    };
    let side = horizontal_side_axis(direction);
    Ok(std::array::from_fn(|axis| {
        let forward = longitudinal * direction[axis];
        let at_origin = forward + origin[axis];
        let sideways = lateral * side[axis];
        sideways + at_origin
    }))
}

/// Mirror the finite-vector horizontal perpendicular normalization in
/// `0x375eca0..0x375ed0c`, including its near-zero fallback. An overflowed
/// norm follows the game's rescaling branch at `0x375f118..0x375f16c`.
fn horizontal_side_axis(direction: [f32; 3]) -> [f32; 3] {
    let [dx, dy, dz] = direction;
    let negative_zero = -0.0_f32;
    let nx = dz + dy * negative_zero;
    let ny = dx.mul_add(0.0, dz * negative_zero);
    let nz = dy.mul_add(0.0, -dx);
    let mut squared = ny * ny;
    squared = nx.mul_add(nx, squared);
    squared = nz.mul_add(nz, squared);
    let norm = squared.sqrt();
    if norm <= f32::from_bits(0x29cc_cccd) {
        return [1.0, 0.0, 0.0];
    }
    if norm.is_infinite() {
        let scale = f32::from_bits(0x1f13_cd3b);
        let scaled = [nx * scale, ny * scale, nz * scale];
        let mut squared = scaled[1] * scaled[1];
        squared = scaled[0].mul_add(scaled[0], squared);
        squared = scaled[2].mul_add(scaled[2], squared);
        let norm = squared.sqrt();
        let reciprocal = 1.0 / norm;
        let axis = scaled.map(|component| component * reciprocal);
        if norm * f32::from_bits(0x5fdd_b3d6) == 0.0 {
            return [1.0, 0.0, 0.0];
        }
        return axis;
    }
    let reciprocal = 1.0 / norm;
    [nx * reciprocal, ny * reciprocal, nz * reciprocal]
}
