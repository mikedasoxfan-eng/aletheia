use crate::bus::GbBus;

const FLAG_Z: u8 = 0x80;
const FLAG_N: u8 = 0x40;
const FLAG_H: u8 = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    pub a: u8,
    pub f: u8,
    pub pc: u16,
    pub sp: u16,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            a: 0x01,
            f: 0xB0,
            pc: 0x0100,
            sp: 0xFFFE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepInfo {
    pub opcode: u8,
    pub cycles: u8,
}

#[derive(Debug, Default)]
pub struct GbCpu {
    regs: Registers,
}

impl GbCpu {
    pub fn reset(&mut self) {
        self.regs = Registers::default();
    }

    pub fn regs(&self) -> Registers {
        self.regs
    }

    pub fn step(&mut self, bus: &mut GbBus) -> StepInfo {
        let opcode = self.fetch8(bus);
        let cycles = match opcode {
            0x00 => 4, // NOP
            0x3E => {
                // LD A,d8
                let value = self.fetch8(bus);
                self.regs.a = value;
                8
            }
            0x3C => {
                // INC A
                let a = self.regs.a;
                let result = a.wrapping_add(1);
                self.regs.a = result;

                self.set_flag(FLAG_Z, result == 0);
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, (a & 0x0F) + 1 > 0x0F);
                4
            }
            0x3D => {
                // DEC A
                let a = self.regs.a;
                let result = a.wrapping_sub(1);
                self.regs.a = result;

                self.set_flag(FLAG_Z, result == 0);
                self.set_flag(FLAG_N, true);
                self.set_flag(FLAG_H, (a & 0x0F) == 0);
                4
            }
            0xAF => {
                // XOR A
                self.regs.a ^= self.regs.a;
                self.regs.f = 0;
                self.set_flag(FLAG_Z, self.regs.a == 0);
                4
            }
            _ => {
                // TODO: Replace fallback with strict decode table + error reporting.
                4
            }
        };

        StepInfo { opcode, cycles }
    }

    fn fetch8(&mut self, bus: &GbBus) -> u8 {
        let value = bus.read8(self.regs.pc);
        self.regs.pc = self.regs.pc.wrapping_add(1);
        value
    }

    fn set_flag(&mut self, flag: u8, enabled: bool) {
        if enabled {
            self.regs.f |= flag;
        } else {
            self.regs.f &= !flag;
        }
        self.regs.f &= 0xF0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{GbBus, PROGRAM_START};

    fn cpu_with_program(program: &[u8]) -> (GbCpu, GbBus) {
        let mut cpu = GbCpu::default();
        cpu.reset();
        let mut bus = GbBus::default();
        bus.load_program(PROGRAM_START, program);
        (cpu, bus)
    }

    #[test]
    fn ld_a_imm_loads_immediate_and_uses_8_cycles() {
        let (mut cpu, mut bus) = cpu_with_program(&[0x3E, 0x42]);
        let step = cpu.step(&mut bus);

        assert_eq!(
            step,
            StepInfo {
                opcode: 0x3E,
                cycles: 8
            }
        );
        assert_eq!(cpu.regs().a, 0x42);
        assert_eq!(cpu.regs().pc, 0x0102);
    }

    #[test]
    fn inc_a_updates_zero_and_halfcarry_flags() {
        let (mut cpu, mut bus) = cpu_with_program(&[0x3E, 0x0F, 0x3C, 0x3C]);
        cpu.step(&mut bus); // LD A,0x0F
        cpu.step(&mut bus); // INC A -> 0x10, H set
        let regs_after_first_inc = cpu.regs();
        cpu.step(&mut bus); // INC A -> 0x11, H clear
        let regs_after_second_inc = cpu.regs();

        assert_eq!(regs_after_first_inc.a, 0x10);
        assert_eq!(regs_after_first_inc.f & FLAG_H, FLAG_H);
        assert_eq!(regs_after_first_inc.f & FLAG_N, 0);
        assert_eq!(regs_after_second_inc.a, 0x11);
        assert_eq!(regs_after_second_inc.f & FLAG_H, 0);
    }

    #[test]
    fn dec_a_sets_subtract_and_halfcarry_flags() {
        let (mut cpu, mut bus) = cpu_with_program(&[0x3E, 0x10, 0x3D]);
        cpu.step(&mut bus); // LD A,0x10
        cpu.step(&mut bus); // DEC A -> 0x0F
        let regs = cpu.regs();

        assert_eq!(regs.a, 0x0F);
        assert_eq!(regs.f & FLAG_N, FLAG_N);
        assert_eq!(regs.f & FLAG_H, FLAG_H);
    }

    #[test]
    fn xor_a_zeroes_accumulator_and_sets_zero_flag() {
        let (mut cpu, mut bus) = cpu_with_program(&[0x3E, 0x77, 0xAF]);
        cpu.step(&mut bus); // LD A,0x77
        cpu.step(&mut bus); // XOR A
        let regs = cpu.regs();

        assert_eq!(regs.a, 0x00);
        assert_eq!(regs.f, FLAG_Z);
    }

    #[test]
    fn carry_flag_is_preserved_by_inc_and_dec() {
        const FLAG_C: u8 = 0x10;
        let (mut cpu, mut bus) = cpu_with_program(&[0x3C, 0x3D]);
        cpu.regs.f = FLAG_C;

        cpu.step(&mut bus);
        assert_eq!(cpu.regs().f & FLAG_C, FLAG_C);

        cpu.step(&mut bus);
        assert_eq!(cpu.regs().f & FLAG_C, FLAG_C);
    }
}
