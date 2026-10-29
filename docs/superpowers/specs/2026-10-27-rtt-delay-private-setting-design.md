# InkScope live dashboard, RTT, and settings design

## Intent

Read a passive Splatoon 3 LAN capture during a room and game without losing the connection, key, stage, difficulty, and gameplay state in a scrolling log. Inspect every original UDP packet when needed. The repeater hub forwards traffic; InkScope observes a copy and continues saving classic PCAP bytes.

This design covers the full dashboard and its decoder prerequisites in one implementation plan. Implement decoder/stream support first, then build the TUI on those shared outputs.

## Approaches

- **Terminal dashboard with in-app views (chosen):** One terminal window has a fixed status pane and a right pane that switches between recognized events and every packet with hex detail. Switching does not restart capture.
- **Separate command-line display modes:** Simpler, but changing views requires another process and loses live context.
- **Browser dashboard:** More layout space, but adds a server and browser state to the current CLI. Defer unless the terminal layout proves insufficient.

Existing `--mode normal|debug` retains its stream row-filter meaning. It is not repurposed as the dashboard view switch.

## Screen and controls

Add `--ui tui` to `capture` and `inspect`. Stream/JSONL remain the default; TUI needs an interactive terminal and conflicts with `--jsonl`. The left status pane stays visible. `Tab` switches the right pane between **Summary** (recognized events with bounded scrollback) and **Packets** (every accepted UDP record, even authentication failures). `Enter` on a packet opens source/destination, direction, authentication status, original UDP payload hex, parsed fields, and authenticated payload hex when available. `Esc` closes detail, arrows/Page keys scroll, `End` follows live traffic, and `q` exits. Narrow terminals stack panes; very small terminals show a resize message while capture continues.

```text
Connection
  Server: 192.168.x.y
  Client: 192.168.x.z  PCAP RTT 34.066 ms
Keys
  GameKey: 0123456789abcdef0123456789abcdef
           file loaded; browse authenticated
  PiaKey:  0123456789abcdef0123456789abcdef
           derived; authenticated packet observed
Setting
  StageId: Cop_Shakespiral
  Difficulty: 1665 (internal integer)
Gameplay
  Sequence (wire): 1   Quota (packet): 4
  Current WAVE: unconfirmed
  Preview W0–W4: conditional (if GameSeed is present)
```

Keys show full 16-byte HEX, source, and observed authentication status persistently, as requested. A PiaKey derived from an unverified BrowseReply is labelled a candidate; only a key that actually authenticates a Pia packet is labelled used. If several candidates exist, show the used key for the selected server/client pair and a count of other candidates. Never send key bytes to JSONL, logs, or error messages. Unknown values use `—` and last update age is visible. Keep a bounded number of client rows and count overflow. Preserve explicit `--target-mac`/`--target-ip` direction arrows; otherwise use confirmed roles when possible and always keep literal endpoints.

For multiple clients, put an RTT on each server/client pair's client row rather than inventing one server-wide number. The server row shows its IP; selected packet details show request direction. This RTT is the capture-observed Type 0 to Type 1 response interval, not one-way latency or the Switch's internal tick result.

Show gameplay state after authenticated gameplay packets appear. `CoopSequenceNetState` supplies raw `Sequence` and a `Quota` packet field believed to be applied by the inspected title path. The mapping from sequence index to the WAVE number shown in-game is unconfirmed, so the UI must not relabel it as current WAVE. Retain the most recent `WavePreviewer` result, because it emits only on a new `OnlineCoopSetting`. That event's stage/difficulty/GameSeed belong to its conditional W0–W4 event/tide previews; do not substitute a later PrivateSetting field into an existing preview. The preview has several Big Run/global-word scenarios; expose each as a separate choice in Summary and mark all as conditional. A PrivateSetting update describes a transmitted setting; it does not prove scene application.

Derive provisional room roles from a proof-verified BrowseReply's host IP and its matched authenticated BrowseRequest client, then confirm a client-to-host authenticated Net 0x32. Repeated Browse traffic or repeated Net 0x32 for the same pair does not reset the room. A different confirmed host, a confirmed role reversal, or an authenticated Net 0x32 after at least 30 capture-time seconds without room traffic begins a new room epoch and clears role-bound RTT, setting, and gameplay state. Retain key candidates separately so the next epoch can still authenticate packets; display their provenance. If no confirmed role exists, show literal IP endpoints with an unknown-role label. `--target-mac`/`--target-ip` arrows retain their original target-relative meaning even when roles are known.

For several clients, `[` and `]` cycle the selected pair; the most recently confirmed pair is selected initially. Each client row uses the latest matched Type 0/Type 1 interval in either request direction and shows the request direction in detail. The Setting pane prefers the latest authenticated PrivateSetting values in the room and labels their source. If an authenticated OnlineCoopSetting disagrees, retain and expose both sets of values with their sources; never silently substitute a PrivateSetting value into a saved WAVE preview.

## Decoder and state

- Use `RTTProtocol` for Pia protocol ID 3 in human output and JSONL. The rename is locally implemented and its focused test passes.
- For authenticated 21-byte protocol 3, Type byte `0` is a request and `1` a response. Key pending requests by requester IP, responder IP, and `OriginTick`. A reverse-direction Type 1 consumes a match and gains the string field `CaptureResponseDelayMs` with three decimals, matching the existing string-valued JSONL field convention. Duplicate Type 0 replaces the older timestamp; expire after two capture-time seconds and cap at 256. Reject reverse or equal timestamps, unmatched, and unauthenticated responses. Process multiple chunks in packet order. Put matching in the decoder so stream, JSONL, and TUI agree. Include it in stream highlights; show latest per-peer result and its capture-time age in TUI.
- Recognize authenticated protocol 11 port 100 with `p[0]==7`, big-endian declared body length `p.len()-8`, exact total lengths and presence bits. Bit offsets below use the existing `bits()` convention: bit 0 is the least significant bit of byte 0, while multi-byte length fields are big-endian. Common: 78 bytes, bits 97–99 `0,0,1`, 512-bit stage string starting at 100. Coop: 161 bytes, bits 97–101 `0,0,0,0,1`, 11-bit difficulty wire starting at 102 and scenario flag at 113. Combined: 226 bytes, Common body at 100–614, branch bits 615–616 `0,1`, Coop body at 617–1801 with 11-bit wire at 617, final flag at 1802 `0`. Validate known trailing flags and padding before naming a layout. Replace the existing 10-bit `HazardCandidatePercent` heuristic for these layouts with the confirmed 11-bit wire field and `Difficulty = DifficultyWire - 1` for nonzero wire. Wire value zero leaves Difficulty unavailable (`—`), does not clear a previous nonzero value, and does not by itself invalidate an otherwise recognized PrivateSetting. A valid printable stage resource becomes `StageId`; an empty one clears stale stage. Malformed/unsupported variants get no PrivateSetting label.
- Specifically require the remaining Common flags 3/4/5 to be `0,0,0`, the Coop final flag to be `0`, and unused rounding bits to be zero for recognized examples. Unknown layouts remain generic protocol 11 and visible in debug/Packets.
- Normal stream mode shows recognized PrivateSetting but hides generic protocol 11; debug shows both. `--hide-cycle` still affects stream, while TUI Packets always retains all records. TUI Summary can suppress repetitive rows without suppressing RTT state updates.
- Keep authoritative decoder/room/RTT/key/setting/gameplay state on the reader thread. The UI copies a small latest-state snapshot under a short lock; only historical Summary/Packets entries may be dropped. Each packet display entry carries `UdpPacket` plus its decoded `Vec<Row>` and key-use metadata, so one packet with multiple chunks remains one selectable item. TUI decoding requests authenticated chunk hex internally even when stream `--details` is false; never add raw key bytes to `Row`. The channel is bounded to 128 entries; the UI history is bounded by both 512 entries and 8 MiB of payload bytes. Report dropped history count. Reader errors appear as error entries without invented hex. The Packets view covers decoded UDP on ports 35000/49152 plus those reader errors; other protocols/ports are outside the capture filter or reader scope.
- Add `--ui` only to `capture` and `inspect`, using ratatui plus crossterm and `std::io::IsTerminal`. `--jsonl` conflicts with TUI. In TUI, `--mode` sets the initial Summary filter, `--hide-cycle` affects Summary only, and `--details` does not control packet detail availability. Original UDP hex is always available in Packets. Keep current stream/JSONL behavior compatible except explicitly improved labels/fields.
- Tee PCAP bytes to the output before display work. Capture stderr and parser errors into status/error entries so they cannot corrupt the alternate screen. `q` and Ctrl-C use the existing controlled child-stop path, then drain complete buffered PCAP records and flush output before restoring the terminal; print final summary only after restoration. Use a Drop guard and panic hook for restoration, with a mock terminal backend unit test plus a pseudo-terminal smoke test. At saved-PCAP EOF, keep the TUI open with totals until `q`.

## Verification

- Decoder tests for RTT matching/reversal/duplicates/staleness/bounds; PrivateSetting Common/Coop/Combined positive examples and malformed rejects; internal difficulty and normal/debug visibility.
- UI reducer tests for role reversal, repeated Browse/Net 0x32 without reset, per-client RTT, room reset, setting updates and empty stage, raw sequence/quota versus preview, bounded records/bytes, key provenance, and original UDP hex after auth failure.
- Run Rust formatting and full tests. Inspect saved PCAP in stream and TUI to compare record counts and payloads. Keep the current live capture process/file untouched. Check terminal layout and restoration in a disposable pseudo-terminal without opening an extra terminal window.

## Sources and limits

- [Pia protocol IDs 6.32–6.40](https://github.com/kinnay/NintendoClients/wiki/Pia-Protocols#632---640) and [RTT Protocol payload](https://github.com/kinnay/NintendoClients/wiki/RTT-Protocol).
- Splatoon 3 11.3.0 PrivateSetting offsets and gameplay-state limits come from the NXCTF static/capture notes. Codex's host can read `/Users/devonly/Developer/personal/NXCTF/site/docs/components/protocolCaptures.ts`, which holds the 226-byte Combined sample. Copy only its payload into a small InkScope test fixture with provenance and expected hash; do not construct the positive case solely from the decoder's bit rules. The current capture `/Users/devonly/captures/inkscope-20261027-075711.pcap` has real Common and Coop samples; save bounded payload-only fixtures from them without keys or unrelated traffic. The existing `crates/packet-capture/src/preview.rs` implements `WavePreviewer` and its four conditional scenarios.
- Capture timestamp differences include endpoint processing and capture-point effects. A conditional WAVE preview is not evidence that the previewed event or tide was applied.
