# Aletheia

Aletheia is an accuracy-first emulator platform and regression lab for 80s/90s game systems.

## Principles
- Determinism over speed.
- Reproducible failures over ad-hoc debugging.
- Small, reviewable increments that keep CI green.

## Initial Scope
- Game Boy (DMG + CGB)
- NES/Famicom

## Workspace Layout
- `crates/aletheia-core`: shared deterministic primitives and run orchestration.
- `crates/aletheia-gb`: Game Boy emulation core.
- `crates/aletheia-nes`: NES emulation core.
- `crates/aletheia-lab-cli`: headless compatibility + regression runner.
- `docs/`: architecture notes and implementation roadmap.
- `USER_SUPPLIED_ROMS/`: ignored local ROM/test binaries.

## Current Status
Implemented today:
- Deterministic replay contract (`ReplayLog`) with canonical event ordering.
- Stable run digests with frame/audio hashes for fixed `(system, replay, cycles)`.
- Minimal executable CPU verticals:
  - GB: `NOP`, `LD A,d8`, `INC A`, `DEC A`, `XOR A`.
  - NES: `NOP`, `LDA #imm`, `TAX`, `INX`, `DEX`.
- Headless lab CLI:
  - `smoke` command for single-system JSON output.
  - `suite` command for multi-system `summary.json` + `summary.html`.
- Unit tests covering instruction behavior, flags, reset/vector behavior, cycle counts, and determinism.

Not implemented yet:
- Full CPU coverage and cycle-accurate timing behavior.
- PPU/APU, interrupts, timers, DMA, mappers/MBCs.
- Save-state schema and deterministic replay checkpoints.
- Differential harness against external reference emulators.
