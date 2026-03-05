# Design 0005: Compatibility and Differential Harness Commands

## Scope
Add practical regression-lab workflows for multi-ROM compatibility sweeps and deterministic hash diffing against reference reports.

## Commands
- `compat`: recursively discovers `.gb/.gbc/.nes/.gba` files under a directory, runs each with deterministic replay settings, and emits compatibility artifacts.
- `diff-rom`: runs one ROM locally and compares frame/audio hashes against a reference JSON report.

## Artifacts
- `compat.json` and `compat.html` with per-ROM pass/fail rows.
- `diff.json` and `diff.html` with local vs reference hash comparison.
- `replay.trace.txt` in each output folder.

## Validation
- Unit tests for reference digest parsing across schema variants.
- Unit tests for supported ROM extension filtering.
- Full workspace tests are required to pass.

## Known Gaps
- Current diff path expects pre-generated reference JSON; it does not yet invoke external emulator binaries directly.
- Compatibility command currently runs sequentially; parallel execution and timeout budgets are pending.
