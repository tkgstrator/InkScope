# OceanCalc OBS JSON facade

This crate only orchestrates existing `ocean_calc` Rust APIs. It has no
packet-capture, cryptography, filesystem, network, clock, or host-RNG dependency.
The facade builds as native Rust and `wasm32-unknown-unknown`; both expose the
same `calculate_obs_json(input: &str) -> String` behavior.

The input is one JSON object, at most **8192 UTF-8 bytes**, with unknown and
duplicate fields rejected. Version1 requires `stage_resource` (ASCII identifier,
1–64bytes), `job_type` (u8), `difficulty` (0–1665), `seed` (exactly4u32),
`unlocked_season` (−1–1022), `main_weapon_ids` and
`original_main_weapon_ids` (up to4 IDs each, −3–65532),
`scenario_app_version` (u32), `scenario_competition_type` (0/1), and optional
`player_count` (1–4; absent/null means unknown).

Output is `{version:1,status:"ok",wave_preview:{...}}` or
`{version:1,status:"error",error:{code,message}}`. Errors do not echo input.
`wave_preview` retains the packet-preview scenario/equipment shape, adding one
`first_boss` per generated WAVE. There are at most4scenarios,5WAVEs per scenario,
and an exact5×4equipment matrix. Independent equipment remains available when
stage/WAVE prediction is unavailable. Inputs, including seed words, are not
returned.

Equipment also carries four `special_weapons` IDs/nulls by internal equipment
index. These reuse the already calculated special selection and consume no
additional RNG draws. Their separate conditions require an inactive runtime
special-reorder gate, no later special override, and the normal adopted-scene
raw-index mapping for room display. They predict the supplied type, not the
remaining use count. Unavailable equipment carries four null special IDs.

`first_boss` contains `status`, `basis:"assumed_idealized"`, nullable
`species_id`/`nominal_tick`, `conditions`, and nullable `reason`. This is a
conditional first species draw, not a confirmed spawn. It uses the existing
selected-JobType1/direct-application/first-Rare APIs. Missing player count,
unsupported events, unknown Fog cycle, and unresolved stage/runtime conditions
remain unavailable. It never synthesizes a later boss sequence or position.
The listed assumptions include tick-zero scheduling, zero initial population,
inactive mode overrides, and no earlier shared-RNG draw. The caller must retain
these evidence conditions when publishing a prediction.

From the sibling Visualizer repository:

```sh
rustup target add wasm32-unknown-unknown
# CLI must match the exact wasm-bindgen crate version0.2.129.
bun tools/build-ocean-calc-wasm.ts --verify
```

The builder emits web-target JS, declarations, WASM and a source/artifact hash
manifest under `api/generated/ocean-calc-wasm`. Bun can load the bytes with
`initSync({module: bytes})` before accepting ingest. Consumers must fail closed
if initialization fails. Native/WASM parity verification exercises the actual
generated export and native executable, including invalid inputs and byte caps.

Execution ledger: the four native contract tests were run against the missing
implementation and all failed. They then passed against this facade. Literal
equipment vectors come from the existing packet wrapper's independently checked
post-special RNG tests; no packet-capture dependency is linked. Target install
uses official rustup. Changes are confined to this new crate, workspace/lock,
the dedicated Visualizer builder, and generated artifacts. No commits, services,
captures, or device actions are part of this task.

Fresh verification:

- `cargo test --locked --offline -p ocean-calc-wasm`:5contract tests pass.
- Built web-target WASM loaded by Bun `initSync`:26native/WASM parity cases
  pass, including16structured errors and exact8192/8193-byte boundaries.
- `cargo clippy --locked --offline -p ocean-calc-wasm --all-targets -- -D warnings`
  and owned-file `rustfmt --check` pass. Builder TypeScript check passes.
- Core/facade workspace excluding InkScope:490tests pass.
- Full Rust workspace with `--test-threads=1`:633tests pass across65suites.
- The first default-parallel workspace attempt failed three existing HDMI
  timing tests: `stalled_frames_are_reported_even_with_an_alive_child`,
  `foreground_group_interrupt_does_not_signal_hdmi_child`, and
  `missing_executable_and_early_exit_are_reported`. Targeted serial HDMI
  rerun passed all12; the complete serial workspace then passed. No HDMI
  source was changed. Ignored verification logs remain beside this README.

The actual WASM parity check also first exposed an oversized multibyte-input
failure in the native reference CLI: its8193-byte bounded read could cut a UTF8
code point before checking length. It now checks byte length before decoding;
the actual WASM/native parity run passes that regression.

The special tuple extension was tested RED before implementation against the
independent core low/high literal vectors, then GREEN. Raw/public API tests
accept older equipment without the tuple and reject incorrect tuple length,
out-of-range or noninteger IDs, nonnumeric entries, and nonnull unavailable
values. Persistence, one-event retention and restart restore retain the tuple.
