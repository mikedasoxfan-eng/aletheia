mod bus;
mod cartridge;
mod cpu;

pub use bus::{NesBus, PROGRAM_START, RESET_VECTOR};
pub use cartridge::{
    CartridgeError as NesCartridgeError, CartridgeInfo as NesCartridgeInfo, MapperKind,
};
pub use cpu::{NesCpu, Registers, StepInfo};

use aletheia_core::{
    DeterminismError, DeterministicMachine, InputEvent, ReplayLog, RunDigest, SystemId,
    run_deterministic,
};
use thiserror::Error;

const BOOT_PROGRAM: [u8; 7] = [0xA9, 0x11, 0xAA, 0xE8, 0xCA, 0xEA, 0xEA];
const CONTROLLER_PORT: u16 = 0x4016;

#[derive(Debug, Default)]
pub struct NesCore {
    cpu: NesCpu,
    bus: NesBus,
    cycles_until_next_step: u8,
    input_mix: u8,
}

impl NesCore {
    pub fn cpu_regs(&self) -> Registers {
        self.cpu.regs()
    }

    pub fn load_rom(&mut self, rom: &[u8]) -> Result<(), NesCartridgeError> {
        self.bus.load_cartridge(rom)?;
        Ok(())
    }
}

impl DeterministicMachine for NesCore {
    fn system_id(&self) -> SystemId {
        SystemId::Nes
    }

    fn reset(&mut self) {
        self.bus.clear_runtime();
        if !self.bus.has_cartridge() {
            self.bus.load_program(PROGRAM_START, &BOOT_PROGRAM);
            self.bus.set_reset_vector(PROGRAM_START);
        }
        self.cpu.reset(&self.bus);
        self.cycles_until_next_step = 0;
        self.input_mix = 0;
    }

    fn tick(&mut self, cycle: u64, input_events: &[InputEvent]) -> (u8, i16) {
        for event in input_events {
            let mix = (event.button as u8) << 1 | (event.state as u8);
            self.input_mix = self.input_mix.rotate_right(1) ^ mix ^ event.port;
            self.bus.write8(CONTROLLER_PORT, self.input_mix);
        }

        if self.cycles_until_next_step == 0 {
            let step = self.cpu.step(&mut self.bus);
            self.cycles_until_next_step = step.cycles.saturating_sub(1);
        } else {
            self.cycles_until_next_step -= 1;
        }

        let regs = self.cpu.regs();
        let frame = regs.a ^ regs.x ^ regs.p ^ self.input_mix ^ (cycle as u8);
        let audio = (((regs.pc as i32) << 2)
            ^ ((self.cycles_until_next_step as i32) << 8)
            ^ ((self.input_mix as i32) << 10)) as i16;
        (frame, audio)
    }
}

pub fn smoke_digest(cycles: u64, replay: &ReplayLog) -> Result<RunDigest, DeterminismError> {
    run_deterministic(&mut NesCore::default(), cycles, replay)
}

#[derive(Debug, Error)]
pub enum NesRunError {
    #[error("{0}")]
    Cartridge(#[from] NesCartridgeError),
    #[error("{0}")]
    Determinism(#[from] DeterminismError),
}

pub fn run_rom_digest(
    cycles: u64,
    replay: &ReplayLog,
    rom: &[u8],
) -> Result<RunDigest, NesRunError> {
    let mut core = NesCore::default();
    core.load_rom(rom)?;
    Ok(run_deterministic(&mut core, cycles, replay)?)
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

    #[test]
    fn cpu_executes_boot_program_deterministically() {
        let replay = replay_fixture();
        let mut core = NesCore::default();
        run_deterministic(&mut core, 16, &replay).expect("run should succeed");
        let regs = core.cpu_regs();
        assert_eq!(regs.a, 0x11);
        assert_eq!(regs.x, 0x11);
    }

    #[test]
    fn run_rom_digest_executes_headered_nrom() {
        let replay = replay_fixture();
        let mut rom = vec![0; 16 + (16 * 1024)];
        rom[..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1; // 16KB PRG
        rom[16] = 0xA9;
        rom[17] = 0x55;
        rom[18] = 0xEA;
        // reset vector for NROM-128 mirrored into upper half
        let reset_lo_index = 16 + 0x3FFC;
        let reset_hi_index = 16 + 0x3FFD;
        rom[reset_lo_index] = 0x00;
        rom[reset_hi_index] = 0x80;

        let digest = run_rom_digest(32, &replay, &rom).expect("rom run");
        assert_eq!(digest.system, SystemId::Nes);
        assert_eq!(digest.applied_events, 3);
    }
}
