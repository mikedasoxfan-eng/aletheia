use aletheia_core::{
    DeterminismError, DeterministicMachine, InputEvent, ReplayLog, RunDigest, SystemId,
    run_deterministic,
};

#[derive(Debug, Default)]
pub struct NesCore {
    bus_mix: u32,
}

impl DeterministicMachine for NesCore {
    fn system_id(&self) -> SystemId {
        SystemId::Nes
    }

    fn reset(&mut self) {
        self.bus_mix = 0x8161_53D1;
    }

    fn tick(&mut self, cycle: u64, input_events: &[InputEvent]) -> (u8, i16) {
        for event in input_events {
            self.bus_mix = self.bus_mix.rotate_right(3)
                ^ ((event.port as u32) << 20)
                ^ ((event.button as u32) << 10)
                ^ ((event.state as u32) << 16)
                ^ (!cycle as u32);
        }

        let frame = self.bus_mix.wrapping_mul(17).wrapping_add(cycle as u32) as u8;
        let audio =
            ((self.bus_mix ^ 0x7F4A_7C15).wrapping_add((cycle as u32) << 1) & 0x7FFF) as i16;
        (frame, audio)
    }
}

pub fn smoke_digest(cycles: u64, replay: &ReplayLog) -> Result<RunDigest, DeterminismError> {
    run_deterministic(&mut NesCore::default(), cycles, replay)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aletheia_core::{InputButton, InputState};

    fn replay_fixture() -> ReplayLog {
        ReplayLog::from(vec![
            InputEvent {
                cycle: 3,
                port: 0,
                button: InputButton::Select,
                state: InputState::Pressed,
            },
            InputEvent {
                cycle: 12,
                port: 0,
                button: InputButton::B,
                state: InputState::Pressed,
            },
            InputEvent {
                cycle: 18,
                port: 0,
                button: InputButton::B,
                state: InputState::Released,
            },
        ])
    }

    #[test]
    fn smoke_digest_is_reproducible() {
        let replay = replay_fixture();
        let first = smoke_digest(160, &replay).expect("smoke run should succeed");
        let second = smoke_digest(160, &replay).expect("smoke run should succeed");

        assert_eq!(first, second);
        assert_eq!(first.system, SystemId::Nes);
        assert_eq!(first.applied_events, 3);
    }
}
