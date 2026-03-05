use crate::bus::NesBus;
use thiserror::Error;

const FLAG_CARRY: u8 = 0x01;
const FLAG_ZERO: u8 = 0x02;
const FLAG_INTERRUPT_DISABLE: u8 = 0x04;
const FLAG_BREAK: u8 = 0x10;
const FLAG_UNUSED: u8 = 0x20;
const FLAG_OVERFLOW: u8 = 0x40;
const FLAG_NEGATIVE: u8 = 0x80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub p: u8,
    pub sp: u8,
    pub pc: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepInfo {
    pub opcode: u8,
    pub cycles: u8,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NesCpuError {
    #[error("unsupported opcode 0x{opcode:02X} at PC 0x{pc:04X}")]
    UnsupportedOpcode { opcode: u8, pc: u16 },
}

#[derive(Debug, Default)]
pub struct NesCpu {
    regs: Registers,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            p: FLAG_INTERRUPT_DISABLE | FLAG_UNUSED,
            sp: 0xFD,
            pc: 0,
        }
    }
}

impl NesCpu {
    pub fn reset(&mut self, bus: &NesBus) {
        self.regs = Registers::default();
        let lo = bus.read8(0xFFFC) as u16;
        let hi = bus.read8(0xFFFD) as u16;
        self.regs.pc = (hi << 8) | lo;
    }

    pub fn regs(&self) -> Registers {
        self.regs
    }

    pub fn step(&mut self, bus: &mut NesBus) -> Result<StepInfo, NesCpuError> {
        let pc_before = self.regs.pc;
        let opcode = self.fetch8(bus);

        let cycles = match opcode {
            0x00 => {
                // BRK
                let return_pc = self.regs.pc.wrapping_add(1);
                self.push16(bus, return_pc);
                let mut p = self.regs.p | FLAG_BREAK | FLAG_UNUSED;
                self.push8(bus, p);
                p |= FLAG_INTERRUPT_DISABLE;
                self.regs.p = p;
                let lo = bus.read8(0xFFFE) as u16;
                let hi = bus.read8(0xFFFF) as u16;
                self.regs.pc = (hi << 8) | lo;
                7
            }
            0xEA => 2, // NOP
            0xA9 => {
                self.regs.a = self.fetch8(bus);
                self.update_zn(self.regs.a);
                2
            }
            0xA2 => {
                self.regs.x = self.fetch8(bus);
                self.update_zn(self.regs.x);
                2
            }
            0xA0 => {
                self.regs.y = self.fetch8(bus);
                self.update_zn(self.regs.y);
                2
            }
            0xA5 => {
                let addr = self.fetch8(bus) as u16;
                self.regs.a = bus.read8(addr);
                self.update_zn(self.regs.a);
                3
            }
            0xAD => {
                let addr = self.fetch16(bus);
                self.regs.a = bus.read8(addr);
                self.update_zn(self.regs.a);
                4
            }
            0x85 => {
                let addr = self.fetch8(bus) as u16;
                bus.write8(addr, self.regs.a);
                3
            }
            0x86 => {
                let addr = self.fetch8(bus) as u16;
                bus.write8(addr, self.regs.x);
                3
            }
            0x84 => {
                let addr = self.fetch8(bus) as u16;
                bus.write8(addr, self.regs.y);
                3
            }
            0x8D => {
                let addr = self.fetch16(bus);
                bus.write8(addr, self.regs.a);
                4
            }
            0xAA => {
                self.regs.x = self.regs.a;
                self.update_zn(self.regs.x);
                2
            }
            0x8A => {
                self.regs.a = self.regs.x;
                self.update_zn(self.regs.a);
                2
            }
            0xA8 => {
                self.regs.y = self.regs.a;
                self.update_zn(self.regs.y);
                2
            }
            0x98 => {
                self.regs.a = self.regs.y;
                self.update_zn(self.regs.a);
                2
            }
            0xE8 => {
                self.regs.x = self.regs.x.wrapping_add(1);
                self.update_zn(self.regs.x);
                2
            }
            0xCA => {
                self.regs.x = self.regs.x.wrapping_sub(1);
                self.update_zn(self.regs.x);
                2
            }
            0xC8 => {
                self.regs.y = self.regs.y.wrapping_add(1);
                self.update_zn(self.regs.y);
                2
            }
            0x88 => {
                self.regs.y = self.regs.y.wrapping_sub(1);
                self.update_zn(self.regs.y);
                2
            }
            0x4C => {
                self.regs.pc = self.fetch16(bus);
                3
            }
            0x20 => {
                let target = self.fetch16(bus);
                let ret = self.regs.pc.wrapping_sub(1);
                self.push16(bus, ret);
                self.regs.pc = target;
                6
            }
            0x60 => {
                self.regs.pc = self.pop16(bus).wrapping_add(1);
                6
            }
            0xD0 => self.branch_relative(bus, !self.flag(FLAG_ZERO)),
            0xF0 => self.branch_relative(bus, self.flag(FLAG_ZERO)),
            0x18 => {
                self.set_flag(FLAG_CARRY, false);
                2
            }
            0x38 => {
                self.set_flag(FLAG_CARRY, true);
                2
            }
            0x69 => {
                let value = self.fetch8(bus);
                self.adc(value);
                2
            }
            0xE9 => {
                let value = self.fetch8(bus);
                self.sbc(value);
                2
            }
            0x29 => {
                self.regs.a &= self.fetch8(bus);
                self.update_zn(self.regs.a);
                2
            }
            0x09 => {
                self.regs.a |= self.fetch8(bus);
                self.update_zn(self.regs.a);
                2
            }
            0x49 => {
                self.regs.a ^= self.fetch8(bus);
                self.update_zn(self.regs.a);
                2
            }
            0xC9 => {
                let value = self.fetch8(bus);
                self.compare(self.regs.a, value);
                2
            }
            0xE0 => {
                let value = self.fetch8(bus);
                self.compare(self.regs.x, value);
                2
            }
            0xC0 => {
                let value = self.fetch8(bus);
                self.compare(self.regs.y, value);
                2
            }
            _ => {
                return Err(NesCpuError::UnsupportedOpcode {
                    opcode,
                    pc: pc_before,
                });
            }
        };

        Ok(StepInfo { opcode, cycles })
    }

    fn fetch8(&mut self, bus: &NesBus) -> u8 {
        let value = bus.read8(self.regs.pc);
        self.regs.pc = self.regs.pc.wrapping_add(1);
        value
    }

    fn fetch16(&mut self, bus: &NesBus) -> u16 {
        let lo = self.fetch8(bus) as u16;
        let hi = self.fetch8(bus) as u16;
        (hi << 8) | lo
    }

    fn push8(&mut self, bus: &mut NesBus, value: u8) {
        let addr = 0x0100 | self.regs.sp as u16;
        bus.write8(addr, value);
        self.regs.sp = self.regs.sp.wrapping_sub(1);
    }

    fn push16(&mut self, bus: &mut NesBus, value: u16) {
        self.push8(bus, (value >> 8) as u8);
        self.push8(bus, (value & 0x00FF) as u8);
    }

    fn pop8(&mut self, bus: &NesBus) -> u8 {
        self.regs.sp = self.regs.sp.wrapping_add(1);
        let addr = 0x0100 | self.regs.sp as u16;
        bus.read8(addr)
    }

    fn pop16(&mut self, bus: &NesBus) -> u16 {
        let lo = self.pop8(bus) as u16;
        let hi = self.pop8(bus) as u16;
        (hi << 8) | lo
    }

    fn branch_relative(&mut self, bus: &NesBus, taken: bool) -> u8 {
        let offset = self.fetch8(bus) as i8 as i16;
        if taken {
            self.regs.pc = (self.regs.pc as i16).wrapping_add(offset) as u16;
            3
        } else {
            2
        }
    }

    fn adc(&mut self, value: u8) {
        let carry_in = u16::from(self.flag(FLAG_CARRY));
        let lhs = self.regs.a as u16;
        let rhs = value as u16;
        let result16 = lhs + rhs + carry_in;
        let result = result16 as u8;

        self.set_flag(FLAG_CARRY, result16 > 0xFF);
        self.set_flag(FLAG_ZERO, result == 0);
        self.set_flag(FLAG_NEGATIVE, (result & 0x80) != 0);
        let overflow = (!(self.regs.a ^ value) & (self.regs.a ^ result) & 0x80) != 0;
        self.set_flag(FLAG_OVERFLOW, overflow);

        self.regs.a = result;
    }

    fn sbc(&mut self, value: u8) {
        self.adc(!value);
    }

    fn compare(&mut self, lhs: u8, rhs: u8) {
        let result = lhs.wrapping_sub(rhs);
        self.set_flag(FLAG_CARRY, lhs >= rhs);
        self.set_flag(FLAG_ZERO, lhs == rhs);
        self.set_flag(FLAG_NEGATIVE, (result & 0x80) != 0);
    }

    fn update_zn(&mut self, value: u8) {
        self.set_flag(FLAG_ZERO, value == 0);
        self.set_flag(FLAG_NEGATIVE, (value & 0x80) != 0);
    }

    fn flag(&self, flag: u8) -> bool {
        (self.regs.p & flag) != 0
    }

    fn set_flag(&mut self, flag: u8, enabled: bool) {
        if enabled {
            self.regs.p |= flag;
        } else {
            self.regs.p &= !flag;
        }
        self.regs.p |= FLAG_UNUSED;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{NesBus, PROGRAM_START};

    fn cpu_with_program(program: &[u8]) -> (NesCpu, NesBus) {
        let mut cpu = NesCpu::default();
        let mut bus = NesBus::default();
        bus.set_reset_vector(PROGRAM_START);
        bus.load_program(PROGRAM_START, program);
        cpu.reset(&bus);
        (cpu, bus)
    }

    #[test]
    fn reset_loads_pc_from_vector() {
        let mut cpu = NesCpu::default();
        let mut bus = NesBus::default();
        bus.set_reset_vector(0x8123);
        cpu.reset(&bus);
        assert_eq!(cpu.regs().pc, 0x8123);
    }

    #[test]
    fn lda_immediate_sets_zero_and_negative_flags() {
        let (mut cpu, mut bus) = cpu_with_program(&[0xA9, 0x00, 0xA9, 0x80]);
        cpu.step(&mut bus).expect("step");
        assert_eq!(cpu.regs().a, 0x00);
        assert_eq!(cpu.regs().p & FLAG_ZERO, FLAG_ZERO);
        assert_eq!(cpu.regs().p & FLAG_NEGATIVE, 0);

        cpu.step(&mut bus).expect("step");
        assert_eq!(cpu.regs().a, 0x80);
        assert_eq!(cpu.regs().p & FLAG_ZERO, 0);
        assert_eq!(cpu.regs().p & FLAG_NEGATIVE, FLAG_NEGATIVE);
    }

    #[test]
    fn jsr_and_rts_return_to_call_site() {
        let (mut cpu, mut bus) = cpu_with_program(&[
            0x20, 0x06, 0x80, // JSR 8006
            0xA9, 0x44, // LDA #44
            0xEA, // NOP
            0xA9, 0x22, // LDA #22
            0x60, // RTS
        ]);

        cpu.step(&mut bus).expect("jsr");
        assert_eq!(cpu.regs().pc, 0x8006);
        cpu.step(&mut bus).expect("lda");
        cpu.step(&mut bus).expect("rts");
        cpu.step(&mut bus).expect("lda caller");
        assert_eq!(cpu.regs().a, 0x44);
    }

    #[test]
    fn adc_and_sbc_update_carry_and_overflow() {
        let (mut cpu, mut bus) = cpu_with_program(&[0xA9, 0x7F, 0x69, 0x01, 0x38, 0xE9, 0x01]);
        cpu.step(&mut bus).expect("lda");
        cpu.step(&mut bus).expect("adc");
        assert_eq!(cpu.regs().a, 0x80);
        assert_eq!(cpu.regs().p & FLAG_OVERFLOW, FLAG_OVERFLOW);

        cpu.step(&mut bus).expect("sec");
        cpu.step(&mut bus).expect("sbc");
        assert_eq!(cpu.regs().a, 0x7F);
    }

    #[test]
    fn bne_branch_is_taken_when_zero_clear() {
        let (mut cpu, mut bus) =
            cpu_with_program(&[0xA9, 0x01, 0xD0, 0x02, 0xA9, 0x11, 0xA9, 0x22]);
        cpu.step(&mut bus).expect("lda");
        cpu.step(&mut bus).expect("bne");
        cpu.step(&mut bus).expect("lda 22");
        assert_eq!(cpu.regs().a, 0x22);
    }

    #[test]
    fn unsupported_opcode_returns_error() {
        let (mut cpu, mut bus) = cpu_with_program(&[0x02]);
        let err = cpu.step(&mut bus).expect_err("opcode should fail");
        assert_eq!(
            err,
            NesCpuError::UnsupportedOpcode {
                opcode: 0x02,
                pc: 0x8000
            }
        );
    }

    #[test]
    fn status_preserves_decimal_and_interrupt_flags() {
        const FLAG_DECIMAL: u8 = 0x08;
        let (mut cpu, mut bus) = cpu_with_program(&[0xA9, 0x00, 0xA9, 0x7F]);
        cpu.regs.p = FLAG_DECIMAL | FLAG_INTERRUPT_DISABLE | FLAG_UNUSED;

        cpu.step(&mut bus).expect("step");
        assert_eq!(cpu.regs().p & FLAG_DECIMAL, FLAG_DECIMAL);
        assert_eq!(
            cpu.regs().p & FLAG_INTERRUPT_DISABLE,
            FLAG_INTERRUPT_DISABLE
        );

        cpu.step(&mut bus).expect("step");
        assert_eq!(cpu.regs().p & FLAG_DECIMAL, FLAG_DECIMAL);
    }
}
