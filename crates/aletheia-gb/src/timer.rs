use crate::bus::GbBus;

const DIV_ADDR: u16 = 0xFF04;
const TIMA_ADDR: u16 = 0xFF05;
const TMA_ADDR: u16 = 0xFF06;
const TAC_ADDR: u16 = 0xFF07;
const IF_ADDR: u16 = 0xFF0F;
const TIMER_IRQ_MASK: u8 = 0x04;

#[derive(Debug, Default)]
pub struct GbTimer {
    div_counter: u16,
    timer_counter: u16,
}

impl GbTimer {
    pub fn reset(&mut self, bus: &mut GbBus) {
        self.div_counter = 0;
        self.timer_counter = 0;
        bus.write8_raw(DIV_ADDR, 0);
        bus.write8_raw(TIMA_ADDR, 0);
        bus.write8_raw(TMA_ADDR, 0);
        bus.write8_raw(TAC_ADDR, 0);
    }

    pub fn tick(&mut self, bus: &mut GbBus, cycles: u8) {
        for _ in 0..cycles {
            self.div_counter = self.div_counter.wrapping_add(1);
            if self.div_counter >= 256 {
                self.div_counter -= 256;
                let next_div = bus.read8(DIV_ADDR).wrapping_add(1);
                bus.write8_raw(DIV_ADDR, next_div);
            }

            let tac = bus.read8(TAC_ADDR);
            if (tac & 0x04) == 0 {
                continue;
            }

            self.timer_counter = self.timer_counter.wrapping_add(1);
            let period = match tac & 0x03 {
                0x00 => 1024,
                0x01 => 16,
                0x02 => 64,
                _ => 256,
            };

            if self.timer_counter >= period {
                self.timer_counter -= period;
                let tima = bus.read8(TIMA_ADDR);
                if tima == 0xFF {
                    let tma = bus.read8(TMA_ADDR);
                    bus.write8_raw(TIMA_ADDR, tma);
                    let iflags = bus.read8(IF_ADDR) | TIMER_IRQ_MASK;
                    bus.write8_raw(IF_ADDR, iflags);
                } else {
                    bus.write8_raw(TIMA_ADDR, tima.wrapping_add(1));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_overflow_sets_interrupt_flag() {
        let mut bus = GbBus::default();
        let mut timer = GbTimer::default();
        timer.reset(&mut bus);

        bus.write8_raw(TMA_ADDR, 0xAA);
        bus.write8_raw(TIMA_ADDR, 0xFF);
        bus.write8_raw(TAC_ADDR, 0x05); // enable, 16 cycles
        timer.tick(&mut bus, 16);

        assert_eq!(bus.read8(TIMA_ADDR), 0xAA);
        assert_eq!(bus.read8(IF_ADDR) & TIMER_IRQ_MASK, TIMER_IRQ_MASK);
    }
}
