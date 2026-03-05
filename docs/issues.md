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
10. Implement NES mapper expansion beyond NROM/MMC1 (UxROM, CNROM, MMC3) from failing ROM compatibility cases.
11. Expand GBA core beyond bootstrap subset (THUMB decode, interrupt model, timers).
12. Add opcode-coverage metrics against selected public test ROM suites (per-system progress report).
13. Add strict/lenient decode mode flags so unsupported op behavior can be tuned per workflow.
14. Add external-emulator invocation mode for `diff-rom` (run reference executable paths directly).
15. Add parallel execution + timeout controls for `compat` directory runs.
