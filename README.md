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
This repository is currently in bootstrap phase. The first slices establish deterministic execution contracts, artifact formats, and test harness plumbing before subsystem fidelity work.
