# Design 0003: ROM Loading and Multi-System Runner

## Scope
Add practical ROM-file testing flow for `.gb/.gbc/.nes/.gba` with deterministic outputs and artifact generation.

## Correctness Target (this slice)
- ROM type detection is explicit and testable.
- GB/NES execution can run against actual ROM bytes through cartridge/bus interfaces.
- GBA files are accepted and executed by a deterministic ARM-state bootstrap core.
- CLI can run one ROM file and emit `run.json`, `run.html`, and replay trace artifacts.

## Validation
- Unit tests for ROM metadata parsing and format detection.
- Unit tests for GB MBC1 bank switching.
- Unit tests for NES iNES/NROM mapping behavior.
- Unit tests for GBA opcode subset execution and deterministic replay.
- Integration-level CLI tests for HTML/report rendering.

## Known Gaps / TODO
- GB MBC3/MBC5 are currently skeleton-level, without RTC/battery behavior.
- NES mappers beyond NROM are not implemented.
- GBA core currently supports a tiny ARM opcode subset and no THUMB/BIOS/APU/PPU.
- This is deterministic test execution, not yet cycle-accurate gameplay emulation.
