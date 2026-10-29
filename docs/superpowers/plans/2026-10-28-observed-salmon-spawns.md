# Observed Salmon Spawn Display Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans for task-by-task implementation.

**Goal:** Show authenticated boss spawn request order and evidence-backed static direction candidates beside the existing WAVE preview.

**Architecture:** Keep packet fields and generated WAVE previews separate. A stateful observer reads authenticated OnlineCoopSetting and CoopEnemySpawnNetEvent rows, numbers observed boss requests, and matches the decoded request XYZ to pinned 11.3.0 stage points only when the match is unique and precise. The console labels those matches as candidates.

**Tech Stack:** Rust workspace, `inkscope` and `ocean-calc`; saved authenticated PCAP for integration verification.

**Spec:** The user's current conversation requests boss spawn order and direction, packet comparison, correct normal-stage Big Run handling, and named Protocol 5 states. `AGENTS.md` defines capture and evidence boundaries.

## Global Constraints

- Never call a spawn request a confirmed created enemy or current enemy position.
- Direction numbers 0–2 are internal stage groups, not compass directions.
- A transmitted GameSeed does not prove scene-seed adoption.
- Keep raw PCAP storage independent of display filtering.

## Review Focus

- Duplicate or unauthenticated event rows must not silently become confirmed spawn facts.
- Unknown stages and imprecise static matches must leave direction unassigned.
- A new OnlineCoopSetting must reset the observed request order.
- Specialized boss callbacks can differ from generic stage points; label all matches as geometry candidates.
- Normal stages should not show Big Run scenarios; unknown stage semantics must stay conditional.

---

### Task 1: Name recognized Protocol 5 states in text

**Files:** `src/render.rs`, `README.md`, `AGENTS.md`.

- [x] Preserve decoder `kind` in text instead of replacing it with UnreliableProtocol.
- [x] Verify 1104, 1107, 1108 against the saved PCAP.

### Task 2: Correct stage name and normal-stage preview

**Files:** `src/decode.rs`, `src/preview.rs`, their unit tests.

- [x] Test `Cop_Shakeship` name from owned localization and regular-stage scenario count.
- [x] Implement confirmed regular-stage handling; retain conditional global-word branches.
- [x] Run unit tests and inspect #21580.

### Task 3: Annotate observed boss requests

**Files:** `src/spawn.rs`, `src/lib.rs`, `src/main.rs`, `src/render.rs`, `test_support/unit/spawn.rs`.

- [x] Test #22390 Pillar request against the 11.3.0 tide1/direction0 anchor, plus reset and unknown cases.
- [x] Observe authenticated event order and add derived candidate fields only for unique precise static matches.
- [x] Render request order and candidate caveat in text; retain derived fields in JSONL.
- [x] Verify built CLI against live saved PCAP #22390 and previous #1609 without treating a geometry mismatch as prediction failure.

### Task 4: Review and integration

- [x] Run focused tests, format check, and saved PCAP inspection.
- [x] Request independent diff review where worker access permits; resolve material findings.
