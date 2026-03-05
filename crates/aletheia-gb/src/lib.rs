use aletheia_core::{
    DeterminismError, DeterministicMachine, InputEvent, ReplayLog, RunDigest, SystemId,
    run_deterministic,
};

#[derive(Debug, Default)]
pub struct DmgCore {
    cpu_mix: u32,
}

impl DeterministicMachine for DmgCore {
    fn system_id(&self) -> SystemId {
        SystemId::GbDmg
    }

    fn reset(&mut self) {
        self.cpu_mix = 0x01D0_3F21;
    }

    fn tick(&mut self, cycle: u64, input_events: &[InputEvent]) -> (u8, i16) {
        for event in input_events {
            self.cpu_mix = self.cpu_mix.rotate_left(5)
                ^ ((event.port as u32) << 24)
                ^ ((event.button as u32) << 8)
                ^ ((event.state as u32) << 16)
                ^ (cycle as u32);
        }

        let frame = self.cpu_mix.wrapping_add(cycle as u32) as u8;
        let audio = ((self.cpu_mix ^ 0x9E37_79B9).wrapping_add(cycle as u32) & 0x7FFF) as i16;
        (frame, audio)
    }
}

pub fn smoke_digest(cycles: u64, replay: &ReplayLog) -> Result<RunDigest, DeterminismError> {
    run_deterministic(&mut DmgCore::default(), cycles, replay)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aletheia_core::{InputButton, InputState};

    fn replay_fixture() -> ReplayLog {
        ReplayLog::from(vec![
            InputEvent {
                cycle: 4,
                port: 0,
                button: InputButton::Start,
                state: InputState::Pressed,
            },
            InputEvent {
                cycle: 7,
                port: 0,
                button: InputButton::A,
                state: InputState::Pressed,
            },
            InputEvent {
                cycle: 10,
                port: 0,
                button: InputButton::A,
                state: InputState::Released,
            },
        ])
    }

    #[test]
    fn smoke_digest_is_reproducible() {
        let replay = replay_fixture();
        let first = smoke_digest(128, &replay).expect("smoke run should succeed");
        let second = smoke_digest(128, &replay).expect("smoke run should succeed");

        assert_eq!(first, second);
        assert_eq!(first.system, SystemId::GbDmg);
        assert_eq!(first.applied_events, 3);
    }
}
