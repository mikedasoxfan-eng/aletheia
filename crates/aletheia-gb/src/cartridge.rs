use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MbcKind {
    RomOnly,
    Mbc1,
    Mbc3,
    Mbc5,
    Unsupported(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartridgeInfo {
    pub mbc_kind: MbcKind,
    pub cgb_flag: u8,
    pub title: String,
}

#[derive(Debug, Error)]
pub enum CartridgeError {
    #[error("ROM is too small to contain a valid GB header")]
    RomTooSmall,
}

#[derive(Debug, Clone)]
pub struct GbCartridge {
    rom: Vec<u8>,
    ram: Vec<u8>,
    info: CartridgeInfo,
    selected_rom_bank: usize,
    ram_enabled: bool,
}

impl GbCartridge {
    pub fn from_rom(rom: &[u8]) -> Result<Self, CartridgeError> {
        if rom.len() < 0x150 {
            return Err(CartridgeError::RomTooSmall);
        }

        let cart_type = rom[0x147];
        let mbc_kind = match cart_type {
            0x00 => MbcKind::RomOnly,
            0x01..=0x03 => MbcKind::Mbc1,
            0x0F..=0x13 => MbcKind::Mbc3,
            0x19..=0x1E => MbcKind::Mbc5,
            other => MbcKind::Unsupported(other),
        };
        let cgb_flag = rom[0x143];
        let title = decode_title(&rom[0x134..=0x143]);

        Ok(Self {
            rom: rom.to_vec(),
            ram: vec![0; 0x2000],
            info: CartridgeInfo {
                mbc_kind,
                cgb_flag,
                title,
            },
            selected_rom_bank: 1,
            ram_enabled: false,
        })
    }

    pub fn info(&self) -> &CartridgeInfo {
        &self.info
    }

    pub fn read8(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.read_rom_bank(0, addr as usize),
            0x4000..=0x7FFF => {
                let offset = (addr as usize) - 0x4000;
                self.read_rom_bank(self.selected_rom_bank, offset)
            }
            0xA000..=0xBFFF => {
                if self.ram_enabled {
                    self.ram[(addr as usize) - 0xA000]
                } else {
                    0xFF
                }
            }
            _ => 0xFF,
        }
    }

    pub fn write8(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => {
                self.ram_enabled = (value & 0x0F) == 0x0A;
            }
            0x2000..=0x3FFF => match self.info.mbc_kind {
                MbcKind::RomOnly => {}
                MbcKind::Mbc1 => {
                    let mut bank = (value & 0x1F) as usize;
                    if bank == 0 {
                        bank = 1;
                    }
                    self.selected_rom_bank = bank;
                }
                MbcKind::Mbc3 => {
                    let bank = (value & 0x7F) as usize;
                    self.selected_rom_bank = bank.max(1);
                }
                MbcKind::Mbc5 => {
                    self.selected_rom_bank = value as usize;
                }
                MbcKind::Unsupported(_) => {}
            },
            0xA000..=0xBFFF => {
                if self.ram_enabled {
                    let offset = (addr as usize) - 0xA000;
                    self.ram[offset] = value;
                }
            }
            _ => {}
        }
    }

    fn read_rom_bank(&self, bank: usize, offset: usize) -> u8 {
        let bank_size = 0x4000usize;
        if self.rom.is_empty() {
            return 0xFF;
        }
        let bank_count = self.rom.len().div_ceil(bank_size).max(1);
        let normalized_bank = bank % bank_count;
        let index = normalized_bank * bank_size + offset;
        self.rom.get(index).copied().unwrap_or(0xFF)
    }
}

fn decode_title(bytes: &[u8]) -> String {
    let filtered: Vec<u8> = bytes
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .filter(|byte| byte.is_ascii_graphic() || *byte == b' ')
        .collect();
    String::from_utf8_lossy(&filtered).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_test_rom(cart_type: u8) -> Vec<u8> {
        let mut rom = vec![0; 0x8000];
        rom[0x134..0x139].copy_from_slice(b"HELLO");
        rom[0x143] = 0x00;
        rom[0x147] = cart_type;
        rom
    }

    #[test]
    fn parses_cartridge_info() {
        let rom = build_test_rom(0x01);
        let cart = GbCartridge::from_rom(&rom).expect("cartridge");
        assert_eq!(cart.info.title, "HELLO");
        assert_eq!(cart.info.mbc_kind, MbcKind::Mbc1);
    }

    #[test]
    fn mbc1_switches_rom_bank() {
        let mut rom = vec![0; 0x4000 * 3];
        rom[0x147] = 0x01;
        rom[0x4000] = 0x11; // bank 1 value
        rom[0x8000] = 0x22; // bank 2 value
        let mut cart = GbCartridge::from_rom(&rom).expect("cartridge");

        assert_eq!(cart.read8(0x4000), 0x11);
        cart.write8(0x2000, 0x02);
        assert_eq!(cart.read8(0x4000), 0x22);
    }
}
