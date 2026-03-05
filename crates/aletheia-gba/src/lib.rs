use aletheia_core::{
    DeterminismError, DeterministicMachine, InputEvent, ReplayLog, RunDigest, SystemId,
    run_deterministic,
};
use thiserror::Error;

const ROM_BASE: u32 = 0x0800_0000;
const WRAM_BASE: u32 = 0x0200_0000;
const WRAM_SIZE: usize = 0x40000;
const CPSR_T_BIT: u32 = 1 << 5;
const CPSR_N_BIT: u32 = 1 << 31;
const CPSR_Z_BIT: u32 = 1 << 30;
const CPSR_C_BIT: u32 = 1 << 29;
const CPSR_V_BIT: u32 = 1 << 28;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    pub r0: u32,
    pub r1: u32,
    pub r2: u32,
    pub r3: u32,
    pub pc: u32,
    pub cpsr: u32,
    pub thumb: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepInfo {
    pub opcode: u32,
    pub cycles: u8,
    pub thumb: bool,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GbaCpuError {
    #[error("unsupported ARM opcode 0x{opcode:08X} at PC 0x{pc:08X}")]
    UnsupportedArm { opcode: u32, pc: u32 },
    #[error("unsupported THUMB opcode 0x{opcode:04X} at PC 0x{pc:08X}")]
    UnsupportedThumb { opcode: u16, pc: u32 },
}

#[derive(Debug, Clone)]
pub struct GbaBus {
    rom: Vec<u8>,
    wram: Vec<u8>,
}

impl GbaBus {
    pub fn with_rom(rom: &[u8]) -> Self {
        Self {
            rom: rom.to_vec(),
            wram: vec![0; WRAM_SIZE],
        }
    }

    fn read8(&self, addr: u32) -> u8 {
        match addr {
            ROM_BASE..=0x09FF_FFFF => {
                let offset = addr.wrapping_sub(ROM_BASE) as usize;
                self.rom.get(offset).copied().unwrap_or(0)
            }
            WRAM_BASE..=0x0203_FFFF => {
                let offset = addr.wrapping_sub(WRAM_BASE) as usize;
                self.wram.get(offset).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write8(&mut self, addr: u32, value: u8) {
        if let WRAM_BASE..=0x0203_FFFF = addr {
            let offset = addr.wrapping_sub(WRAM_BASE) as usize;
            if let Some(slot) = self.wram.get_mut(offset) {
                *slot = value;
            }
        }
    }

    pub fn read16(&self, addr: u32) -> u16 {
        let b0 = self.read8(addr);
        let b1 = self.read8(addr.wrapping_add(1));
        u16::from_le_bytes([b0, b1])
    }

    pub fn read32(&self, addr: u32) -> u32 {
        let b0 = self.read8(addr);
        let b1 = self.read8(addr.wrapping_add(1));
        let b2 = self.read8(addr.wrapping_add(2));
        let b3 = self.read8(addr.wrapping_add(3));
        u32::from_le_bytes([b0, b1, b2, b3])
    }

    pub fn write32(&mut self, addr: u32, value: u32) {
        let bytes = value.to_le_bytes();
        self.write8(addr, bytes[0]);
        self.write8(addr.wrapping_add(1), bytes[1]);
        self.write8(addr.wrapping_add(2), bytes[2]);
        self.write8(addr.wrapping_add(3), bytes[3]);
    }
}

#[derive(Debug)]
pub struct GbaCore {
    bus: GbaBus,
    gpr: [u32; 16],
    cpsr: u32,
    boot_thumb: bool,
    input_mix: u32,
    fault: Option<GbaCpuError>,
}

impl Default for GbaCore {
    fn default() -> Self {
        Self {
            bus: GbaBus::with_rom(&[]),
            gpr: [0; 16],
            cpsr: 0x6000_001F,
            boot_thumb: false,
            input_mix: 0,
            fault: None,
        }
    }
}

impl GbaCore {
    pub fn load_rom(&mut self, rom: &[u8]) {
        self.bus = GbaBus::with_rom(rom);
    }

    pub fn regs(&self) -> Registers {
        Registers {
            r0: self.gpr[0],
            r1: self.gpr[1],
            r2: self.gpr[2],
            r3: self.gpr[3],
            pc: self.gpr[15],
            cpsr: self.cpsr,
            thumb: self.thumb_mode(),
        }
    }

    pub fn fault(&self) -> Option<&GbaCpuError> {
        self.fault.as_ref()
    }

    pub fn set_boot_thumb(&mut self, enabled: bool) {
        self.boot_thumb = enabled;
    }

    fn reset_state(&mut self) {
        self.gpr = [0; 16];
        self.gpr[15] = ROM_BASE;
        self.cpsr = 0x6000_001F;
        self.set_thumb_mode(self.boot_thumb);
        self.input_mix = 0;
        self.fault = None;
    }

    fn thumb_mode(&self) -> bool {
        (self.cpsr & CPSR_T_BIT) != 0
    }

    fn set_thumb_mode(&mut self, enabled: bool) {
        if enabled {
            self.cpsr |= CPSR_T_BIT;
        } else {
            self.cpsr &= !CPSR_T_BIT;
        }
    }

    fn set_nz(&mut self, value: u32) {
        if value == 0 {
            self.cpsr |= CPSR_Z_BIT;
        } else {
            self.cpsr &= !CPSR_Z_BIT;
        }

        if (value & 0x8000_0000) != 0 {
            self.cpsr |= CPSR_N_BIT;
        } else {
            self.cpsr &= !CPSR_N_BIT;
        }
    }

    fn step(&mut self) -> Result<StepInfo, GbaCpuError> {
        if self.thumb_mode() {
            self.step_thumb()
        } else {
            self.step_arm()
        }
    }

    fn step_arm(&mut self) -> Result<StepInfo, GbaCpuError> {
        let pc = self.gpr[15];
        let opcode = self.bus.read32(pc);
        self.gpr[15] = self.gpr[15].wrapping_add(4);

        let cond = (opcode >> 28) & 0xF;
        if cond != 0xE {
            // For now, only AL executes. Other conditions are treated as NOP to keep deterministic stepping.
            return Ok(StepInfo {
                opcode,
                cycles: 1,
                thumb: false,
            });
        }

        if (opcode & 0x0FFF_FFF0) == 0x012F_FF10 {
            // BX Rm
            let rm = (opcode & 0x0F) as usize;
            let target = self.gpr[rm];
            let thumb = (target & 1) != 0;
            self.set_thumb_mode(thumb);
            self.gpr[15] = if thumb { target & !1 } else { target & !3 };
            return Ok(StepInfo {
                opcode,
                cycles: 3,
                thumb: false,
            });
        }

        if (opcode & 0x0E00_0000) == 0x0A00_0000 {
            // B/BL
            let imm24 = opcode & 0x00FF_FFFF;
            let signed = ((imm24 << 8) as i32) >> 6;
            self.gpr[15] = (self.gpr[15] as i32).wrapping_add(signed) as u32;
            return Ok(StepInfo {
                opcode,
                cycles: 3,
                thumb: false,
            });
        }

        if (opcode & 0x0C00_0000) == 0x0000_0000 {
            return self.step_arm_data_processing(pc, opcode);
        }

        if (opcode & 0x0C00_0000) == 0x0400_0000 {
            return self.step_arm_load_store(pc, opcode);
        }

        Err(GbaCpuError::UnsupportedArm { opcode, pc })
    }

    fn step_arm_data_processing(&mut self, pc: u32, opcode: u32) -> Result<StepInfo, GbaCpuError> {
        let immediate = (opcode & (1 << 25)) != 0;
        let op = ((opcode >> 21) & 0xF) as u8;
        let set_flags = (opcode & (1 << 20)) != 0;
        let rn = ((opcode >> 16) & 0xF) as usize;
        let rd = ((opcode >> 12) & 0xF) as usize;

        let operand2 = if immediate {
            let imm8 = opcode & 0xFF;
            let rotate = ((opcode >> 8) & 0xF) * 2;
            imm8.rotate_right(rotate)
        } else {
            if (opcode & 0x0000_0FF0) != 0 {
                return Err(GbaCpuError::UnsupportedArm { opcode, pc });
            }
            let rm = (opcode & 0xF) as usize;
            self.gpr[rm]
        };

        match op {
            0x0 => {
                self.gpr[rd] = self.gpr[rn] & operand2;
                if set_flags {
                    self.set_nz(self.gpr[rd]);
                }
            }
            0x1 => {
                self.gpr[rd] = self.gpr[rn] ^ operand2;
                if set_flags {
                    self.set_nz(self.gpr[rd]);
                }
            }
            0x2 => {
                let (result, borrow) = self.gpr[rn].overflowing_sub(operand2);
                self.gpr[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    if !borrow {
                        self.cpsr |= CPSR_C_BIT;
                    } else {
                        self.cpsr &= !CPSR_C_BIT;
                    }
                    let overflow = ((self.gpr[rn] ^ operand2) & (self.gpr[rn] ^ result) & 0x8000_0000) != 0;
                    if overflow {
                        self.cpsr |= CPSR_V_BIT;
                    } else {
                        self.cpsr &= !CPSR_V_BIT;
                    }
                }
            }
            0x4 => {
                let (result, carry) = self.gpr[rn].overflowing_add(operand2);
                self.gpr[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    if carry {
                        self.cpsr |= CPSR_C_BIT;
                    } else {
                        self.cpsr &= !CPSR_C_BIT;
                    }
                    let overflow = ((!(self.gpr[rn] ^ operand2)) & (self.gpr[rn] ^ result) & 0x8000_0000) != 0;
                    if overflow {
                        self.cpsr |= CPSR_V_BIT;
                    } else {
                        self.cpsr &= !CPSR_V_BIT;
                    }
                }
            }
            0xA => {
                let (result, borrow) = self.gpr[rn].overflowing_sub(operand2);
                self.set_nz(result);
                if !borrow {
                    self.cpsr |= CPSR_C_BIT;
                } else {
                    self.cpsr &= !CPSR_C_BIT;
                }
            }
            0xC => {
                self.gpr[rd] = self.gpr[rn] | operand2;
                if set_flags {
                    self.set_nz(self.gpr[rd]);
                }
            }
            0xD => {
                self.gpr[rd] = operand2;
                if set_flags || immediate {
                    self.set_nz(self.gpr[rd]);
                }
            }
            _ => {
                return Err(GbaCpuError::UnsupportedArm { opcode, pc });
            }
        }

        Ok(StepInfo {
            opcode,
            cycles: 1,
            thumb: false,
        })
    }

    fn step_arm_load_store(&mut self, pc: u32, opcode: u32) -> Result<StepInfo, GbaCpuError> {
        let pre_index = (opcode & (1 << 24)) != 0;
        let add = (opcode & (1 << 23)) != 0;
        let byte = (opcode & (1 << 22)) != 0;
        let writeback = (opcode & (1 << 21)) != 0;
        let load = (opcode & (1 << 20)) != 0;
        let rn = ((opcode >> 16) & 0xF) as usize;
        let rd = ((opcode >> 12) & 0xF) as usize;

        if byte || !pre_index || writeback {
            return Err(GbaCpuError::UnsupportedArm { opcode, pc });
        }

        let offset = opcode & 0xFFF;
        let base = self.gpr[rn];
        let addr = if add {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };

        if load {
            self.gpr[rd] = self.bus.read32(addr);
            if rd == 15 {
                self.gpr[15] &= !1;
                self.set_thumb_mode(false);
            }
        } else {
            self.bus.write32(addr, self.gpr[rd]);
        }

        Ok(StepInfo {
            opcode,
            cycles: 2,
            thumb: false,
        })
    }

    fn step_thumb(&mut self) -> Result<StepInfo, GbaCpuError> {
        let pc = self.gpr[15];
        let opcode = self.bus.read16(pc);
        self.gpr[15] = self.gpr[15].wrapping_add(2);

        if (opcode & 0xF800) == 0x2000 {
            // MOV Rd,#imm8
            let rd = ((opcode >> 8) & 0x7) as usize;
            let imm = (opcode & 0xFF) as u32;
            self.gpr[rd] = imm;
            self.set_nz(self.gpr[rd]);
            return Ok(StepInfo {
                opcode: opcode as u32,
                cycles: 1,
                thumb: true,
            });
        }

        if (opcode & 0xF800) == 0x2800 {
            // CMP Rd,#imm8
            let rd = ((opcode >> 8) & 0x7) as usize;
            let imm = (opcode & 0xFF) as u32;
            let (result, borrow) = self.gpr[rd].overflowing_sub(imm);
            self.set_nz(result);
            if !borrow {
                self.cpsr |= CPSR_C_BIT;
            } else {
                self.cpsr &= !CPSR_C_BIT;
            }
            return Ok(StepInfo {
                opcode: opcode as u32,
                cycles: 1,
                thumb: true,
            });
        }

        if (opcode & 0xF800) == 0x3000 {
            // ADD Rd,#imm8
            let rd = ((opcode >> 8) & 0x7) as usize;
            let imm = (opcode & 0xFF) as u32;
            self.gpr[rd] = self.gpr[rd].wrapping_add(imm);
            self.set_nz(self.gpr[rd]);
            return Ok(StepInfo {
                opcode: opcode as u32,
                cycles: 1,
                thumb: true,
            });
        }

        if (opcode & 0xF800) == 0x3800 {
            // SUB Rd,#imm8
            let rd = ((opcode >> 8) & 0x7) as usize;
            let imm = (opcode & 0xFF) as u32;
            self.gpr[rd] = self.gpr[rd].wrapping_sub(imm);
            self.set_nz(self.gpr[rd]);
            return Ok(StepInfo {
                opcode: opcode as u32,
                cycles: 1,
                thumb: true,
            });
        }

        if (opcode & 0xF800) == 0xE000 {
            // B (unconditional)
            let imm11 = (opcode & 0x07FF) as i16;
            let signed = ((imm11 << 5) >> 4) as i32;
            self.gpr[15] = (self.gpr[15] as i32).wrapping_add(signed) as u32;
            return Ok(StepInfo {
                opcode: opcode as u32,
                cycles: 3,
                thumb: true,
            });
        }

        if (opcode & 0xFF87) == 0x4700 {
            // BX Rm
            let rm = ((opcode >> 3) & 0x0F) as usize;
            let target = self.gpr[rm];
            let thumb = (target & 1) != 0;
            self.set_thumb_mode(thumb);
            self.gpr[15] = if thumb { target & !1 } else { target & !3 };
            return Ok(StepInfo {
                opcode: opcode as u32,
                cycles: 3,
                thumb: true,
            });
        }

        if opcode == 0x46C0 {
            // NOP (MOV r8,r8)
            return Ok(StepInfo {
                opcode: opcode as u32,
                cycles: 1,
                thumb: true,
            });
        }

        Err(GbaCpuError::UnsupportedThumb { opcode, pc })
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

        if self.fault.is_none() {
            if let Err(error) = self.step() {
                self.fault = Some(error);
            }
        }

        let frame = (self.gpr[0] as u8) ^ (self.gpr[15] as u8) ^ (self.input_mix as u8);
        let audio = ((self.gpr[0] as i32)
            ^ ((self.gpr[1] as i32) << 1)
            ^ ((self.gpr[15] as i32) >> 2)
            ^ ((self.cpsr as i32) >> 8)
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
    #[error("{0}")]
    Cpu(#[from] GbaCpuError),
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
    let digest = run_deterministic(&mut core, cycles, replay)?;
    if let Some(fault) = core.fault() {
        return Err(GbaRunError::Cpu(fault.clone()));
    }
    Ok(digest)
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
        rom[0..4].copy_from_slice(&0xE3B0_0005u32.to_le_bytes()); // MOVS R0,#5
        rom[4..8].copy_from_slice(&0xE290_1001u32.to_le_bytes()); // ADDS R1,R0,#1
        rom[8..12].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes()); // B .

        let a = run_rom_digest(8, &replay, &rom).expect("run");
        let b = run_rom_digest(8, &replay, &rom).expect("run");
        assert_eq!(a, b);
        assert_eq!(a.system, SystemId::Gba);
    }

    #[test]
    fn arm_data_processing_executes_and_updates_registers() {
        let mut core = GbaCore::default();
        let mut rom = vec![0; 32];
        rom[0..4].copy_from_slice(&0xE3A0_0002u32.to_le_bytes()); // MOV R0,#2
        rom[4..8].copy_from_slice(&0xE280_1003u32.to_le_bytes()); // ADD R1,R0,#3
        rom[8..12].copy_from_slice(&0xE241_2001u32.to_le_bytes()); // SUB R2,R1,#1
        rom[12..16].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes()); // B .
        core.load_rom(&rom);

        run_deterministic(&mut core, 6, &ReplayLog::new()).expect("run");
        let regs = core.regs();
        assert_eq!(regs.r0, 2);
        assert_eq!(regs.r1, 5);
        assert_eq!(regs.r2, 4);
    }

    #[test]
    fn thumb_immediate_ops_execute_when_thumb_mode_set() {
        let mut core = GbaCore::default();
        let mut rom = vec![0; 32];
        // THUMB stream at ROM base
        rom[0..2].copy_from_slice(&0x2001u16.to_le_bytes()); // MOV R0,#1
        rom[2..4].copy_from_slice(&0x3002u16.to_le_bytes()); // ADD R0,#2
        rom[4..6].copy_from_slice(&0x3801u16.to_le_bytes()); // SUB R0,#1
        rom[6..8].copy_from_slice(&0xE7FFu16.to_le_bytes()); // B .
        core.load_rom(&rom);
        core.set_boot_thumb(true);

        run_deterministic(&mut core, 6, &ReplayLog::new()).expect("run");
        let regs = core.regs();
        assert!(regs.thumb);
        assert_eq!(regs.r0, 2);
    }

    #[test]
    fn unsupported_opcode_fails_rom_run() {
        let replay = replay_fixture();
        let mut rom = vec![0; 16];
        rom[0..4].copy_from_slice(&0xE120_0070u32.to_le_bytes()); // BKPT-like unsupported

        let error = run_rom_digest(2, &replay, &rom).expect_err("should fail");
        assert!(matches!(error, GbaRunError::Cpu(_)));
    }
}
