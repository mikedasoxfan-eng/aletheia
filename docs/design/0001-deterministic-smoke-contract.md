# Design 0001: Deterministic Smoke Contract

## Scope
Define the first executable contract shared by all emulator cores: cycle-based execution with cycle-stamped input replay and stable frame/audio digests.

## Correctness Target (for this slice)
- Replays are explicit, versioned, and sorted canonically.
- A deterministic machine run is a pure function of `(core implementation, replay log, cycle budget)`.
- Reported frame/audio digests are stable across repeated runs.

## Validation
- Unit tests assert reproducibility of digests for identical inputs.
- Unit tests assert insertion-order independence for same-cycle events.
- Unit tests reject unsupported replay versions.

## Known Gaps / TODO
- Digest sampling model is placeholder and will be replaced by real PPU/APU outputs.
- Save-state format and replay checkpoint semantics are not implemented yet.
- Differential execution against reference emulators is still pending.
