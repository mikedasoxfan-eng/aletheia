# Roadmap (Initial Slices)

## Slice 0: Bootstrap Workspace
- Acceptance criteria:
  - `cargo test --workspace` passes.
  - GitHub repo exists and has the bootstrap push.
  - Repository has ROM ignore conventions and architecture notes.
- Artifacts:
  - Rust workspace crates.
  - `README.md`, `docs/architecture.md`, this roadmap.

## Slice 1: Deterministic Headless Contract
- Acceptance criteria:
  - Shared run contract supports cycle-based stepping and input replay.
  - Same core + same replay always yield identical frame/audio hashes.
  - Unit tests validate deterministic ordering and digest stability.
- Artifacts:
  - `aletheia-core` determinism/replay module.
  - Determinism test suite.

## Slice 2: GB/NES Skeleton Cores + Lab Runner
- Acceptance criteria:
  - CLI can run GB or NES smoke execution headlessly.
  - CLI emits machine-readable JSON with run hashes and metadata.
  - Golden smoke tests assert reproducible outputs.
- Artifacts:
  - `aletheia-gb` and `aletheia-nes` deterministic placeholder cores.
  - `aletheia-lab-cli` smoke command.

## Slice 2B: ROM File Runner + GBA Bootstrap Path
- Acceptance criteria:
  - CLI can auto-detect and run `.gb`, `.gbc`, `.nes`, `.gba` files headlessly.
  - ROM-run command emits deterministic `run.json`, `run.html`, and replay trace artifacts.
  - GB/NES load from cartridge bytes instead of hardcoded boot snippets only.
- Artifacts:
  - ROM metadata loader in `aletheia-core`.
  - Cartridge modules for GB and NES.
  - `aletheia-gba` bootstrap deterministic core.
  - `aletheia-lab-cli run-rom` command.

## Slice 2C: Expanded Decode + Mapper/Timer Foundations
- Acceptance criteria:
  - GB core executes broader control-flow/stack/ALU subset and services timer IRQs.
  - NES core supports strict unsupported-op errors and MMC1 PRG banking.
  - GBA core executes a broader ARM/THUMB subset and fails fast on unsupported instructions.
  - `cargo test --workspace` remains green.
- Artifacts:
  - GB timer/interrupt module and expanded CPU tests.
  - NES MMC1 mapper logic and expanded CPU tests.
  - GBA ARM/THUMB decoder expansion and fault-path tests.

## Slice 3: First Real CPU Verticals
- Acceptance criteria:
  - GB CPU executes a small instruction subset with flag-accurate unit tests.
  - NES CPU executes reset/vector fetch + baseline opcodes with tests.
  - Regression command can run selected public ROM tests and summarize pass/fail.
- Artifacts:
  - CPU modules + instruction tests.
  - JSON + HTML report scaffold for regression outputs.

## Slice 4: Save State + Replay Compatibility Envelope
- Acceptance criteria:
  - Versioned save-state format round-trips across runs.
  - Replays remain deterministic when loading from save-state checkpoints.
  - Compatibility checks fail fast on version mismatch.
- Artifacts:
  - Save-state schema crate/module.
  - Replay compatibility tests.
