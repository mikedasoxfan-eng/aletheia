# Design 0002: Minimal CPU Vertical (GB + NES)

## Scope
Introduce first real instruction execution for both systems using tiny opcode subsets, while keeping deterministic replay/hash pipeline unchanged.

## Correctness Target (this slice)
- CPU reset state is explicit and testable.
- Fetch/decode/execute path mutates registers and flags correctly for selected opcodes.
- Instruction cycle counts are explicit and drive execution cadence in deterministic tick loop.

## Initial Opcode Set
- GB: `NOP (0x00)`, `LD A,d8 (0x3E)`, `INC A (0x3C)`, `DEC A (0x3D)`, `XOR A (0xAF)`.
- NES: `NOP (0xEA)`, `LDA #imm (0xA9)`, `TAX (0xAA)`, `INX (0xE8)`, `DEX (0xCA)`.

## Validation
- Unit tests assert register and flag behavior for implemented opcodes.
- Unit tests assert PC progression and instruction-cycle accounting.
- Smoke digest tests remain reproducible for fixed replay and cycle budget.

## Known Gaps / TODO
- This is not full CPU compatibility and excludes interrupts, memory map side effects, and unofficial opcodes.
- PPU/APU/timer coupling is not yet modeled.
