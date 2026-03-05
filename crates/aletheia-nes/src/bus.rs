use crate::cartridge::{CartridgeError, CartridgeInfo, NesCartridge};

pub const RAM_SIZE: usize = 0x1_0000;
pub const PROGRAM_START: u16 = 0x8000;
pub const RESET_VECTOR: u16 = 0xFFFC;

#[derive(Debug, Clone)]
pub struct NesBus {
    mem: [u8; RAM_SIZE],
    cartridge: Option<NesCartridge>,
}

impl Default for NesBus {
    fn default() -> Self {
        Self {
            mem: [0; RAM_SIZE],
            cartridge: None,
        }
    }
}

impl NesBus {
    pub fn clear_runtime(&mut self) {
        self.mem = [0; RAM_SIZE];
    }

    pub fn load_cartridge(&mut self, rom: &[u8]) -> Result<(), CartridgeError> {
        self.cartridge = Some(NesCartridge::from_ines(rom)?);
        Ok(())
    }

    pub fn has_cartridge(&self) -> bool {
        self.cartridge.is_some()
    }

    pub fn cartridge_info(&self) -> Option<&CartridgeInfo> {
        self.cartridge.as_ref().map(NesCartridge::info)
    }

    pub fn read8(&self, addr: u16) -> u8 {
        if let Some(cartridge) = &self.cartridge {
            if (0x8000..=0xFFFF).contains(&addr) {
                return cartridge.prg_read8(addr);
            }
        }
        self.mem[addr as usize]
    }

    pub fn write8(&mut self, addr: u16, value: u8) {
        if let Some(cartridge) = &mut self.cartridge {
            if (0x8000..=0xFFFF).contains(&addr) {
                cartridge.cpu_write(addr, value);
                return;
            }
        }
        self.mem[addr as usize] = value;
    }

    pub fn load_program(&mut self, start: u16, program: &[u8]) {
        self.cartridge = None;
        for (offset, byte) in program.iter().copied().enumerate() {
            let addr = start.wrapping_add(offset as u16);
            self.write8(addr, byte);
        }
    }

    pub fn set_reset_vector(&mut self, target: u16) {
        self.write8(RESET_VECTOR, (target & 0x00FF) as u8);
        self.write8(RESET_VECTOR.wrapping_add(1), (target >> 8) as u8);
    }
}
