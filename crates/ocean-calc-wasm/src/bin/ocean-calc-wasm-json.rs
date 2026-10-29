//! Native reference executable for the built-WASM parity check.
use std::io::{Read, Write};

fn main() -> std::io::Result<()> {
    let mut bytes = Vec::new();
    std::io::stdin().take(8193).read_to_end(&mut bytes)?;
    // Check the byte cap before UTF-8 decoding: a bounded read may end in
    // the middle of a code point from an oversized, otherwise valid input.
    let overflow = " ".repeat(8193);
    let input = if bytes.len() > 8192 {
        overflow.as_str()
    } else {
        std::str::from_utf8(&bytes).unwrap_or("{")
    };
    writeln!(
        std::io::stdout(),
        "{}",
        ocean_calc_wasm::calculate_obs_json(input)
    )
}
