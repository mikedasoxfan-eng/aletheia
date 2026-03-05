use crate::cartridge::{CartridgeError, CartridgeInfo, GbCartridge};

pub const RAM_SIZE: usize = 0x1_0000;
pub const PROGRAM_START: u16 = 0x0100;
const DIV_ADDR: u16 = 0xFF04;

#[derive(Debug, Clone)]
pub struct GbBus {
    mem: [u8; RAM_SIZE],
    cartridge: Option<GbCartridge>,
}

impl Default for GbBus {
    fn default() -> Self {
        Self {
            mem: [0; RAM_SIZE],
            cartridge: None,
        }
    }
}

impl GbBus {
    pub fn clear_runtime(&mut self) {
        self.mem = [0; RAM_SIZE];
    }

    pub fn load_cartridge(&mut self, rom: &[u8]) -> Result<(), CartridgeError> {
        self.cartridge = Some(GbCartridge::from_rom(rom)?);
        Ok(())
    }

    pub fn has_cartridge(&self) -> bool {
        self.cartridge.is_some()
    }

    pub fn cartridge_info(&self) -> Option<&CartridgeInfo> {
        self.cartridge.as_ref().map(GbCartridge::info)
    }

    pub fn read8(&self, addr: u16) -> u8 {
        if let Some(cartridge) = &self.cartridge {
            match addr {
                0x0000..=0x7FFF | 0xA000..=0xBFFF => return cartridge.read8(addr),
                _ => {}
            }
        }
        self.mem[addr as usize]
    }

    pub fn write8(&mut self, addr: u16, value: u8) {
        if let Some(cartridge) = &mut self.cartridge {
            match addr {
                0x0000..=0x7FFF | 0xA000..=0xBFFF => {
                    cartridge.write8(addr, value);
                    return;
                }
                _ => {}
            }
        }
        if addr == DIV_ADDR {
            self.mem[DIV_ADDR as usize] = 0;
            return;
        }
        self.mem[addr as usize] = value;
    }

    pub fn write8_raw(&mut self, addr: u16, value: u8) {
        self.mem[addr as usize] = value;
    }

    pub fn load_program(&mut self, start: u16, program: &[u8]) {
        self.cartridge = None;
        for (offset, byte) in program.iter().copied().enumerate() {
            let addr = start.wrapping_add(offset as u16);
            self.write8(addr, byte);
        }
    }
}
