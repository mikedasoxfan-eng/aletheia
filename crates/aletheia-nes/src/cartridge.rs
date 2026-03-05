use thiserror::Error;

const INES_HEADER_SIZE: usize = 16;
const NES_MAGIC: &[u8; 4] = b"NES\x1A";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapperKind {
    Nrom,
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
pub struct NesCartridge {
    prg_rom: Vec<u8>,
    _chr_rom: Vec<u8>,
    info: CartridgeInfo,
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
        let mapper_kind = if mapper == 0 {
            MapperKind::Nrom
        } else {
            MapperKind::Unsupported(mapper)
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
        })
    }

    pub fn info(&self) -> &CartridgeInfo {
        &self.info
    }

    pub fn prg_read8(&self, addr: u16) -> u8 {
        if self.prg_rom.is_empty() {
            return 0xFF;
        }

        let offset = (addr as usize) - 0x8000;
        let mapped = if self.prg_rom.len() == 0x4000 {
            offset % 0x4000
        } else {
            offset % self.prg_rom.len()
        };
        self.prg_rom[mapped]
    }

    #[cfg(test)]
    pub fn chr_len(&self) -> usize {
        self._chr_rom.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ines(prg_banks: u8, mapper_low: u8, mapper_high: u8) -> Vec<u8> {
        let prg_len = prg_banks as usize * 16 * 1024;
        let mut rom = vec![0; 16 + prg_len];
        rom[..4].copy_from_slice(b"NES\x1A");
        rom[4] = prg_banks;
        rom[6] = mapper_low << 4;
        rom[7] = mapper_high << 4;
        for i in 0..prg_len {
            rom[16 + i] = (i & 0xFF) as u8;
        }
        rom
    }

    #[test]
    fn parses_nrom_cartridge() {
        let rom = make_ines(2, 0, 0);
        let cart = NesCartridge::from_ines(&rom).expect("parse");
        assert_eq!(cart.info.mapper, 0);
        assert_eq!(cart.info.mapper_kind, MapperKind::Nrom);
        assert_eq!(cart.info.prg_rom_bytes, 32768);
        assert_eq!(cart.chr_len(), 0);
    }

    #[test]
    fn nrom_16k_mirrors_upper_bank() {
        let rom = make_ines(1, 0, 0);
        let cart = NesCartridge::from_ines(&rom).expect("parse");
        assert_eq!(cart.prg_read8(0x8000), 0x00);
        assert_eq!(cart.prg_read8(0xC000), 0x00);
        assert_eq!(cart.prg_read8(0xBFFF), cart.prg_read8(0xFFFF));
    }
}
