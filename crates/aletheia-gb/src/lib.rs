mod bus;
mod cartridge;
mod cpu;
mod timer;

pub use bus::{GbBus, PROGRAM_START};
pub use cartridge::{
    CartridgeError as GbCartridgeError, CartridgeInfo as GbCartridgeInfo, MbcKind,
};
pub use cpu::{GbCpu, Registers, StepInfo};
pub use timer::GbTimer;

use aletheia_core::{
    DeterminismError, DeterministicMachine, InputEvent, ReplayLog, RunDigest, SystemId,
    run_deterministic,
};
use thiserror::Error;

const BOOT_PROGRAM: [u8; 9] = [0x3E, 0x10, 0x3C, 0x3D, 0xAF, 0x3E, 0x42, 0x00, 0x00];
const JOYPAD_REG: u16 = 0xFF00;

#[derive(Debug)]
pub struct DmgCore {
    cpu: GbCpu,
    bus: GbBus,
    timer: GbTimer,
    cycles_until_next_step: u8,
    input_mix: u8,
    system_id: SystemId,
}

impl Default for DmgCore {
    fn default() -> Self {
        Self {
            cpu: GbCpu::default(),
            bus: GbBus::default(),
            timer: GbTimer::default(),
            cycles_until_next_step: 0,
            input_mix: 0,
            system_id: SystemId::GbDmg,
        }
    }
}

impl DmgCore {
    pub fn cpu_regs(&self) -> Registers {
        self.cpu.regs()
    }

    pub fn load_rom(&mut self, rom: &[u8]) -> Result<(), GbCartridgeError> {
        self.bus.load_cartridge(rom)?;
        self.system_id = match self.bus.cartridge_info().map(|info| info.cgb_flag) {
            Some(0x80) | Some(0xC0) => SystemId::GbCgb,
            _ => SystemId::GbDmg,
        };
        Ok(())
    }
}

impl DeterministicMachine for DmgCore {
    fn system_id(&self) -> SystemId {
        self.system_id
    }

    fn reset(&mut self) {
        self.bus.clear_runtime();
        if !self.bus.has_cartridge() {
            self.bus.load_program(PROGRAM_START, &BOOT_PROGRAM);
            self.system_id = SystemId::GbDmg;
        }
        self.cpu.reset();
        self.timer.reset(&mut self.bus);
        self.cycles_until_next_step = 0;
        self.input_mix = 0;
    }

    fn tick(&mut self, cycle: u64, input_events: &[InputEvent]) -> (u8, i16) {
        for event in input_events {
            let mix = (event.button as u8) << 1 | (event.state as u8);
            self.input_mix = self.input_mix.rotate_left(1) ^ mix ^ event.port;
            self.bus.write8(JOYPAD_REG, self.input_mix);
        }

        self.timer.tick(&mut self.bus, 1);

        if self.cycles_until_next_step == 0 {
            if let Some(cycles) = self.cpu.service_interrupt(&mut self.bus) {
                self.cycles_until_next_step = cycles.saturating_sub(1);
            } else {
                let step = self.cpu.step(&mut self.bus);
                self.cycles_until_next_step = step.cycles.saturating_sub(1);
            }
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

#[derive(Debug, Error)]
pub enum GbRunError {
    #[error("{0}")]
    Cartridge(#[from] GbCartridgeError),
    #[error("{0}")]
    Determinism(#[from] DeterminismError),
}

pub fn run_rom_digest(
    cycles: u64,
    replay: &ReplayLog,
    rom: &[u8],
) -> Result<RunDigest, GbRunError> {
    let mut core = DmgCore::default();
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

    #[test]
    fn run_rom_digest_loads_cgb_rom_and_sets_system_id() {
        let replay = replay_fixture();
        let mut rom = vec![0; 0x8000];
        rom[0x100] = 0x3E;
        rom[0x101] = 0x99;
        rom[0x143] = 0xC0; // CGB only
        rom[0x147] = 0x00; // ROM only
        let digest = run_rom_digest(16, &replay, &rom).expect("rom run");
        assert_eq!(digest.system, SystemId::GbCgb);
    }

    #[test]
    fn timer_interrupt_path_jumps_to_vector() {
        let replay = ReplayLog::new();
        let mut rom = vec![0; 0x8000];
        rom[0x143] = 0x00;
        rom[0x147] = 0x00;

        // Main program at 0x100
        let mut i = 0x100usize;
        rom[i] = 0xF3; // DI
        i += 1;
        rom[i] = 0x3E; // LD A,04
        rom[i + 1] = 0x04;
        i += 2;
        rom[i] = 0xEA; // LD (FFFF),A -> IE
        rom[i + 1] = 0xFF;
        rom[i + 2] = 0xFF;
        i += 3;
        rom[i] = 0x3E; // LD A,FE
        rom[i + 1] = 0xFE;
        i += 2;
        rom[i] = 0xEA; // LD (FF05),A -> TIMA
        rom[i + 1] = 0x05;
        rom[i + 2] = 0xFF;
        i += 3;
        rom[i] = 0x3E; // LD A,80
        rom[i + 1] = 0x80;
        i += 2;
        rom[i] = 0xEA; // LD (FF06),A -> TMA
        rom[i + 1] = 0x06;
        rom[i + 2] = 0xFF;
        i += 3;
        rom[i] = 0x3E; // LD A,05 (timer enable, freq 16)
        rom[i + 1] = 0x05;
        i += 2;
        rom[i] = 0xEA; // LD (FF07),A -> TAC
        rom[i + 1] = 0x07;
        rom[i + 2] = 0xFF;
        i += 3;
        rom[i] = 0xFB; // EI
        i += 1;
        rom[i] = 0x76; // HALT

        // Timer ISR at 0x50: LD A,99 ; RETI
        rom[0x50] = 0x3E;
        rom[0x51] = 0x99;
        rom[0x52] = 0xD9;

        let mut core = DmgCore::default();
        core.load_rom(&rom).expect("rom load");
        run_deterministic(&mut core, 512, &replay).expect("run");
        assert_eq!(core.cpu_regs().a, 0x99);
    }
}
