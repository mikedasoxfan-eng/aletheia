# Open Issues (Seed List)

1. Define authoritative timing references for GB timer/divider edge behavior.
2. Decide save-state binary format (postcard/bincode/custom) and forward-compat policy.
3. Establish naming/attribution policy for public test ROM suites in docs.
4. Design differential harness interface for user-provided reference emulator binaries.
5. Specify HTML report schema and retained failure artifacts.
6. Expand GB opcode coverage and add table-driven instruction tests.
7. Expand NES opcode coverage and define behavior for unsupported opcodes in strict mode.
8. Add structured failure artifacts (trace + replay snapshot) to suite output folders.
9. Implement GB MBC3 RTC semantics and battery-backed RAM handling.
10. Implement NES mapper expansion (MMC1, UxROM, CNROM) from failing ROM compatibility cases.
11. Expand GBA core beyond bootstrap subset (THUMB decode, interrupt model, timers).
