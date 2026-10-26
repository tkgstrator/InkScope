# InkScope Workspace Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task by task.

**Goal:** Publish a standalone Rust workspace that decodes saved LAN PCAPs and shows conditional Salmon Run wave previews when a confirmed OnlineCoopSetting carries GameSeed.

**Architecture:** Keep the existing packet and lookahead crates separate. The packet CLI consumes the lookahead library through a path dependency and attaches a structured preview to the first confirmed setting with each distinct seed and context. Four scenarios cover the unknown IsBigRun flag and the two relevant global-word classes. Reuse generated stage and level data; never embed key material.

**Tech Stack:** Rust 2024, Cargo workspace, GitHub Actions, Rust devcontainer.

**Spec:** User requests in this session: protocol coverage README, external random table or game key, first-seed preview, Rust example devcontainer and workflows.

## Global Constraints

- Preserve original NXCTF tools and leave private keys, ELF, PCAP and raw assets outside this public repository.
- A preview is conditional on GameSeed becoming the scene seed and on 11.3.0 resource/runtime assumptions.
- Only the authenticated, confirmed 211-byte OnlineCoopSetting layout can trigger a preview.
- JSONL must carry the same structured prediction as terminal output.
- Do not treat generated event, tide, or nominal queue as applied game state or observed spawns.
- Clean Cargo build caches after local verification.

## Tasks

- [ ] Copy both Rust crates into a workspace and make all paths self-contained.
- [ ] Add a structured, deduplicated conditional preview with synthetic tests and a saved-PCAP smoke check.
- [ ] Adapt the Rust devcontainer and CI/deployment workflows for the workspace.
- [ ] Document protocol completeness, prediction assumptions, and key-source requirements.
- [ ] Run format, Clippy, tests, release build, private-material scan, cleanup, review, and guarded Git publication/readback.
