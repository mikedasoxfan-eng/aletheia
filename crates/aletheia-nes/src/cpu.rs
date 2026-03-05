use crate::bus::NesBus;

const FLAG_ZERO: u8 = 0x02;
const FLAG_INTERRUPT_DISABLE: u8 = 0x04;
const FLAG_UNUSED: u8 = 0x20;
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

    pub fn step(&mut self, bus: &mut NesBus) -> StepInfo {
        let opcode = self.fetch8(bus);
        let cycles = match opcode {
            0xEA => 2, // NOP
            0xA9 => {
                // LDA #imm
                let value = self.fetch8(bus);
                self.regs.a = value;
                self.update_zn(self.regs.a);
                2
            }
            0xAA => {
                // TAX
                self.regs.x = self.regs.a;
                self.update_zn(self.regs.x);
                2
            }
            0xE8 => {
                // INX
                self.regs.x = self.regs.x.wrapping_add(1);
                self.update_zn(self.regs.x);
                2
            }
            0xCA => {
                // DEX
                self.regs.x = self.regs.x.wrapping_sub(1);
                self.update_zn(self.regs.x);
                2
            }
            _ => {
                // TODO: Replace fallback with strict decode table + error reporting.
                2
            }
        };

        StepInfo { opcode, cycles }
    }

    fn fetch8(&mut self, bus: &NesBus) -> u8 {
        let value = bus.read8(self.regs.pc);
        self.regs.pc = self.regs.pc.wrapping_add(1);
        value
    }

    fn update_zn(&mut self, value: u8) {
        self.set_flag(FLAG_ZERO, value == 0);
        self.set_flag(FLAG_NEGATIVE, (value & 0x80) != 0);
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
        cpu.step(&mut bus);
        assert_eq!(cpu.regs().a, 0x00);
        assert_eq!(cpu.regs().p & FLAG_ZERO, FLAG_ZERO);
        assert_eq!(cpu.regs().p & FLAG_NEGATIVE, 0);

        cpu.step(&mut bus);
        assert_eq!(cpu.regs().a, 0x80);
        assert_eq!(cpu.regs().p & FLAG_ZERO, 0);
        assert_eq!(cpu.regs().p & FLAG_NEGATIVE, FLAG_NEGATIVE);
    }

    #[test]
    fn tax_and_index_ops_update_x_and_flags() {
        let (mut cpu, mut bus) = cpu_with_program(&[0xA9, 0x01, 0xAA, 0xE8, 0xCA, 0xCA]);
        cpu.step(&mut bus); // LDA #1
        cpu.step(&mut bus); // TAX
        assert_eq!(cpu.regs().x, 0x01);
        assert_eq!(cpu.regs().p & FLAG_ZERO, 0);

        cpu.step(&mut bus); // INX -> 2
        assert_eq!(cpu.regs().x, 0x02);

        cpu.step(&mut bus); // DEX -> 1
        assert_eq!(cpu.regs().x, 0x01);
        assert_eq!(cpu.regs().p & FLAG_ZERO, 0);

        cpu.step(&mut bus); // DEX -> 0
        assert_eq!(cpu.regs().x, 0x00);
        assert_eq!(cpu.regs().p & FLAG_ZERO, FLAG_ZERO);
    }

    #[test]
    fn step_reports_instruction_cycle_count() {
        let (mut cpu, mut bus) = cpu_with_program(&[0xEA, 0xA9, 0x44]);
        assert_eq!(
            cpu.step(&mut bus),
            StepInfo {
                opcode: 0xEA,
                cycles: 2
            }
        );
        assert_eq!(
            cpu.step(&mut bus),
            StepInfo {
                opcode: 0xA9,
                cycles: 2
            }
        );
    }

    #[test]
    fn status_preserves_unrelated_flags_across_zn_updates() {
        const FLAG_CARRY: u8 = 0x01;
        const FLAG_DECIMAL: u8 = 0x08;
        const FLAG_BREAK: u8 = 0x10;
        const FLAG_OVERFLOW: u8 = 0x40;
        const STICKY_FLAGS: u8 = FLAG_CARRY
            | FLAG_INTERRUPT_DISABLE
            | FLAG_DECIMAL
            | FLAG_BREAK
            | FLAG_OVERFLOW
            | FLAG_UNUSED;

        let (mut cpu, mut bus) = cpu_with_program(&[0xA9, 0x00, 0xA9, 0x7F]);
        cpu.regs.p = STICKY_FLAGS;

        cpu.step(&mut bus);
        assert_eq!(cpu.regs().p & STICKY_FLAGS, STICKY_FLAGS);
        assert_eq!(cpu.regs().p & FLAG_ZERO, FLAG_ZERO);

        cpu.step(&mut bus);
        assert_eq!(cpu.regs().p & STICKY_FLAGS, STICKY_FLAGS);
        assert_eq!(cpu.regs().p & FLAG_ZERO, 0);
    }
}
