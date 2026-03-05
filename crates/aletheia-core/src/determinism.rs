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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DeterminismError {
    #[error("unsupported replay log version {found}; expected {expected}")]
    UnsupportedReplayVersion { expected: u16, found: u16 },
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

    Ok(RunDigest {
        schema_version: 1,
        replay_version: replay.version,
        system: machine.system_id(),
        executed_cycles: cycles,
        applied_events,
        frame_hash: frame_hasher.finalize().to_hex().to_string(),
        audio_hash: audio_hasher.finalize().to_hex().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::{InputButton, InputState};

    #[derive(Default)]
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
}
