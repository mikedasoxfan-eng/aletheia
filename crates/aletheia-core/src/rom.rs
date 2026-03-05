use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;

const NES_MAGIC: &[u8; 4] = b"NES\x1A";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RomFormat {
    Gb,
    Gbc,
    Nes,
    Gba,
    Unknown,
}

impl RomFormat {
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Gb => "gb",
            Self::Gbc => "gbc",
            Self::Nes => "nes",
            Self::Gba => "gba",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GbMetadata {
    pub title: String,
    pub cartridge_type: u8,
    pub cgb_compatible: bool,
    pub cgb_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NesMetadata {
    pub mapper: u16,
    pub prg_rom_bytes: usize,
    pub chr_rom_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GbaMetadata {
    pub title: String,
    pub game_code: String,
    pub maker_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RomMetadata {
    Gb(GbMetadata),
    Nes(NesMetadata),
    Gba(GbaMetadata),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RomImage {
    pub path: String,
    pub format: RomFormat,
    pub byte_len: usize,
    pub blake3: String,
    pub metadata: RomMetadata,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum RomError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("ROM file is empty")]
    Empty,
}

pub fn load_rom_image(path: &Path) -> Result<RomImage, RomError> {
    let bytes = fs::read(path)?;
    if bytes.is_empty() {
        return Err(RomError::Empty);
    }

    let format = detect_rom_format(path, &bytes);
    let metadata = parse_metadata(format, &bytes);
    let mut hasher = Hasher::new();
    hasher.update(&bytes);
    let blake3 = hasher.finalize().to_hex().to_string();

    Ok(RomImage {
        path: path.to_string_lossy().to_string(),
        format,
        byte_len: bytes.len(),
        blake3,
        metadata,
        bytes,
    })
}

pub fn detect_rom_format(path: &Path, bytes: &[u8]) -> RomFormat {
    if bytes.len() >= NES_MAGIC.len() && &bytes[..NES_MAGIC.len()] == NES_MAGIC {
        return RomFormat::Nes;
    }

    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    if let Some(ext) = ext {
        match ext.as_str() {
            "gb" => return RomFormat::Gb,
            "gbc" => return RomFormat::Gbc,
            "nes" => return RomFormat::Nes,
            "gba" => return RomFormat::Gba,
            _ => {}
        }
    }

    if bytes.len() >= 0xB0 {
        // GBA header checksum byte exists at 0xBD, but this heuristic is enough for initial routing.
        let game_code = &bytes[0xAC..0xB0];
        if game_code.iter().all(|b| b.is_ascii_alphanumeric()) {
            return RomFormat::Gba;
        }
    }

    if bytes.len() > 0x150 {
        let cgb_flag = bytes[0x143];
        if matches!(cgb_flag, 0x80 | 0xC0) {
            return RomFormat::Gbc;
        }
        return RomFormat::Gb;
    }

    RomFormat::Unknown
}

fn parse_metadata(format: RomFormat, bytes: &[u8]) -> RomMetadata {
    match format {
        RomFormat::Gb | RomFormat::Gbc => {
            parse_gb_metadata(bytes).map_or(RomMetadata::Unknown, RomMetadata::Gb)
        }
        RomFormat::Nes => parse_nes_metadata(bytes).map_or(RomMetadata::Unknown, RomMetadata::Nes),
        RomFormat::Gba => parse_gba_metadata(bytes).map_or(RomMetadata::Unknown, RomMetadata::Gba),
        RomFormat::Unknown => RomMetadata::Unknown,
    }
}

fn parse_gb_metadata(bytes: &[u8]) -> Option<GbMetadata> {
    if bytes.len() <= 0x147 {
        return None;
    }
    let title_range = &bytes[0x134..=0x143];
    let title = decode_ascii(title_range);
    let cgb_flag = bytes[0x143];

    Some(GbMetadata {
        title,
        cartridge_type: bytes[0x147],
        cgb_compatible: cgb_flag == 0x80 || cgb_flag == 0xC0,
        cgb_only: cgb_flag == 0xC0,
    })
}

fn parse_nes_metadata(bytes: &[u8]) -> Option<NesMetadata> {
    if bytes.len() < 16 || &bytes[..4] != NES_MAGIC {
        return None;
    }
    let mapper = ((bytes[7] as u16) & 0xF0) | ((bytes[6] as u16) >> 4);
    let prg_rom_bytes = bytes[4] as usize * 16 * 1024;
    let chr_rom_bytes = bytes[5] as usize * 8 * 1024;

    Some(NesMetadata {
        mapper,
        prg_rom_bytes,
        chr_rom_bytes,
    })
}

fn parse_gba_metadata(bytes: &[u8]) -> Option<GbaMetadata> {
    if bytes.len() < 0xB2 {
        return None;
    }
    Some(GbaMetadata {
        title: decode_ascii(&bytes[0xA0..0xAC]),
        game_code: decode_ascii(&bytes[0xAC..0xB0]),
        maker_code: decode_ascii(&bytes[0xB0..0xB2]),
    })
}

fn decode_ascii(bytes: &[u8]) -> String {
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
    use std::path::{Path, PathBuf};

    #[test]
    fn detects_nes_from_header_magic() {
        let mut bytes = vec![0; 32];
        bytes[..4].copy_from_slice(b"NES\x1A");
        let format = detect_rom_format(Path::new("mystery.bin"), &bytes);
        assert_eq!(format, RomFormat::Nes);
    }

    #[test]
    fn detects_gba_from_extension() {
        let bytes = vec![0; 0x200];
        let format = detect_rom_format(Path::new("test.gba"), &bytes);
        assert_eq!(format, RomFormat::Gba);
    }

    #[test]
    fn parses_gb_metadata_title_and_flags() {
        let mut bytes = vec![0; 0x200];
        bytes[0x134..0x13A].copy_from_slice(b"TETRIS");
        bytes[0x143] = 0x80;
        bytes[0x147] = 0x01;

        let metadata = parse_gb_metadata(&bytes).expect("metadata");
        assert_eq!(metadata.title, "TETRIS");
        assert_eq!(metadata.cartridge_type, 0x01);
        assert!(metadata.cgb_compatible);
        assert!(!metadata.cgb_only);
    }

    #[test]
    fn parses_nes_mapper_information() {
        let mut bytes = vec![0; 16];
        bytes[..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 0x10;
        bytes[7] = 0x20;

        let metadata = parse_nes_metadata(&bytes).expect("metadata");
        assert_eq!(metadata.mapper, 0x21);
        assert_eq!(metadata.prg_rom_bytes, 32 * 1024);
        assert_eq!(metadata.chr_rom_bytes, 8 * 1024);
    }

    #[test]
    fn parses_gba_metadata() {
        let mut bytes = vec![0; 0x200];
        bytes[0xA0..0xAC].copy_from_slice(b"POKEMONTEST!");
        bytes[0xAC..0xB0].copy_from_slice(b"ABCD");
        bytes[0xB0..0xB2].copy_from_slice(b"01");
        let metadata = parse_gba_metadata(&bytes).expect("metadata");
        assert_eq!(metadata.game_code, "ABCD");
        assert_eq!(metadata.maker_code, "01");
    }

    #[test]
    fn load_rom_image_populates_digest_and_metadata() {
        let mut tmp_path = std::env::temp_dir();
        tmp_path.push(format!("aletheia-rom-test-{}.gb", std::process::id()));
        let mut bytes = vec![0; 0x200];
        bytes[0x134..0x139].copy_from_slice(b"HELLO");
        bytes[0x143] = 0x00;
        bytes[0x147] = 0x00;
        fs::write(&tmp_path, &bytes).expect("write test rom");

        let image = load_rom_image(&tmp_path).expect("load rom");
        assert_eq!(image.format, RomFormat::Gb);
        assert_eq!(image.byte_len, bytes.len());
        assert!(!image.blake3.is_empty());
        assert!(matches!(image.metadata, RomMetadata::Gb(_)));

        fs::remove_file(PathBuf::from(&tmp_path)).expect("cleanup");
    }
}
