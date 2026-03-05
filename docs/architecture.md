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
- Save states will be versioned and tied to replay compatibility constraints.

## Expansion Plan
1. Establish deterministic run contract and smoke harness.
2. Land minimal CPU+bus execution for GB and NES with instruction tests.
3. Introduce timing-critical subsystems (timers, interrupts, PPU) with ROM-suite gating.
4. Expand mapper/MBC coverage driven by regression failures.
