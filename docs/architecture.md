# Architecture Notes (Bootstrap)

## Goal
Build cycle-driven, deterministic emulation cores with shared replay/hash infrastructure so correctness can be validated with automated differential tests.

## Runtime Model
- Core components advance in cycles, not frames.
- Input is represented as cycle-stamped events.
- Headless execution emits stable frame/audio hashes.

## Determinism Guardrails
- No wall-clock dependence in core code.
- Replay logs are versioned, sortable, and serialized.
- Mid-run deterministic checkpoint verification is available in the lab runner (`--checkpoint-cycle`) and compares full baseline vs resumed digests.
- Save-state schema remains versioned work-in-progress and is tied to replay compatibility constraints.

## Lab Harness Notes
- `compat` supports parallel workers (`--jobs`) and optional per-ROM timeout controls (`--timeout-ms`) for large ROM sets.
- `diff-rom` can either consume a JSON report (`--reference-report`) or auto-run an external reference executable (`--reference-exe` + `--reference-arg` placeholders).
- HTML reports are generated alongside JSON artifacts for visual triage on local machines (including Windows PowerShell workflows).

## Current GBA AV Slice
- The GBA core now includes explicit bus-backed VRAM/palette/OAM/IO regions.
- A bootstrap PPU path advances scanline timing and renders mode 3/4 pixels to a 240x160 framebuffer.
- A bootstrap APU path reads sound-control registers and emits deterministic samples when master sound is enabled.
- `play-rom` default live mode now uses this internal framebuffer/audio path (still pre-fidelity relative to hardware).

## Expansion Plan
1. Establish deterministic run contract and smoke harness.
2. Land minimal CPU+bus execution for GB and NES with instruction tests.
3. Introduce timing-critical subsystems (timers, interrupts, PPU) with ROM-suite gating.
4. Expand mapper/MBC coverage driven by regression failures.
