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
- Expanded CPU/timing coverage:
  - GB: broader control-flow/ALU/load-store subset (`JP/JR/CALL/RET/RETI`, `ADD/CP`, `LD r,d8`, `LD (nn),A`, `LD A,(nn)`, `DI/EI/HALT`) plus basic timer + interrupt request handling.
  - NES: broader official opcode subset (`LDA/LDX/LDY`, `STA/STX/STY`, `ADC/SBC`, `CMP/CPX/CPY`, `JSR/RTS`, `JMP`, `BNE/BEQ`, `CLC/SEC`, `AND/ORA/EOR`) with strict unsupported-op errors.
  - GBA: broader ARM/THUMB bootstrap decode path with strict unsupported-op errors (fails fast instead of silent no-op).
- Headless lab CLI:
  - `smoke` command for single-system JSON output.
  - `suite` command for multi-system `summary.json` + `summary.html`.
  - `run-rom` command for user ROM files with auto-detect support for:
    - `.gb`
    - `.gbc`
    - `.nes` (iNES, NROM mapper path)
    - `.gba` (deterministic bootstrap core path)
- Unit tests covering instruction behavior, flags, reset/vector behavior, cycle counts, and determinism.

## Current ROM Testing Flow
Use local files under `USER_SUPPLIED_ROMS/` and run the headless lab CLI:

```bash
cargo run -p aletheia-lab-cli -- run-rom USER_SUPPLIED_ROMS/samples/demo.gb --cycles 100000 --output-dir lab-output/run-rom
```

Artifacts produced:
- `run.json` (machine-readable summary with ROM metadata and hashes)
- `run.html` (human-readable report)
- `replay.trace.txt` (canonical replay event trace)

Not implemented yet:
- Full CPU coverage and cycle-accurate timing behavior.
- PPU/APU/DMA fidelity and full interrupt/timer edge-case coverage.
- Full mapper/MBC coverage (MMC1/NROM and MBC1 scaffolds are present; MBC3 RTC, MBC5, and additional NES mappers are pending).
- Save-state schema and deterministic replay checkpoints.
- Differential harness against external reference emulators.
