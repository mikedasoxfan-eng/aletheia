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
  - `compat` command for recursive directory compatibility runs, with `--jobs` parallel workers and optional per-ROM `--timeout-ms`.
  - `diff-rom` command to compare local hashes against:
    - a reference JSON report (`--reference-report`), or
    - an external emulator executable (`--reference-exe`) that is auto-invoked.
  - Mid-run deterministic checkpoint verification for `run-rom` via `--checkpoint-cycle`.
  - Improved HTML reports with run metadata, timeout/checkpoint sections, and reference invocation details.
  - `play-rom` live mode with real-time frame window + audio playback from the deterministic core loop.
- Unit tests covering instruction behavior, flags, reset/vector behavior, cycle counts, and determinism.

## Current ROM Testing Flow
Use local files under `USER_SUPPLIED_ROMS/` and run the headless lab CLI:

```bash
cargo run -p aletheia-lab-cli -- run-rom USER_SUPPLIED_ROMS/samples/demo.gb --cycles 100000 --checkpoint-cycle 50000 --output-dir lab-output/run-rom
```

Artifacts produced:
- `run.json` (machine-readable summary with ROM metadata and hashes)
- `run.html` (human-readable report)
- `replay.trace.txt` (canonical replay event trace)

Additional harness commands:

```bash
cargo run -p aletheia-lab-cli -- compat USER_SUPPLIED_ROMS/samples --cycles 100000 --jobs 4 --timeout-ms 10000 --output-dir lab-output/compat
cargo run -p aletheia-lab-cli -- diff-rom USER_SUPPLIED_ROMS/samples/demo.gba --reference-report lab-output/run-rom-gba/run.json --cycles 1000 --output-dir lab-output/diff
cargo run -p aletheia-lab-cli -- diff-rom USER_SUPPLIED_ROMS/samples/demo.gba --reference-exe target/debug/aletheia-lab-cli --reference-arg run-rom --reference-arg {rom} --reference-arg=--cycles --reference-arg {cycles} --reference-arg=--output-dir --reference-arg {output_dir} --cycles 1000 --output-dir lab-output/diff-external
cargo run -p aletheia-lab-cli -- play-rom USER_SUPPLIED_ROMS/samples/demo.gba --fps 60 --sample-rate 48000
```

## Windows Quickstart
PowerShell command pattern:

```powershell
cargo run -p aletheia-lab-cli -- run-rom "C:\path\to\game.gba" --cycles 200000 --checkpoint-cycle 100000 --output-dir lab-output\gba-run
```

Open generated HTML reports on Windows:

```powershell
Start-Process (Resolve-Path .\lab-output\gba-run\run.html)
Start-Process (Resolve-Path .\lab-output\compat\compat.html)
Start-Process (Resolve-Path .\lab-output\diff\diff.html)
cargo run -p aletheia-lab-cli -- play-rom "C:\path\to\game.gba" --fps 60 --sample-rate 48000
```

Live mode notes:
- `Esc` quits, `P` pauses/resumes.
- Input mapping: `Z/X/Enter/Space/Arrow keys`.
- Current visuals/audio are deterministic core-driven preview output while full PPU/APU fidelity is still in progress.

Not implemented yet:
- Full CPU coverage and cycle-accurate timing behavior.
- PPU/APU/DMA fidelity and full interrupt/timer edge-case coverage.
- Full mapper/MBC coverage (MMC1/NROM and MBC1 scaffolds are present; MBC3 RTC, MBC5, and additional NES mappers are pending).
- Save-state schema and deterministic replay checkpoints.
- Differential harness execution against external emulator binaries (current diff path compares against reference JSON outputs).
