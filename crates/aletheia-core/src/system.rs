use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SystemId {
    GbDmg,
    GbCgb,
    Nes,
    Gba,
}

impl fmt::Display for SystemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::GbDmg => "gb-dmg",
            Self::GbCgb => "gb-cgb",
            Self::Nes => "nes",
            Self::Gba => "gba",
        };
        f.write_str(label)
    }
}
