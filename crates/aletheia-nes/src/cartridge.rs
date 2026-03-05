use thiserror::Error;

const INES_HEADER_SIZE: usize = 16;
const NES_MAGIC: &[u8; 4] = b"NES\x1A";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapperKind {
    Nrom,
    Mmc1,
    Unsupported(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartridgeInfo {
    pub mapper: u16,
    pub prg_rom_bytes: usize,
    pub chr_rom_bytes: usize,
    pub mapper_kind: MapperKind,
}

#[derive(Debug, Error)]
pub enum CartridgeError {
    #[error("ROM is too small to be an iNES file")]
    RomTooSmall,
    #[error("ROM does not contain a valid iNES header")]
    InvalidHeader,
}

#[derive(Debug, Clone)]
struct Mmc1State {
    control: u8,
    chr_bank_0: u8,
    chr_bank_1: u8,
    prg_bank: u8,
    shift_register: u8,
}

impl Default for Mmc1State {
    fn default() -> Self {
        Self {
            control: 0x0C,
            chr_bank_0: 0,
            chr_bank_1: 0,
            prg_bank: 0,
            shift_register: 0x10,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NesCartridge {
    prg_rom: Vec<u8>,
    _chr_rom: Vec<u8>,
    info: CartridgeInfo,
    mmc1: Mmc1State,
}

impl NesCartridge {
    pub fn from_ines(rom: &[u8]) -> Result<Self, CartridgeError> {
        if rom.len() < INES_HEADER_SIZE {
            return Err(CartridgeError::RomTooSmall);
        }
        if &rom[..4] != NES_MAGIC {
            return Err(CartridgeError::InvalidHeader);
        }

        let prg_rom_bytes = rom[4] as usize * 16 * 1024;
        let chr_rom_bytes = rom[5] as usize * 8 * 1024;
        let mapper = ((rom[7] as u16) & 0xF0) | ((rom[6] as u16) >> 4);
        let mapper_kind = match mapper {
            0 => MapperKind::Nrom,
            1 => MapperKind::Mmc1,
            _ => MapperKind::Unsupported(mapper),
        };

        let has_trainer = (rom[6] & 0x04) != 0;
        let mut cursor = INES_HEADER_SIZE;
        if has_trainer {
            cursor += 512;
        }

        let prg_end = cursor + prg_rom_bytes;
        if prg_end > rom.len() {
            return Err(CartridgeError::RomTooSmall);
        }
        let chr_end = prg_end + chr_rom_bytes;
        if chr_end > rom.len() {
            return Err(CartridgeError::RomTooSmall);
        }

        Ok(Self {
            prg_rom: rom[cursor..prg_end].to_vec(),
            _chr_rom: rom[prg_end..chr_end].to_vec(),
            info: CartridgeInfo {
                mapper,
                prg_rom_bytes,
                chr_rom_bytes,
                mapper_kind,
            },
            mmc1: Mmc1State::default(),
        })
    }

    pub fn info(&self) -> &CartridgeInfo {
        &self.info
    }

    pub fn cpu_write(&mut self, addr: u16, value: u8) {
        if !matches!(self.info.mapper_kind, MapperKind::Mmc1) {
            return;
        }

        if (value & 0x80) != 0 {
            self.mmc1.shift_register = 0x10;
            self.mmc1.control |= 0x0C;
            return;
        }

        let complete = (self.mmc1.shift_register & 1) == 1;
        self.mmc1.shift_register >>= 1;
        self.mmc1.shift_register |= (value & 1) << 4;

        if complete {
            let data = self.mmc1.shift_register;
            match addr {
                0x8000..=0x9FFF => self.mmc1.control = data,
                0xA000..=0xBFFF => self.mmc1.chr_bank_0 = data,
                0xC000..=0xDFFF => self.mmc1.chr_bank_1 = data,
                0xE000..=0xFFFF => self.mmc1.prg_bank = data & 0x0F,
                _ => {}
            }
            self.mmc1.shift_register = 0x10;
        }
    }

    pub fn prg_read8(&self, addr: u16) -> u8 {
        if self.prg_rom.is_empty() {
            return 0xFF;
        }

        let bank_size = 0x4000usize;
        let bank_count = self.prg_rom.len().div_ceil(bank_size).max(1);

        let bank = match self.info.mapper_kind {
            MapperKind::Nrom => {
                let offset = (addr as usize) - 0x8000;
                if bank_count == 1 {
                    return self.prg_rom[offset % bank_size];
                }
                return self.prg_rom[offset % self.prg_rom.len()];
            }
            MapperKind::Mmc1 => {
                let mode = (self.mmc1.control >> 2) & 0x03;
                let prg_bank = self.mmc1.prg_bank as usize % bank_count;

                match mode {
                    0 | 1 => {
                        let pair_count = (bank_count / 2).max(1);
                        let bank32 = (prg_bank & !1) % pair_count;
                        if addr < 0xC000 {
                            bank32 * 2
                        } else {
                            (bank32 * 2 + 1).min(bank_count - 1)
                        }
                    }
                    2 => {
                        if addr < 0xC000 {
                            0
                        } else {
                            prg_bank
                        }
                    }
                    _ => {
                        if addr < 0xC000 {
                            prg_bank
                        } else {
                            bank_count - 1
                        }
                    }
                }
            }
            MapperKind::Unsupported(_) => {
                let offset = (addr as usize) - 0x8000;
                return self.prg_rom[offset % self.prg_rom.len()];
            }
        };

        let offset = (addr as usize) & 0x3FFF;
        let index = bank * bank_size + offset;
        self.prg_rom.get(index).copied().unwrap_or(0xFF)
    }

    #[cfg(test)]
    pub fn chr_len(&self) -> usize {
        self._chr_rom.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ines(prg_banks: u8, mapper: u8) -> Vec<u8> {
        let prg_len = prg_banks as usize * 16 * 1024;
        let mut rom = vec![0; 16 + prg_len];
        rom[..4].copy_from_slice(b"NES\x1A");
        rom[4] = prg_banks;
        rom[6] = mapper << 4;
        for bank in 0..prg_banks as usize {
            for i in 0..0x4000 {
                rom[16 + bank * 0x4000 + i] = bank as u8;
            }
        }
        rom
    }

    fn mmc1_write_register(cart: &mut NesCartridge, addr: u16, value5: u8) {
        for i in 0..5 {
            let bit = (value5 >> i) & 1;
            cart.cpu_write(addr, bit);
        }
    }

    #[test]
    fn parses_nrom_cartridge() {
        let rom = make_ines(2, 0);
        let cart = NesCartridge::from_ines(&rom).expect("parse");
        assert_eq!(cart.info.mapper, 0);
        assert_eq!(cart.info.mapper_kind, MapperKind::Nrom);
        assert_eq!(cart.info.prg_rom_bytes, 32768);
        assert_eq!(cart.chr_len(), 0);
    }

    #[test]
    fn nrom_16k_mirrors_upper_bank() {
        let rom = make_ines(1, 0);
        let cart = NesCartridge::from_ines(&rom).expect("parse");
        assert_eq!(cart.prg_read8(0x8000), 0x00);
        assert_eq!(cart.prg_read8(0xC000), 0x00);
        assert_eq!(cart.prg_read8(0xBFFF), cart.prg_read8(0xFFFF));
    }

    #[test]
    fn mmc1_switches_16k_bank_in_mode3() {
        let rom = make_ines(4, 1);
        let mut cart = NesCartridge::from_ines(&rom).expect("parse");

        // control mode 3 => switch 0x8000, fix last bank at 0xC000
        mmc1_write_register(&mut cart, 0x8000, 0x0C);
        mmc1_write_register(&mut cart, 0xE000, 0x02);

        assert_eq!(cart.prg_read8(0x8000), 0x02);
        assert_eq!(cart.prg_read8(0xC000), 0x03);
    }
}
