# Design 0004: CPU Expansion and Strict Decode Faults

## Scope
Advance all initial system cores from minimal bootstrap behavior toward practical test-ROM execution by expanding opcode subsets, mapper behavior, and fault handling.

## Changes in This Slice
- GB:
  - Expanded instruction subset to include control-flow, stack, load/store, and ALU/flag operations.
  - Added cycle-driven timer model with TIMA/TMA/TAC/DIV progression.
  - Added interrupt servicing path (IF/IE + vector dispatch, RETI path).
- NES:
  - Added MMC1 mapper behavior (serial register writes + PRG banking modes).
  - Expanded 6502 opcode coverage and flag behavior.
  - Added strict unsupported-opcode errors (no silent fallback).
- GBA:
  - Expanded ARM decode (data processing, branch, BX, basic load/store).
  - Added THUMB immediate/branch/BX subset.
  - Added strict ARM/THUMB unsupported-op faults.

## Validation
- Unit tests for GB timer overflow IRQ signaling and interrupt vector execution.
- Unit tests for MMC1 bank switching and expanded NES opcode behavior.
- Unit tests for ARM/THUMB execution and strict unsupported-op failures.
- Full workspace test pass required for each commit.

## Known Limits
- Timing is still simplified and not hardware-cycle exact.
- GBA decode remains partial and is not sufficient for broad commercial compatibility.
- NES mapper support beyond NROM/MMC1 remains pending.
