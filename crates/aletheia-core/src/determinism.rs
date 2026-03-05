use crate::replay::{InputEvent, ReplayLog};
use crate::system::SystemId;
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunDigest {
    pub schema_version: u16,
    pub replay_version: u16,
    pub system: SystemId,
    pub executed_cycles: u64,
    pub applied_events: usize,
    pub frame_hash: String,
    pub audio_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointDigest {
    pub checkpoint_cycle: u64,
    pub baseline: RunDigest,
    pub resumed: RunDigest,
    pub digests_match: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DeterminismError {
    #[error("unsupported replay log version {found}; expected {expected}")]
    UnsupportedReplayVersion { expected: u16, found: u16 },
    #[error("invalid checkpoint cycle {checkpoint}; total cycles {total}")]
    InvalidCheckpoint { checkpoint: u64, total: u64 },
}

pub trait DeterministicMachine {
    fn system_id(&self) -> SystemId;
    fn reset(&mut self);

    fn tick(&mut self, cycle: u64, input_events: &[InputEvent]) -> (u8, i16);
}

pub fn run_deterministic<M: DeterministicMachine>(
    machine: &mut M,
    cycles: u64,
    replay: &ReplayLog,
) -> Result<RunDigest, DeterminismError> {
    if replay.version != ReplayLog::CURRENT_VERSION {
        return Err(DeterminismError::UnsupportedReplayVersion {
            expected: ReplayLog::CURRENT_VERSION,
            found: replay.version,
        });
    }

    machine.reset();
    let sorted_events = replay.sorted_events();
    let mut next_event_index = 0usize;
    let mut applied_events = 0usize;
    let mut frame_hasher = Hasher::new();
    let mut audio_hasher = Hasher::new();

    for cycle in 0..cycles {
        let start = next_event_index;
        while next_event_index < sorted_events.len()
            && sorted_events[next_event_index].cycle == cycle
        {
            next_event_index += 1;
        }
        let cycle_events = &sorted_events[start..next_event_index];
        applied_events += cycle_events.len();

        let (frame_sample, audio_sample) = machine.tick(cycle, cycle_events);
        let cycle_bytes = cycle.to_le_bytes();
        frame_hasher.update(&cycle_bytes);
        frame_hasher.update(&[frame_sample]);
        audio_hasher.update(&cycle_bytes);
        audio_hasher.update(&audio_sample.to_le_bytes());
    }

    Ok(build_digest(
        machine.system_id(),
        replay.version,
        cycles,
        applied_events,
        frame_hasher,
        audio_hasher,
    ))
}

pub fn run_deterministic_with_checkpoint<M: DeterministicMachine + Clone>(
    machine: &mut M,
    cycles: u64,
    replay: &ReplayLog,
    checkpoint_cycle: u64,
) -> Result<CheckpointDigest, DeterminismError> {
    if replay.version != ReplayLog::CURRENT_VERSION {
        return Err(DeterminismError::UnsupportedReplayVersion {
            expected: ReplayLog::CURRENT_VERSION,
            found: replay.version,
        });
    }
    if checkpoint_cycle >= cycles {
        return Err(DeterminismError::InvalidCheckpoint {
            checkpoint: checkpoint_cycle,
            total: cycles,
        });
    }

    machine.reset();
    let sorted_events = replay.sorted_events();
    let mut next_event_index = 0usize;
    let mut applied_events = 0usize;
    let mut frame_hasher = Hasher::new();
    let mut audio_hasher = Hasher::new();

    let mut checkpoint_machine = if checkpoint_cycle == 0 {
        Some(machine.clone())
    } else {
        None
    };
    let mut checkpoint_next_event_index = 0usize;
    let mut checkpoint_applied_events = 0usize;
    let mut checkpoint_frame_hasher = frame_hasher.clone();
    let mut checkpoint_audio_hasher = audio_hasher.clone();

    for cycle in 0..cycles {
        let start = next_event_index;
        while next_event_index < sorted_events.len()
            && sorted_events[next_event_index].cycle == cycle
        {
            next_event_index += 1;
        }
        let cycle_events = &sorted_events[start..next_event_index];
        applied_events += cycle_events.len();

        let (frame_sample, audio_sample) = machine.tick(cycle, cycle_events);
        let cycle_bytes = cycle.to_le_bytes();
        frame_hasher.update(&cycle_bytes);
        frame_hasher.update(&[frame_sample]);
        audio_hasher.update(&cycle_bytes);
        audio_hasher.update(&audio_sample.to_le_bytes());

        if cycle + 1 == checkpoint_cycle {
            checkpoint_machine = Some(machine.clone());
            checkpoint_next_event_index = next_event_index;
            checkpoint_applied_events = applied_events;
            checkpoint_frame_hasher = frame_hasher.clone();
            checkpoint_audio_hasher = audio_hasher.clone();
        }
    }

    let baseline = build_digest(
        machine.system_id(),
        replay.version,
        cycles,
        applied_events,
        frame_hasher,
        audio_hasher,
    );

    let mut resumed_machine = checkpoint_machine.expect("checkpoint machine should always exist");
    let mut resumed_next_event_index = checkpoint_next_event_index;
    let mut resumed_applied_events = checkpoint_applied_events;
    let mut resumed_frame_hasher = checkpoint_frame_hasher;
    let mut resumed_audio_hasher = checkpoint_audio_hasher;

    for cycle in checkpoint_cycle..cycles {
        let start = resumed_next_event_index;
        while resumed_next_event_index < sorted_events.len()
            && sorted_events[resumed_next_event_index].cycle == cycle
        {
            resumed_next_event_index += 1;
        }
        let cycle_events = &sorted_events[start..resumed_next_event_index];
        resumed_applied_events += cycle_events.len();

        let (frame_sample, audio_sample) = resumed_machine.tick(cycle, cycle_events);
        let cycle_bytes = cycle.to_le_bytes();
        resumed_frame_hasher.update(&cycle_bytes);
        resumed_frame_hasher.update(&[frame_sample]);
        resumed_audio_hasher.update(&cycle_bytes);
        resumed_audio_hasher.update(&audio_sample.to_le_bytes());
    }

    let resumed = build_digest(
        resumed_machine.system_id(),
        replay.version,
        cycles,
        resumed_applied_events,
        resumed_frame_hasher,
        resumed_audio_hasher,
    );

    Ok(CheckpointDigest {
        checkpoint_cycle,
        digests_match: baseline.frame_hash == resumed.frame_hash
            && baseline.audio_hash == resumed.audio_hash
            && baseline == resumed,
        baseline,
        resumed,
    })
}

fn build_digest(
    system: SystemId,
    replay_version: u16,
    cycles: u64,
    applied_events: usize,
    frame_hasher: Hasher,
    audio_hasher: Hasher,
) -> RunDigest {
    RunDigest {
        schema_version: 1,
        replay_version,
        system,
        executed_cycles: cycles,
        applied_events,
        frame_hash: frame_hasher.finalize().to_hex().to_string(),
        audio_hash: audio_hasher.finalize().to_hex().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::{InputButton, InputState};

    #[derive(Default, Clone)]
    struct DummyMachine {
        state: u32,
    }

    impl DeterministicMachine for DummyMachine {
        fn system_id(&self) -> SystemId {
            SystemId::GbDmg
        }

        fn reset(&mut self) {
            self.state = 0xC0DE_1234;
        }

        fn tick(&mut self, cycle: u64, input_events: &[InputEvent]) -> (u8, i16) {
            for event in input_events {
                self.state = self.state.rotate_left(3)
                    ^ (event.port as u32)
                    ^ ((event.button as u32) << 8)
                    ^ ((event.state as u32) << 16)
                    ^ (cycle as u32);
            }

            let frame = self.state.wrapping_add(cycle as u32) as u8;
            let audio = ((self.state ^ 0xA5A5_5A5A).wrapping_add(cycle as u32) & 0x7FFF) as i16;
            (frame, audio)
        }
    }

    fn event(cycle: u64, button: InputButton) -> InputEvent {
        InputEvent {
            cycle,
            port: 0,
            button,
            state: InputState::Pressed,
        }
    }

    #[test]
    fn deterministic_digest_is_stable_for_identical_inputs() {
        let replay = ReplayLog::from(vec![
            event(5, InputButton::A),
            event(10, InputButton::Start),
        ]);

        let digest_a = run_deterministic(&mut DummyMachine::default(), 128, &replay)
            .expect("replay should be valid");
        let digest_b = run_deterministic(&mut DummyMachine::default(), 128, &replay)
            .expect("replay should be valid");

        assert_eq!(digest_a, digest_b);
    }

    #[test]
    fn digest_does_not_depend_on_event_insertion_order() {
        let replay_a = ReplayLog::from(vec![
            event(8, InputButton::B),
            event(8, InputButton::A),
            event(8, InputButton::Start),
        ]);
        let replay_b = ReplayLog::from(vec![
            event(8, InputButton::Start),
            event(8, InputButton::A),
            event(8, InputButton::B),
        ]);

        let digest_a = run_deterministic(&mut DummyMachine::default(), 64, &replay_a)
            .expect("replay should be valid");
        let digest_b = run_deterministic(&mut DummyMachine::default(), 64, &replay_b)
            .expect("replay should be valid");

        assert_eq!(digest_a.frame_hash, digest_b.frame_hash);
        assert_eq!(digest_a.audio_hash, digest_b.audio_hash);
    }

    #[test]
    fn version_mismatch_is_rejected() {
        let replay = ReplayLog {
            version: 99,
            events: vec![],
        };

        let error = run_deterministic(&mut DummyMachine::default(), 16, &replay)
            .expect_err("invalid version should fail");

        assert_eq!(
            error,
            DeterminismError::UnsupportedReplayVersion {
                expected: ReplayLog::CURRENT_VERSION,
                found: 99
            }
        );
    }

    #[test]
    fn checkpoint_replay_matches_baseline() {
        let replay = ReplayLog::from(vec![
            event(2, InputButton::A),
            event(17, InputButton::Start),
            event(30, InputButton::B),
        ]);
        let result =
            run_deterministic_with_checkpoint(&mut DummyMachine::default(), 64, &replay, 20)
                .expect("checkpoint run should work");
        assert!(result.digests_match);
        assert_eq!(result.baseline, result.resumed);
    }

    #[test]
    fn invalid_checkpoint_is_rejected() {
        let replay = ReplayLog::new();
        let error =
            run_deterministic_with_checkpoint(&mut DummyMachine::default(), 10, &replay, 10)
                .expect_err("checkpoint equal to end should fail");
        assert_eq!(
            error,
            DeterminismError::InvalidCheckpoint {
                checkpoint: 10,
                total: 10
            }
        );
    }
}
