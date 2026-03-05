use crate::bus::GbBus;

const FLAG_Z: u8 = 0x80;
const FLAG_N: u8 = 0x40;
const FLAG_H: u8 = 0x20;
const FLAG_C: u8 = 0x10;

const IF_ADDR: u16 = 0xFF0F;
const IE_ADDR: u16 = 0xFFFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub pc: u16,
    pub sp: u16,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            a: 0x01,
            f: 0xB0,
            b: 0x00,
            c: 0x13,
            d: 0x00,
            e: 0xD8,
            h: 0x01,
            l: 0x4D,
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

#[derive(Debug, Clone, Default)]
pub struct GbCpu {
    regs: Registers,
    ime: bool,
    halted: bool,
}

impl GbCpu {
    pub fn reset(&mut self) {
        self.regs = Registers::default();
        self.ime = false;
        self.halted = false;
    }

    pub fn regs(&self) -> Registers {
        self.regs
    }

    pub fn service_interrupt(&mut self, bus: &mut GbBus) -> Option<u8> {
        let iflags = bus.read8(IF_ADDR);
        let ie = bus.read8(IE_ADDR);
        let pending = iflags & ie & 0x1F;

        if pending == 0 {
            return None;
        }

        if self.halted {
            self.halted = false;
        }

        if !self.ime {
            return None;
        }

        let bit = pending.trailing_zeros() as u8;
        let vector = match bit {
            0 => 0x40,
            1 => 0x48,
            2 => 0x50,
            3 => 0x58,
            4 => 0x60,
            _ => return None,
        };

        let cleared = iflags & !(1 << bit);
        bus.write8(IF_ADDR, cleared);

        self.ime = false;
        self.push16(bus, self.regs.pc);
        self.regs.pc = vector;
        Some(20)
    }

    pub fn step(&mut self, bus: &mut GbBus) -> StepInfo {
        if self.halted {
            return StepInfo {
                opcode: 0x76,
                cycles: 4,
            };
        }

        let opcode = self.fetch8(bus);
        let cycles = match opcode {
            0x00 => 4, // NOP
            0x06 => {
                self.regs.b = self.fetch8(bus);
                8
            }
            0x0E => {
                self.regs.c = self.fetch8(bus);
                8
            }
            0x16 => {
                self.regs.d = self.fetch8(bus);
                8
            }
            0x1E => {
                self.regs.e = self.fetch8(bus);
                8
            }
            0x26 => {
                self.regs.h = self.fetch8(bus);
                8
            }
            0x2E => {
                self.regs.l = self.fetch8(bus);
                8
            }
            0x3E => {
                self.regs.a = self.fetch8(bus);
                8
            }
            0x78 => {
                self.regs.a = self.regs.b;
                4
            }
            0x79 => {
                self.regs.a = self.regs.c;
                4
            }
            0x7A => {
                self.regs.a = self.regs.d;
                4
            }
            0x7B => {
                self.regs.a = self.regs.e;
                4
            }
            0x7C => {
                self.regs.a = self.regs.h;
                4
            }
            0x7D => {
                self.regs.a = self.regs.l;
                4
            }
            0x7F => 4,
            0x80 => {
                self.add_a(self.regs.b);
                4
            }
            0x81 => {
                self.add_a(self.regs.c);
                4
            }
            0x82 => {
                self.add_a(self.regs.d);
                4
            }
            0x83 => {
                self.add_a(self.regs.e);
                4
            }
            0x84 => {
                self.add_a(self.regs.h);
                4
            }
            0x85 => {
                self.add_a(self.regs.l);
                4
            }
            0x87 => {
                self.add_a(self.regs.a);
                4
            }
            0xAF => {
                self.regs.a ^= self.regs.a;
                self.regs.f = FLAG_Z;
                4
            }
            0xC6 => {
                let value = self.fetch8(bus);
                self.add_a(value);
                8
            }
            0x3C => {
                let a = self.regs.a;
                let result = a.wrapping_add(1);
                self.regs.a = result;
                self.set_flag(FLAG_Z, result == 0);
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, (a & 0x0F) + 1 > 0x0F);
                4
            }
            0x3D => {
                let a = self.regs.a;
                let result = a.wrapping_sub(1);
                self.regs.a = result;
                self.set_flag(FLAG_Z, result == 0);
                self.set_flag(FLAG_N, true);
                self.set_flag(FLAG_H, (a & 0x0F) == 0);
                4
            }
            0xFE => {
                let value = self.fetch8(bus);
                self.cp(value);
                8
            }
            0x18 => {
                let rel = self.fetch8(bus) as i8 as i16 as u16;
                self.regs.pc = self.regs.pc.wrapping_add(rel);
                12
            }
            0x20 => {
                let rel = self.fetch8(bus) as i8 as i16 as u16;
                if !self.flag(FLAG_Z) {
                    self.regs.pc = self.regs.pc.wrapping_add(rel);
                    12
                } else {
                    8
                }
            }
            0xC3 => {
                let addr = self.fetch16(bus);
                self.regs.pc = addr;
                16
            }
            0xCD => {
                let addr = self.fetch16(bus);
                self.push16(bus, self.regs.pc);
                self.regs.pc = addr;
                24
            }
            0xC9 => {
                self.regs.pc = self.pop16(bus);
                16
            }
            0xD9 => {
                self.regs.pc = self.pop16(bus);
                self.ime = true;
                16
            }
            0xE0 => {
                let offset = self.fetch8(bus);
                bus.write8(0xFF00 | offset as u16, self.regs.a);
                12
            }
            0xEA => {
                let addr = self.fetch16(bus);
                bus.write8(addr, self.regs.a);
                16
            }
            0xFA => {
                let addr = self.fetch16(bus);
                self.regs.a = bus.read8(addr);
                16
            }
            0xF3 => {
                self.ime = false;
                4
            }
            0xFB => {
                self.ime = true;
                4
            }
            0x76 => {
                self.halted = true;
                4
            }
            _ => {
                // TODO: replace with strict decode table once opcode coverage reaches baseline.
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

    fn fetch16(&mut self, bus: &GbBus) -> u16 {
        let lo = self.fetch8(bus) as u16;
        let hi = self.fetch8(bus) as u16;
        (hi << 8) | lo
    }

    fn push16(&mut self, bus: &mut GbBus, value: u16) {
        self.regs.sp = self.regs.sp.wrapping_sub(1);
        bus.write8(self.regs.sp, (value >> 8) as u8);
        self.regs.sp = self.regs.sp.wrapping_sub(1);
        bus.write8(self.regs.sp, (value & 0x00FF) as u8);
    }

    fn pop16(&mut self, bus: &GbBus) -> u16 {
        let lo = bus.read8(self.regs.sp) as u16;
        self.regs.sp = self.regs.sp.wrapping_add(1);
        let hi = bus.read8(self.regs.sp) as u16;
        self.regs.sp = self.regs.sp.wrapping_add(1);
        (hi << 8) | lo
    }

    fn add_a(&mut self, rhs: u8) {
        let lhs = self.regs.a;
        let (result, carry) = lhs.overflowing_add(rhs);
        self.regs.a = result;

        self.set_flag(FLAG_Z, result == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, ((lhs & 0x0F) + (rhs & 0x0F)) > 0x0F);
        self.set_flag(FLAG_C, carry);
    }

    fn cp(&mut self, rhs: u8) {
        let lhs = self.regs.a;
        let result = lhs.wrapping_sub(rhs);
        self.set_flag(FLAG_Z, result == 0);
        self.set_flag(FLAG_N, true);
        self.set_flag(FLAG_H, (lhs & 0x0F) < (rhs & 0x0F));
        self.set_flag(FLAG_C, lhs < rhs);
    }

    fn flag(&self, flag: u8) -> bool {
        (self.regs.f & flag) != 0
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
    fn ld_a_and_add_immediate_updates_flags() {
        let (mut cpu, mut bus) = cpu_with_program(&[0x3E, 0x0F, 0xC6, 0x01]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        let regs = cpu.regs();
        assert_eq!(regs.a, 0x10);
        assert_eq!(regs.f & FLAG_H, FLAG_H);
        assert_eq!(regs.f & FLAG_N, 0);
    }

    #[test]
    fn ld_register_chain_and_add_register() {
        let (mut cpu, mut bus) = cpu_with_program(&[0x06, 0x02, 0x78, 0x80]);
        cpu.step(&mut bus); // LD B,2
        cpu.step(&mut bus); // LD A,B
        cpu.step(&mut bus); // ADD A,B
        let regs = cpu.regs();
        assert_eq!(regs.a, 0x04);
    }

    #[test]
    fn jp_and_jr_change_program_counter() {
        let (mut cpu, mut bus) =
            cpu_with_program(&[0xC3, 0x08, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0xFE]);
        cpu.step(&mut bus); // JP 0108
        assert_eq!(cpu.regs().pc, 0x0108);
        cpu.step(&mut bus); // JR -2
        assert_eq!(cpu.regs().pc, 0x0108);
    }

    #[test]
    fn call_and_ret_round_trip_program_counter() {
        let (mut cpu, mut bus) = cpu_with_program(&[
            0xCD, 0x06, 0x01, // CALL 0106
            0x00, // NOP
            0x00, // NOP
            0x00, // NOP
            0x3E, 0x44, // LD A,44
            0xC9, // RET
        ]);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs().pc, 0x0106);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs().a, 0x44);
        assert_eq!(cpu.regs().pc, 0x0103);
    }

    #[test]
    fn cp_sets_carry_and_zero_as_expected() {
        let (mut cpu, mut bus) = cpu_with_program(&[0x3E, 0x05, 0xFE, 0x05, 0xFE, 0x06]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs().f & FLAG_Z, FLAG_Z);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs().f & FLAG_C, FLAG_C);
    }

    #[test]
    fn interrupt_service_pushes_pc_and_jumps_vector() {
        let (mut cpu, mut bus) = cpu_with_program(&[0x00]);
        cpu.ime = true;
        bus.write8(IE_ADDR, 0x04);
        bus.write8(IF_ADDR, 0x04);

        let cycles = cpu
            .service_interrupt(&mut bus)
            .expect("interrupt should fire");
        assert_eq!(cycles, 20);
        assert_eq!(cpu.regs().pc, 0x0050);
    }
}
