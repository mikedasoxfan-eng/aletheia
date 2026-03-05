use aletheia_core::{
    DeterminismError, DeterministicMachine, InputEvent, ReplayLog, RunDigest, SystemId,
    run_deterministic,
};
use thiserror::Error;

const ROM_BASE: u32 = 0x0800_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    pub r0: u32,
    pub pc: u32,
    pub cpsr: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepInfo {
    pub opcode: u32,
    pub cycles: u8,
}

#[derive(Debug, Clone)]
pub struct GbaBus {
    rom: Vec<u8>,
}

impl GbaBus {
    pub fn with_rom(rom: &[u8]) -> Self {
        Self { rom: rom.to_vec() }
    }

    pub fn read32_rom(&self, addr: u32) -> u32 {
        if self.rom.is_empty() {
            return 0;
        }

        let offset = addr.wrapping_sub(ROM_BASE) as usize;
        let b0 = self.rom.get(offset).copied().unwrap_or(0);
        let b1 = self.rom.get(offset + 1).copied().unwrap_or(0);
        let b2 = self.rom.get(offset + 2).copied().unwrap_or(0);
        let b3 = self.rom.get(offset + 3).copied().unwrap_or(0);
        u32::from_le_bytes([b0, b1, b2, b3])
    }
}

#[derive(Debug)]
pub struct GbaCore {
    bus: GbaBus,
    regs: Registers,
    input_mix: u32,
}

impl Default for GbaCore {
    fn default() -> Self {
        Self {
            bus: GbaBus::with_rom(&[]),
            regs: Registers {
                r0: 0,
                pc: ROM_BASE,
                cpsr: 0x6000_001F,
            },
            input_mix: 0,
        }
    }
}

impl GbaCore {
    pub fn load_rom(&mut self, rom: &[u8]) {
        self.bus = GbaBus::with_rom(rom);
    }

    pub fn regs(&self) -> Registers {
        self.regs
    }

    fn reset_state(&mut self) {
        self.regs = Registers {
            r0: 0,
            pc: ROM_BASE,
            cpsr: 0x6000_001F,
        };
        self.input_mix = 0;
    }

    fn step(&mut self) -> StepInfo {
        let opcode = self.bus.read32_rom(self.regs.pc);
        self.regs.pc = self.regs.pc.wrapping_add(4);

        // Tiny bootstrap opcode subset, enough to execute simple homebrew stubs deterministically.
        match opcode {
            0xE1A00000 => {} // MOV R0,R0 (NOP)
            0xE2800001 => {
                // ADD R0,R0,#1
                self.regs.r0 = self.regs.r0.wrapping_add(1);
                self.update_zn(self.regs.r0);
            }
            op if (op & 0xFFFF_FF00) == 0xE3A0_0000 => {
                // MOV R0,#imm8
                self.regs.r0 = op & 0xFF;
                self.update_zn(self.regs.r0);
            }
            _ => {
                // TODO: Replace fallback with strict ARM decoder and THUMB support.
            }
        }

        StepInfo { opcode, cycles: 1 }
    }

    fn update_zn(&mut self, value: u32) {
        if value == 0 {
            self.regs.cpsr |= 1 << 30;
        } else {
            self.regs.cpsr &= !(1 << 30);
        }

        if (value & 0x8000_0000) != 0 {
            self.regs.cpsr |= 1 << 31;
        } else {
            self.regs.cpsr &= !(1 << 31);
        }
    }
}

impl DeterministicMachine for GbaCore {
    fn system_id(&self) -> SystemId {
        SystemId::Gba
    }

    fn reset(&mut self) {
        self.reset_state();
    }

    fn tick(&mut self, cycle: u64, input_events: &[InputEvent]) -> (u8, i16) {
        for event in input_events {
            let mix =
                ((event.port as u32) << 16) | ((event.button as u32) << 8) | event.state as u32;
            self.input_mix = self.input_mix.rotate_left(3) ^ mix ^ cycle as u32;
        }

        let _step = self.step();

        let frame = (self.regs.r0 as u8) ^ (self.regs.pc as u8) ^ (self.input_mix as u8);
        let audio = ((self.regs.r0 as i32)
            ^ ((self.regs.pc as i32) >> 2)
            ^ ((self.regs.cpsr as i32) >> 8)
            ^ ((self.input_mix as i32) << 1)) as i16;
        (frame, audio)
    }
}

#[derive(Debug, Error)]
pub enum GbaRunError {
    #[error("ROM is empty")]
    EmptyRom,
    #[error("{0}")]
    Determinism(#[from] DeterminismError),
}

pub fn run_rom_digest(
    cycles: u64,
    replay: &ReplayLog,
    rom: &[u8],
) -> Result<RunDigest, GbaRunError> {
    if rom.is_empty() {
        return Err(GbaRunError::EmptyRom);
    }
    let mut core = GbaCore::default();
    core.load_rom(rom);
    Ok(run_deterministic(&mut core, cycles, replay)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aletheia_core::{InputButton, InputState};

    fn replay_fixture() -> ReplayLog {
        ReplayLog::from(vec![
            InputEvent {
                cycle: 2,
                port: 0,
                button: InputButton::A,
                state: InputState::Pressed,
            },
            InputEvent {
                cycle: 3,
                port: 0,
                button: InputButton::A,
                state: InputState::Released,
            },
        ])
    }

    #[test]
    fn run_rom_digest_is_reproducible() {
        let replay = replay_fixture();
        let mut rom = vec![0; 32];
        rom[0..4].copy_from_slice(&0xE3A0_0005u32.to_le_bytes()); // MOV R0,#5
        rom[4..8].copy_from_slice(&0xE280_0001u32.to_le_bytes()); // ADD R0,#1
        rom[8..12].copy_from_slice(&0xE1A0_0000u32.to_le_bytes()); // NOP

        let a = run_rom_digest(8, &replay, &rom).expect("run");
        let b = run_rom_digest(8, &replay, &rom).expect("run");
        assert_eq!(a, b);
        assert_eq!(a.system, SystemId::Gba);
    }

    #[test]
    fn core_executes_mov_and_add() {
        let mut core = GbaCore::default();
        let mut rom = vec![0; 16];
        rom[0..4].copy_from_slice(&0xE3A0_0002u32.to_le_bytes()); // MOV R0,#2
        rom[4..8].copy_from_slice(&0xE280_0001u32.to_le_bytes()); // ADD R0,#1
        core.load_rom(&rom);

        run_deterministic(&mut core, 2, &ReplayLog::new()).expect("run");
        let regs = core.regs();
        assert_eq!(regs.r0, 3);
        assert_eq!(regs.pc, ROM_BASE + 8);
    }
}
