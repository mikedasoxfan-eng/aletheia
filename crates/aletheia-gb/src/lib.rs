mod bus;
mod cpu;

pub use bus::{GbBus, PROGRAM_START};
pub use cpu::{GbCpu, Registers, StepInfo};

use aletheia_core::{
    DeterminismError, DeterministicMachine, InputEvent, ReplayLog, RunDigest, SystemId,
    run_deterministic,
};

const BOOT_PROGRAM: [u8; 9] = [0x3E, 0x10, 0x3C, 0x3D, 0xAF, 0x3E, 0x42, 0x00, 0x00];
const JOYPAD_REG: u16 = 0xFF00;

#[derive(Debug, Default)]
pub struct DmgCore {
    cpu: GbCpu,
    bus: GbBus,
    cycles_until_next_step: u8,
    input_mix: u8,
}

impl DmgCore {
    pub fn cpu_regs(&self) -> Registers {
        self.cpu.regs()
    }
}

impl DeterministicMachine for DmgCore {
    fn system_id(&self) -> SystemId {
        SystemId::GbDmg
    }

    fn reset(&mut self) {
        self.bus.clear();
        self.bus.load_program(PROGRAM_START, &BOOT_PROGRAM);
        self.cpu.reset();
        self.cycles_until_next_step = 0;
        self.input_mix = 0;
    }

    fn tick(&mut self, cycle: u64, input_events: &[InputEvent]) -> (u8, i16) {
        for event in input_events {
            let mix = (event.button as u8) << 1 | (event.state as u8);
            self.input_mix = self.input_mix.rotate_left(1) ^ mix ^ event.port;
            self.bus.write8(JOYPAD_REG, self.input_mix);
        }

        if self.cycles_until_next_step == 0 {
            let step = self.cpu.step(&mut self.bus);
            self.cycles_until_next_step = step.cycles.saturating_sub(1);
        } else {
            self.cycles_until_next_step -= 1;
        }

        let regs = self.cpu.regs();
        let frame = regs.a ^ regs.f ^ self.input_mix ^ (cycle as u8);
        let audio = (((regs.pc as i32) << 2)
            ^ ((self.cycles_until_next_step as i32) << 7)
            ^ ((self.input_mix as i32) << 10)) as i16;
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

    #[test]
    fn cpu_executes_boot_program_deterministically() {
        let replay = replay_fixture();
        let mut core = DmgCore::default();
        run_deterministic(&mut core, 40, &replay).expect("run should succeed");
        let regs = core.cpu_regs();
        assert_eq!(regs.a, 0x42);
    }
}
