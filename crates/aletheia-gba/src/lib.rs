use aletheia_core::{
    CheckpointDigest, DeterminismError, DeterministicMachine, InputButton, InputEvent, InputState,
    ReplayLog, RunDigest, SystemId, run_deterministic, run_deterministic_with_checkpoint,
};
use thiserror::Error;

const ROM_BASE: u32 = 0x0800_0000;
const WRAM_BASE: u32 = 0x0200_0000;
const IO_BASE: u32 = 0x0400_0000;
const PALETTE_BASE: u32 = 0x0500_0000;
const VRAM_BASE: u32 = 0x0600_0000;
const OAM_BASE: u32 = 0x0700_0000;
const WRAM_SIZE: usize = 0x40000;
const IO_SIZE: usize = 0x400;
const PALETTE_SIZE: usize = 0x400;
const VRAM_SIZE: usize = 0x18000;
const OAM_SIZE: usize = 0x400;
const GBA_WIDTH: usize = 240;
const GBA_HEIGHT: usize = 160;
const CYCLES_PER_PIXEL: u16 = 4;
const VISIBLE_DOTS: u16 = (GBA_WIDTH as u16) * CYCLES_PER_PIXEL;
const DOTS_PER_LINE: u16 = 1232;
const LINES_PER_FRAME: u16 = 228;
const VISIBLE_LINES: u16 = GBA_HEIGHT as u16;
const CPSR_T_BIT: u32 = 1 << 5;
const CPSR_N_BIT: u32 = 1 << 31;
const CPSR_Z_BIT: u32 = 1 << 30;
const CPSR_C_BIT: u32 = 1 << 29;
const CPSR_V_BIT: u32 = 1 << 28;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    pub r0: u32,
    pub r1: u32,
    pub r2: u32,
    pub r3: u32,
    pub pc: u32,
    pub cpsr: u32,
    pub thumb: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepInfo {
    pub opcode: u32,
    pub cycles: u8,
    pub thumb: bool,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GbaCpuError {
    #[error("unsupported ARM opcode 0x{opcode:08X} at PC 0x{pc:08X}")]
    UnsupportedArm { opcode: u32, pc: u32 },
    #[error("unsupported THUMB opcode 0x{opcode:04X} at PC 0x{pc:08X}")]
    UnsupportedThumb { opcode: u16, pc: u32 },
}

#[derive(Debug, Clone)]
pub struct GbaBus {
    rom: Vec<u8>,
    wram: Vec<u8>,
    io: Vec<u8>,
    palette: Vec<u8>,
    vram: Vec<u8>,
    oam: Vec<u8>,
}

impl GbaBus {
    pub fn with_rom(rom: &[u8]) -> Self {
        Self {
            rom: rom.to_vec(),
            wram: vec![0; WRAM_SIZE],
            io: vec![0; IO_SIZE],
            palette: vec![0; PALETTE_SIZE],
            vram: vec![0; VRAM_SIZE],
            oam: vec![0; OAM_SIZE],
        }
    }

    fn reset_runtime(&mut self) {
        self.wram.fill(0);
        self.io.fill(0);
        self.palette.fill(0);
        self.vram.fill(0);
        self.oam.fill(0);
        // DISPCNT defaults to forced blank disabled, mode 0.
        self.write16(IO_BASE, 0x0000);
    }

    fn read8(&self, addr: u32) -> u8 {
        match addr {
            ROM_BASE..=0x09FF_FFFF => {
                let offset = addr.wrapping_sub(ROM_BASE) as usize;
                self.rom.get(offset).copied().unwrap_or(0)
            }
            WRAM_BASE..=0x0203_FFFF => {
                let offset = addr.wrapping_sub(WRAM_BASE) as usize;
                self.wram.get(offset).copied().unwrap_or(0)
            }
            IO_BASE..=0x0400_03FF => {
                let offset = addr.wrapping_sub(IO_BASE) as usize;
                self.io.get(offset).copied().unwrap_or(0)
            }
            PALETTE_BASE..=0x0500_03FF => {
                let offset = addr.wrapping_sub(PALETTE_BASE) as usize;
                self.palette.get(offset).copied().unwrap_or(0)
            }
            VRAM_BASE..=0x0601_7FFF => {
                let offset = addr.wrapping_sub(VRAM_BASE) as usize;
                self.vram.get(offset).copied().unwrap_or(0)
            }
            OAM_BASE..=0x0700_03FF => {
                let offset = addr.wrapping_sub(OAM_BASE) as usize;
                self.oam.get(offset).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write8(&mut self, addr: u32, value: u8) {
        match addr {
            WRAM_BASE..=0x0203_FFFF => {
                let offset = addr.wrapping_sub(WRAM_BASE) as usize;
                if let Some(slot) = self.wram.get_mut(offset) {
                    *slot = value;
                }
            }
            IO_BASE..=0x0400_03FF => {
                let offset = addr.wrapping_sub(IO_BASE) as usize;
                if let Some(slot) = self.io.get_mut(offset) {
                    *slot = value;
                }
            }
            PALETTE_BASE..=0x0500_03FF => {
                let offset = addr.wrapping_sub(PALETTE_BASE) as usize;
                if let Some(slot) = self.palette.get_mut(offset) {
                    *slot = value;
                }
            }
            VRAM_BASE..=0x0601_7FFF => {
                let offset = addr.wrapping_sub(VRAM_BASE) as usize;
                if let Some(slot) = self.vram.get_mut(offset) {
                    *slot = value;
                }
            }
            OAM_BASE..=0x0700_03FF => {
                let offset = addr.wrapping_sub(OAM_BASE) as usize;
                if let Some(slot) = self.oam.get_mut(offset) {
                    *slot = value;
                }
            }
            _ => {}
        }
    }

    pub fn read16(&self, addr: u32) -> u16 {
        let b0 = self.read8(addr);
        let b1 = self.read8(addr.wrapping_add(1));
        u16::from_le_bytes([b0, b1])
    }

    pub fn write16(&mut self, addr: u32, value: u16) {
        let bytes = value.to_le_bytes();
        self.write8(addr, bytes[0]);
        self.write8(addr.wrapping_add(1), bytes[1]);
    }

    pub fn read32(&self, addr: u32) -> u32 {
        let b0 = self.read8(addr);
        let b1 = self.read8(addr.wrapping_add(1));
        let b2 = self.read8(addr.wrapping_add(2));
        let b3 = self.read8(addr.wrapping_add(3));
        u32::from_le_bytes([b0, b1, b2, b3])
    }

    pub fn write32(&mut self, addr: u32, value: u32) {
        let bytes = value.to_le_bytes();
        self.write8(addr, bytes[0]);
        self.write8(addr.wrapping_add(1), bytes[1]);
        self.write8(addr.wrapping_add(2), bytes[2]);
        self.write8(addr.wrapping_add(3), bytes[3]);
    }
}

#[derive(Debug, Clone)]
struct GbaPpu {
    framebuffer: Vec<u32>,
    line_cycle: u16,
    scanline: u16,
    frames: u64,
}

impl Default for GbaPpu {
    fn default() -> Self {
        Self {
            framebuffer: vec![0xFF00_0000; GBA_WIDTH * GBA_HEIGHT],
            line_cycle: 0,
            scanline: 0,
            frames: 0,
        }
    }
}

impl GbaPpu {
    fn reset(&mut self) {
        self.framebuffer.fill(0xFF00_0000);
        self.line_cycle = 0;
        self.scanline = 0;
        self.frames = 0;
    }

    fn tick(&mut self, bus: &mut GbaBus) {
        self.update_vcount(bus);
        if self.scanline < VISIBLE_LINES
            && self.line_cycle < VISIBLE_DOTS
            && (self.line_cycle % CYCLES_PER_PIXEL) == 0
        {
            let x = (self.line_cycle / CYCLES_PER_PIXEL) as usize;
            let y = self.scanline as usize;
            self.framebuffer[y * GBA_WIDTH + x] = self.render_pixel(bus, x, y);
        }

        self.line_cycle = self.line_cycle.wrapping_add(1);
        if self.line_cycle >= DOTS_PER_LINE {
            self.line_cycle = 0;
            self.scanline = self.scanline.wrapping_add(1);
            if self.scanline >= LINES_PER_FRAME {
                self.scanline = 0;
                self.frames = self.frames.wrapping_add(1);
            }
            self.update_vcount(bus);
        }
    }

    fn render_pixel(&self, bus: &GbaBus, x: usize, y: usize) -> u32 {
        let dispcnt = bus.read16(IO_BASE);
        if (dispcnt & (1 << 7)) != 0 {
            return 0xFFFF_FFFF;
        }
        let mode = dispcnt & 0x0007;
        match mode {
            0 => self.render_mode0(bus, dispcnt, x, y),
            3 => {
                let offset = ((y * GBA_WIDTH) + x) * 2;
                if offset + 1 < bus.vram.len() {
                    let color = u16::from_le_bytes([bus.vram[offset], bus.vram[offset + 1]]);
                    bgr555_to_argb8888(color)
                } else {
                    self.backdrop_color(bus)
                }
            }
            4 => {
                let frame_base = if (dispcnt & (1 << 4)) != 0 { 0xA000 } else { 0 };
                let index_offset = frame_base + (y * GBA_WIDTH + x);
                if index_offset < bus.vram.len() {
                    let palette_index = bus.vram[index_offset] as usize * 2;
                    if palette_index + 1 < bus.palette.len() {
                        let color = u16::from_le_bytes([
                            bus.palette[palette_index],
                            bus.palette[palette_index + 1],
                        ]);
                        bgr555_to_argb8888(color)
                    } else {
                        self.backdrop_color(bus)
                    }
                } else {
                    self.backdrop_color(bus)
                }
            }
            _ => self.backdrop_color(bus),
        }
    }

    fn update_vcount(&self, bus: &mut GbaBus) {
        bus.write16(IO_BASE + 0x0006, self.scanline);
    }

    fn backdrop_color(&self, bus: &GbaBus) -> u32 {
        let color = u16::from_le_bytes([bus.palette[0], bus.palette[1]]);
        bgr555_to_argb8888(color)
    }

    fn render_mode0(&self, bus: &GbaBus, dispcnt: u16, x: usize, y: usize) -> u32 {
        let mut best_color = None;
        let mut best_priority = u16::MAX;
        for bg_index in 0..4u16 {
            if (dispcnt & (1 << (8 + bg_index))) == 0 {
                continue;
            }
            let bgcnt = bus.read16(IO_BASE + 0x0008 + (bg_index * 2) as u32);
            let priority = bgcnt & 0x3;
            if priority > best_priority {
                continue;
            }
            if let Some(color) = self.render_mode0_bg(bus, bg_index as usize, bgcnt, x, y) {
                best_priority = priority;
                best_color = Some(color);
            }
        }

        best_color.unwrap_or_else(|| self.backdrop_color(bus))
    }

    fn render_mode0_bg(
        &self,
        bus: &GbaBus,
        bg_index: usize,
        bgcnt: u16,
        x: usize,
        y: usize,
    ) -> Option<u32> {
        let color_8bpp = (bgcnt & (1 << 7)) != 0;
        let char_base = (((bgcnt >> 2) & 0x3) as usize) * 0x4000;
        let screen_base = (((bgcnt >> 8) & 0x1F) as usize) * 0x800;
        let bg_size = (bgcnt >> 14) & 0x3;
        let (bg_width, bg_height) = match bg_size {
            0 => (256, 256),
            1 => (512, 256),
            2 => (256, 512),
            _ => (512, 512),
        };

        let hofs = bus.read16(IO_BASE + 0x0010 + (bg_index as u32) * 4) as usize;
        let vofs = bus.read16(IO_BASE + 0x0012 + (bg_index as u32) * 4) as usize;
        let sx = (x + hofs) % bg_width;
        let sy = (y + vofs) % bg_height;
        let tile_x = sx / 8;
        let tile_y = sy / 8;
        let tiles_per_row = bg_width / 8;
        let map_index = tile_y * tiles_per_row + tile_x;
        let map_addr = screen_base + (map_index * 2);
        if map_addr + 1 >= bus.vram.len() {
            return None;
        }

        let map_entry = u16::from_le_bytes([bus.vram[map_addr], bus.vram[map_addr + 1]]);
        let tile_index = (map_entry & 0x03FF) as usize;
        let hflip = (map_entry & (1 << 10)) != 0;
        let vflip = (map_entry & (1 << 11)) != 0;
        let palette_bank = ((map_entry >> 12) & 0xF) as usize;

        let mut px = sx % 8;
        let mut py = sy % 8;
        if hflip {
            px = 7 - px;
        }
        if vflip {
            py = 7 - py;
        }

        let palette_index = if color_8bpp {
            let tile_addr = char_base + tile_index * 64 + py * 8 + px;
            if tile_addr >= bus.vram.len() {
                return None;
            }
            bus.vram[tile_addr] as usize
        } else {
            let tile_addr = char_base + tile_index * 32 + py * 4 + (px / 2);
            if tile_addr >= bus.vram.len() {
                return None;
            }
            let packed = bus.vram[tile_addr];
            let index = if (px & 1) == 0 {
                packed & 0x0F
            } else {
                packed >> 4
            } as usize;
            (palette_bank * 16) + index
        };

        if palette_index == 0 {
            return None;
        }
        let palette_addr = palette_index * 2;
        if palette_addr + 1 >= bus.palette.len() {
            return None;
        }
        let color = u16::from_le_bytes([bus.palette[palette_addr], bus.palette[palette_addr + 1]]);
        Some(bgr555_to_argb8888(color))
    }
}

#[derive(Debug, Clone)]
struct GbaApu {
    phase: f32,
    last_sample: i16,
}

impl Default for GbaApu {
    fn default() -> Self {
        Self {
            phase: 0.0,
            last_sample: 0,
        }
    }
}

impl GbaApu {
    fn reset(&mut self) {
        self.phase = 0.0;
        self.last_sample = 0;
    }

    fn tick(&mut self, bus: &GbaBus, input_mix: u32) -> i16 {
        // Keep output silent unless master sound is enabled.
        let soundcnt_x = bus.read16(IO_BASE + 0x0084);
        if (soundcnt_x & 0x0080) == 0 {
            self.last_sample = 0;
            return 0;
        }

        // Bootstrap tone model driven by sound regs; deterministic and controllable.
        let sound1_freq = bus.read16(IO_BASE + 0x0064) & 0x07FF;
        let sound1_env = bus.read16(IO_BASE + 0x0062);
        let base_hz = 131_072.0 / (2048.0 - (sound1_freq as f32).max(1.0));
        let volume = (((sound1_env >> 12) & 0x0F) as f32 / 15.0).max(0.05);
        let mixed_hz = (base_hz + (input_mix as f32 % 53.0)).clamp(32.0, 20_000.0);
        self.phase += mixed_hz / 16_777_216.0;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let amp = (volume * 12_000.0) as i16;
        self.last_sample = if self.phase < 0.5 { amp } else { -amp };
        self.last_sample
    }
}

fn bgr555_to_argb8888(color: u16) -> u32 {
    let r5 = (color & 0x1F) as u32;
    let g5 = ((color >> 5) & 0x1F) as u32;
    let b5 = ((color >> 10) & 0x1F) as u32;
    let r8 = (r5 << 3) | (r5 >> 2);
    let g8 = (g5 << 3) | (g5 >> 2);
    let b8 = (b5 << 3) | (b5 >> 2);
    0xFF00_0000 | (r8 << 16) | (g8 << 8) | b8
}

#[derive(Debug, Clone)]
pub struct GbaCore {
    bus: GbaBus,
    ppu: GbaPpu,
    apu: GbaApu,
    gpr: [u32; 16],
    cpsr: u32,
    boot_thumb: bool,
    input_mix: u32,
    fault: Option<GbaCpuError>,
}

impl Default for GbaCore {
    fn default() -> Self {
        Self {
            bus: GbaBus::with_rom(&[]),
            ppu: GbaPpu::default(),
            apu: GbaApu::default(),
            gpr: [0; 16],
            cpsr: 0x6000_001F,
            boot_thumb: false,
            input_mix: 0,
            fault: None,
        }
    }
}

impl GbaCore {
    pub fn load_rom(&mut self, rom: &[u8]) {
        self.bus = GbaBus::with_rom(rom);
        self.ppu.reset();
        self.apu.reset();
    }

    pub fn regs(&self) -> Registers {
        Registers {
            r0: self.gpr[0],
            r1: self.gpr[1],
            r2: self.gpr[2],
            r3: self.gpr[3],
            pc: self.gpr[15],
            cpsr: self.cpsr,
            thumb: self.thumb_mode(),
        }
    }

    pub fn fault(&self) -> Option<&GbaCpuError> {
        self.fault.as_ref()
    }

    pub fn set_boot_thumb(&mut self, enabled: bool) {
        self.boot_thumb = enabled;
    }

    pub fn frame_buffer_argb(&self) -> &[u32] {
        &self.ppu.framebuffer
    }

    pub fn non_black_pixel_count(&self) -> usize {
        self.ppu
            .framebuffer
            .iter()
            .filter(|pixel| (**pixel & 0x00FF_FFFF) != 0)
            .count()
    }

    fn reset_state(&mut self) {
        self.bus.reset_runtime();
        self.ppu.reset();
        self.apu.reset();
        self.bus.write16(IO_BASE + 0x0130, 0x03FF);
        self.gpr = [0; 16];
        self.gpr[15] = ROM_BASE;
        self.cpsr = 0x6000_001F;
        self.set_thumb_mode(self.boot_thumb);
        self.input_mix = 0;
        self.fault = None;
    }

    fn apply_input_events(&mut self, cycle: u64, input_events: &[InputEvent]) {
        let mut keyinput = self.bus.read16(IO_BASE + 0x0130);
        for event in input_events {
            let mix =
                ((event.port as u32) << 16) | ((event.button as u32) << 8) | event.state as u32;
            self.input_mix = self.input_mix.rotate_left(3) ^ mix ^ cycle as u32;

            let bit = match event.button {
                InputButton::A => 0,
                InputButton::B => 1,
                InputButton::Select => 2,
                InputButton::Start => 3,
                InputButton::Right => 4,
                InputButton::Left => 5,
                InputButton::Up => 6,
                InputButton::Down => 7,
            };

            if matches!(event.state, InputState::Pressed) {
                keyinput &= !(1 << bit);
            } else {
                keyinput |= 1 << bit;
            }
        }
        self.bus.write16(IO_BASE + 0x0130, keyinput);
    }

    fn thumb_mode(&self) -> bool {
        (self.cpsr & CPSR_T_BIT) != 0
    }

    fn set_thumb_mode(&mut self, enabled: bool) {
        if enabled {
            self.cpsr |= CPSR_T_BIT;
        } else {
            self.cpsr &= !CPSR_T_BIT;
        }
    }

    fn set_nz(&mut self, value: u32) {
        if value == 0 {
            self.cpsr |= CPSR_Z_BIT;
        } else {
            self.cpsr &= !CPSR_Z_BIT;
        }

        if (value & 0x8000_0000) != 0 {
            self.cpsr |= CPSR_N_BIT;
        } else {
            self.cpsr &= !CPSR_N_BIT;
        }
    }

    fn condition_holds(&self, cond: u32) -> bool {
        let n = (self.cpsr & CPSR_N_BIT) != 0;
        let z = (self.cpsr & CPSR_Z_BIT) != 0;
        let c = (self.cpsr & CPSR_C_BIT) != 0;
        let v = (self.cpsr & CPSR_V_BIT) != 0;
        match cond {
            0x0 => z,
            0x1 => !z,
            0x2 => c,
            0x3 => !c,
            0x4 => n,
            0x5 => !n,
            0x6 => v,
            0x7 => !v,
            0x8 => c && !z,
            0x9 => !c || z,
            0xA => n == v,
            0xB => n != v,
            0xC => !z && (n == v),
            0xD => z || (n != v),
            0xE => true,
            _ => false,
        }
    }

    fn step(&mut self) -> Result<StepInfo, GbaCpuError> {
        if self.thumb_mode() {
            self.step_thumb()
        } else {
            self.step_arm()
        }
    }

    fn step_arm(&mut self) -> Result<StepInfo, GbaCpuError> {
        let pc = self.gpr[15];
        let opcode = self.bus.read32(pc);
        self.gpr[15] = self.gpr[15].wrapping_add(4);

        let cond = (opcode >> 28) & 0xF;
        if !self.condition_holds(cond) {
            return Ok(StepInfo {
                opcode,
                cycles: 1,
                thumb: false,
            });
        }

        if (opcode & 0x0FFF_FFF0) == 0x012F_FF10 {
            // BX Rm
            let rm = (opcode & 0x0F) as usize;
            let target = self.gpr[rm];
            let thumb = (target & 1) != 0;
            self.set_thumb_mode(thumb);
            self.gpr[15] = if thumb { target & !1 } else { target & !3 };
            return Ok(StepInfo {
                opcode,
                cycles: 3,
                thumb: false,
            });
        }

        if (opcode & 0x0E00_0000) == 0x0A00_0000 {
            // B/BL
            let imm24 = opcode & 0x00FF_FFFF;
            let signed = ((imm24 << 8) as i32) >> 6;
            if (opcode & (1 << 24)) != 0 {
                self.gpr[14] = self.gpr[15];
            }
            self.gpr[15] = (self.gpr[15] as i32).wrapping_add(4 + signed) as u32;
            return Ok(StepInfo {
                opcode,
                cycles: 3,
                thumb: false,
            });
        }

        if (opcode & 0x0C00_0000) == 0x0000_0000 {
            return self.step_arm_data_processing(pc, opcode);
        }

        if (opcode & 0x0F00_0000) == 0x0F00_0000 {
            return self.step_arm_swi(pc, opcode);
        }

        if (opcode & 0x0C00_0000) == 0x0400_0000 {
            return self.step_arm_load_store(pc, opcode);
        }

        if (opcode & 0x0E00_0000) == 0x0800_0000 {
            return self.step_arm_block_transfer(pc, opcode);
        }

        Err(GbaCpuError::UnsupportedArm { opcode, pc })
    }

    fn step_arm_swi(&mut self, pc: u32, opcode: u32) -> Result<StepInfo, GbaCpuError> {
        let swi_raw = opcode & 0x00FF_FFFF;
        let swi = if (swi_raw & 0xFFFF) == 0 {
            (swi_raw >> 16) & 0xFF
        } else {
            swi_raw & 0xFF
        };
        match swi {
            0x00 | 0x01 | 0x02 | 0x03 | 0x04 | 0x05 => {
                // soft reset / ram reset / halt / stop / intrwait stubs
            }
            0x06 | 0x07 => {
                // Div / DivArm
                let numerator = self.gpr[0] as i32;
                let denominator = self.gpr[1] as i32;
                if denominator != 0 {
                    let quotient = numerator.wrapping_div(denominator);
                    let remainder = numerator.wrapping_rem(denominator);
                    self.gpr[0] = quotient as u32;
                    self.gpr[1] = remainder as u32;
                    self.gpr[3] = quotient.unsigned_abs();
                }
            }
            0x08 => {
                // Sqrt
                self.gpr[0] = (self.gpr[0] as f64).sqrt() as u32;
            }
            0x0B => {
                self.cpu_set(false);
            }
            0x0C => {
                self.cpu_set(true);
            }
            0x11 => {
                self.lz77_uncomp_wram()?;
            }
            _ => {
                return Err(GbaCpuError::UnsupportedArm { opcode, pc });
            }
        }

        Ok(StepInfo {
            opcode,
            cycles: 4,
            thumb: false,
        })
    }

    fn step_arm_data_processing(&mut self, pc: u32, opcode: u32) -> Result<StepInfo, GbaCpuError> {
        let immediate = (opcode & (1 << 25)) != 0;
        let op = ((opcode >> 21) & 0xF) as u8;
        let set_flags = (opcode & (1 << 20)) != 0;
        let rn = ((opcode >> 16) & 0xF) as usize;
        let rd = ((opcode >> 12) & 0xF) as usize;

        let operand2 = if immediate {
            let imm8 = opcode & 0xFF;
            let rotate = ((opcode >> 8) & 0xF) * 2;
            imm8.rotate_right(rotate)
        } else {
            if (opcode & 0x0000_0FF0) != 0 {
                return Err(GbaCpuError::UnsupportedArm { opcode, pc });
            }
            let rm = (opcode & 0xF) as usize;
            self.gpr[rm]
        };

        match op {
            0x0 => {
                self.gpr[rd] = self.gpr[rn] & operand2;
                if set_flags {
                    self.set_nz(self.gpr[rd]);
                }
            }
            0x1 => {
                self.gpr[rd] = self.gpr[rn] ^ operand2;
                if set_flags {
                    self.set_nz(self.gpr[rd]);
                }
            }
            0x2 => {
                let (result, borrow) = self.gpr[rn].overflowing_sub(operand2);
                self.gpr[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    if !borrow {
                        self.cpsr |= CPSR_C_BIT;
                    } else {
                        self.cpsr &= !CPSR_C_BIT;
                    }
                    let overflow =
                        ((self.gpr[rn] ^ operand2) & (self.gpr[rn] ^ result) & 0x8000_0000) != 0;
                    if overflow {
                        self.cpsr |= CPSR_V_BIT;
                    } else {
                        self.cpsr &= !CPSR_V_BIT;
                    }
                }
            }
            0x3 => {
                let (result, borrow) = operand2.overflowing_sub(self.gpr[rn]);
                self.gpr[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    if !borrow {
                        self.cpsr |= CPSR_C_BIT;
                    } else {
                        self.cpsr &= !CPSR_C_BIT;
                    }
                }
            }
            0x4 => {
                let (result, carry) = self.gpr[rn].overflowing_add(operand2);
                self.gpr[rd] = result;
                if set_flags {
                    self.set_nz(result);
                    if carry {
                        self.cpsr |= CPSR_C_BIT;
                    } else {
                        self.cpsr &= !CPSR_C_BIT;
                    }
                    let overflow =
                        ((!(self.gpr[rn] ^ operand2)) & (self.gpr[rn] ^ result) & 0x8000_0000) != 0;
                    if overflow {
                        self.cpsr |= CPSR_V_BIT;
                    } else {
                        self.cpsr &= !CPSR_V_BIT;
                    }
                }
            }
            0x8 => {
                let result = self.gpr[rn] & operand2;
                self.set_nz(result);
            }
            0x9 => {
                let result = self.gpr[rn] ^ operand2;
                self.set_nz(result);
            }
            0xA => {
                let (result, borrow) = self.gpr[rn].overflowing_sub(operand2);
                self.set_nz(result);
                if !borrow {
                    self.cpsr |= CPSR_C_BIT;
                } else {
                    self.cpsr &= !CPSR_C_BIT;
                }
            }
            0xB => {
                let (result, carry) = self.gpr[rn].overflowing_add(operand2);
                self.set_nz(result);
                if carry {
                    self.cpsr |= CPSR_C_BIT;
                } else {
                    self.cpsr &= !CPSR_C_BIT;
                }
            }
            0xC => {
                self.gpr[rd] = self.gpr[rn] | operand2;
                if set_flags {
                    self.set_nz(self.gpr[rd]);
                }
            }
            0xD => {
                self.gpr[rd] = operand2;
                if set_flags || immediate {
                    self.set_nz(self.gpr[rd]);
                }
            }
            0xE => {
                self.gpr[rd] = self.gpr[rn] & !operand2;
                if set_flags {
                    self.set_nz(self.gpr[rd]);
                }
            }
            0xF => {
                self.gpr[rd] = !operand2;
                if set_flags {
                    self.set_nz(self.gpr[rd]);
                }
            }
            _ => {
                return Err(GbaCpuError::UnsupportedArm { opcode, pc });
            }
        }

        Ok(StepInfo {
            opcode,
            cycles: 1,
            thumb: false,
        })
    }

    fn step_arm_load_store(&mut self, pc: u32, opcode: u32) -> Result<StepInfo, GbaCpuError> {
        let pre_index = (opcode & (1 << 24)) != 0;
        let add = (opcode & (1 << 23)) != 0;
        let byte = (opcode & (1 << 22)) != 0;
        let writeback = (opcode & (1 << 21)) != 0;
        let load = (opcode & (1 << 20)) != 0;
        let rn = ((opcode >> 16) & 0xF) as usize;
        let rd = ((opcode >> 12) & 0xF) as usize;
        let immediate_offset = (opcode & (1 << 25)) == 0;
        if !immediate_offset {
            return Err(GbaCpuError::UnsupportedArm { opcode, pc });
        }

        let offset = opcode & 0xFFF;
        let base = self.gpr[rn];
        let apply_offset = |addr: u32| -> u32 {
            if add {
                addr.wrapping_add(offset)
            } else {
                addr.wrapping_sub(offset)
            }
        };

        let addr = if pre_index { apply_offset(base) } else { base };

        if load {
            self.gpr[rd] = if byte {
                self.bus.read8(addr) as u32
            } else {
                self.bus.read32(addr)
            };
            if rd == 15 {
                self.gpr[15] &= !1;
                self.set_thumb_mode(false);
            }
        } else {
            let value = if rd == 15 {
                self.gpr[15].wrapping_add(4)
            } else {
                self.gpr[rd]
            };
            if byte {
                self.bus.write8(addr, value as u8);
            } else {
                self.bus.write32(addr, value);
            }
        }

        if !pre_index || writeback {
            self.gpr[rn] = apply_offset(base);
        }

        Ok(StepInfo {
            opcode,
            cycles: 2,
            thumb: false,
        })
    }

    fn step_arm_block_transfer(&mut self, pc: u32, opcode: u32) -> Result<StepInfo, GbaCpuError> {
        let pre = (opcode & (1 << 24)) != 0;
        let up = (opcode & (1 << 23)) != 0;
        let s = (opcode & (1 << 22)) != 0;
        let writeback = (opcode & (1 << 21)) != 0;
        let load = (opcode & (1 << 20)) != 0;
        let rn = ((opcode >> 16) & 0xF) as usize;
        let reg_list = opcode & 0xFFFF;

        if s || reg_list == 0 {
            return Err(GbaCpuError::UnsupportedArm { opcode, pc });
        }

        let reg_count = reg_list.count_ones();
        let transfer_bytes = reg_count * 4;
        let base = self.gpr[rn];
        let start_addr = match (pre, up) {
            (false, true) => base,
            (true, true) => base.wrapping_add(4),
            (false, false) => base.wrapping_sub(transfer_bytes).wrapping_add(4),
            (true, false) => base.wrapping_sub(transfer_bytes),
        };

        let mut addr = start_addr;
        for reg in 0..16usize {
            if (reg_list & (1 << reg)) == 0 {
                continue;
            }

            if load {
                let value = self.bus.read32(addr);
                self.gpr[reg] = value;
                if reg == 15 {
                    self.gpr[15] &= !1;
                    self.set_thumb_mode(false);
                }
            } else {
                let value = if reg == 15 {
                    self.gpr[15].wrapping_add(4)
                } else {
                    self.gpr[reg]
                };
                self.bus.write32(addr, value);
            }
            addr = addr.wrapping_add(4);
        }

        if writeback {
            self.gpr[rn] = if up {
                base.wrapping_add(transfer_bytes)
            } else {
                base.wrapping_sub(transfer_bytes)
            };
        }

        Ok(StepInfo {
            opcode,
            cycles: (1 + reg_count) as u8,
            thumb: false,
        })
    }

    fn step_thumb(&mut self) -> Result<StepInfo, GbaCpuError> {
        let pc = self.gpr[15];
        let opcode = self.bus.read16(pc);
        self.gpr[15] = self.gpr[15].wrapping_add(2);

        if (opcode & 0xF800) == 0x2000 {
            // MOV Rd,#imm8
            let rd = ((opcode >> 8) & 0x7) as usize;
            let imm = (opcode & 0xFF) as u32;
            self.gpr[rd] = imm;
            self.set_nz(self.gpr[rd]);
            return Ok(StepInfo {
                opcode: opcode as u32,
                cycles: 1,
                thumb: true,
            });
        }

        if (opcode & 0xF800) == 0x2800 {
            // CMP Rd,#imm8
            let rd = ((opcode >> 8) & 0x7) as usize;
            let imm = (opcode & 0xFF) as u32;
            let (result, borrow) = self.gpr[rd].overflowing_sub(imm);
            self.set_nz(result);
            if !borrow {
                self.cpsr |= CPSR_C_BIT;
            } else {
                self.cpsr &= !CPSR_C_BIT;
            }
            return Ok(StepInfo {
                opcode: opcode as u32,
                cycles: 1,
                thumb: true,
            });
        }

        if (opcode & 0xF800) == 0x3000 {
            // ADD Rd,#imm8
            let rd = ((opcode >> 8) & 0x7) as usize;
            let imm = (opcode & 0xFF) as u32;
            self.gpr[rd] = self.gpr[rd].wrapping_add(imm);
            self.set_nz(self.gpr[rd]);
            return Ok(StepInfo {
                opcode: opcode as u32,
                cycles: 1,
                thumb: true,
            });
        }

        if (opcode & 0xF800) == 0x3800 {
            // SUB Rd,#imm8
            let rd = ((opcode >> 8) & 0x7) as usize;
            let imm = (opcode & 0xFF) as u32;
            self.gpr[rd] = self.gpr[rd].wrapping_sub(imm);
            self.set_nz(self.gpr[rd]);
            return Ok(StepInfo {
                opcode: opcode as u32,
                cycles: 1,
                thumb: true,
            });
        }

        if (opcode & 0xF800) == 0xE000 {
            // B (unconditional)
            let imm11 = (opcode & 0x07FF) as i16;
            let signed = ((imm11 << 5) >> 4) as i32;
            self.gpr[15] = (self.gpr[15] as i32).wrapping_add(signed) as u32;
            return Ok(StepInfo {
                opcode: opcode as u32,
                cycles: 3,
                thumb: true,
            });
        }

        if (opcode & 0xFF87) == 0x4700 {
            // BX Rm
            let rm = ((opcode >> 3) & 0x0F) as usize;
            let target = self.gpr[rm];
            let thumb = (target & 1) != 0;
            self.set_thumb_mode(thumb);
            self.gpr[15] = if thumb { target & !1 } else { target & !3 };
            return Ok(StepInfo {
                opcode: opcode as u32,
                cycles: 3,
                thumb: true,
            });
        }

        if opcode == 0x46C0 {
            // NOP (MOV r8,r8)
            return Ok(StepInfo {
                opcode: opcode as u32,
                cycles: 1,
                thumb: true,
            });
        }

        if (opcode & 0xFF00) == 0xDF00 {
            // SWI nn in THUMB mode
            let arm_swi = 0xEF00_0000 | (opcode as u32 & 0xFF);
            return self.step_arm_swi(pc, arm_swi);
        }

        Err(GbaCpuError::UnsupportedThumb { opcode, pc })
    }

    fn cpu_set(&mut self, fast: bool) {
        let mut src = self.gpr[0];
        let mut dst = self.gpr[1];
        let cnt = self.gpr[2];
        let fill = (cnt & (1 << 24)) != 0;
        let units = (cnt & 0x1F_FFFF) as usize;
        if units == 0 {
            return;
        }

        let unit_size = if fast || (cnt & (1 << 26)) != 0 { 4 } else { 2 };
        let mut fill_value32 = 0u32;
        let mut fill_value16 = 0u16;
        if fill {
            if unit_size == 4 {
                fill_value32 = self.bus.read32(src);
            } else {
                fill_value16 = self.bus.read16(src);
            }
        }

        for _ in 0..units {
            if unit_size == 4 {
                let value = if fill {
                    fill_value32
                } else {
                    let v = self.bus.read32(src);
                    src = src.wrapping_add(4);
                    v
                };
                self.bus.write32(dst, value);
                dst = dst.wrapping_add(4);
            } else {
                let value = if fill {
                    fill_value16
                } else {
                    let v = self.bus.read16(src);
                    src = src.wrapping_add(2);
                    v
                };
                self.bus.write16(dst, value);
                dst = dst.wrapping_add(2);
            }
        }
    }

    fn lz77_uncomp_wram(&mut self) -> Result<(), GbaCpuError> {
        let src_start = self.gpr[0];
        let dst_start = self.gpr[1];
        let header = self.bus.read32(src_start);
        if (header & 0xFF) != 0x10 {
            // If data is not in LZ77 format under the current memory model, treat as a no-op.
            return Ok(());
        }

        let out_len = (header >> 8) as usize;
        let mut src = src_start.wrapping_add(4);
        let mut produced = 0usize;

        while produced < out_len {
            let flags = self.bus.read8(src);
            src = src.wrapping_add(1);

            for bit in 0..8 {
                if produced >= out_len {
                    break;
                }

                let is_compressed = (flags & (0x80 >> bit)) != 0;
                if !is_compressed {
                    let value = self.bus.read8(src);
                    src = src.wrapping_add(1);
                    self.bus
                        .write8(dst_start.wrapping_add(produced as u32), value);
                    produced += 1;
                    continue;
                }

                let b1 = self.bus.read8(src);
                let b2 = self.bus.read8(src.wrapping_add(1));
                src = src.wrapping_add(2);
                let length = ((b1 >> 4) as usize) + 3;
                let disp = ((((b1 as usize) & 0x0F) << 8) | b2 as usize) + 1;

                for _ in 0..length {
                    if produced >= out_len {
                        break;
                    }
                    let from = produced.saturating_sub(disp);
                    let value = self.bus.read8(dst_start.wrapping_add(from as u32));
                    self.bus
                        .write8(dst_start.wrapping_add(produced as u32), value);
                    produced += 1;
                }
            }
        }

        Ok(())
    }
}

impl DeterministicMachine for GbaCore {
    fn system_id(&self) -> SystemId {
        SystemId::Gba
    }

    fn reset(&mut self) {
        self.reset_state();
    }

    fn tick(&mut self, cycle: u64, input_events: &[InputEvent]) -> (u8, i16) {
        self.apply_input_events(cycle, input_events);

        if self.fault.is_none() {
            if let Err(error) = self.step() {
                self.fault = Some(error);
            }
        }

        self.ppu.tick(&mut self.bus);
        let audio = self.apu.tick(&self.bus, self.input_mix);
        let video_index = (cycle as usize) % self.ppu.framebuffer.len();
        let frame = (self.ppu.framebuffer[video_index] & 0xFF) as u8;
        (frame, audio)
    }
}

#[derive(Debug, Error)]
pub enum GbaRunError {
    #[error("ROM is empty")]
    EmptyRom,
    #[error("{0}")]
    Determinism(#[from] DeterminismError),
    #[error("{0}")]
    Cpu(#[from] GbaCpuError),
}

pub fn run_rom_digest(
    cycles: u64,
    replay: &ReplayLog,
    rom: &[u8],
) -> Result<RunDigest, GbaRunError> {
    if rom.is_empty() {
        return Err(GbaRunError::EmptyRom);
    }
    let mut core = GbaCore::default();
    core.load_rom(rom);
    let digest = run_deterministic(&mut core, cycles, replay)?;
    if let Some(fault) = core.fault() {
        return Err(GbaRunError::Cpu(fault.clone()));
    }
    Ok(digest)
}

pub fn run_rom_digest_with_checkpoint(
    cycles: u64,
    replay: &ReplayLog,
    rom: &[u8],
    checkpoint_cycle: u64,
) -> Result<CheckpointDigest, GbaRunError> {
    if rom.is_empty() {
        return Err(GbaRunError::EmptyRom);
    }
    let mut core = GbaCore::default();
    core.load_rom(rom);
    let result = run_deterministic_with_checkpoint(&mut core, cycles, replay, checkpoint_cycle)?;
    if let Some(fault) = core.fault() {
        return Err(GbaRunError::Cpu(fault.clone()));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aletheia_core::{InputButton, InputState};

    fn replay_fixture() -> ReplayLog {
        ReplayLog::from(vec![
            InputEvent {
                cycle: 2,
                port: 0,
                button: InputButton::A,
                state: InputState::Pressed,
            },
            InputEvent {
                cycle: 3,
                port: 0,
                button: InputButton::A,
                state: InputState::Released,
            },
        ])
    }

    #[test]
    fn run_rom_digest_is_reproducible() {
        let replay = replay_fixture();
        let mut rom = vec![0; 32];
        rom[0..4].copy_from_slice(&0xE3B0_0005u32.to_le_bytes()); // MOVS R0,#5
        rom[4..8].copy_from_slice(&0xE290_1001u32.to_le_bytes()); // ADDS R1,R0,#1
        rom[8..12].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes()); // B .

        let a = run_rom_digest(8, &replay, &rom).expect("run");
        let b = run_rom_digest(8, &replay, &rom).expect("run");
        assert_eq!(a, b);
        assert_eq!(a.system, SystemId::Gba);
    }

    #[test]
    fn arm_data_processing_executes_and_updates_registers() {
        let mut core = GbaCore::default();
        let mut rom = vec![0; 32];
        rom[0..4].copy_from_slice(&0xE3A0_0002u32.to_le_bytes()); // MOV R0,#2
        rom[4..8].copy_from_slice(&0xE280_1003u32.to_le_bytes()); // ADD R1,R0,#3
        rom[8..12].copy_from_slice(&0xE241_2001u32.to_le_bytes()); // SUB R2,R1,#1
        rom[12..16].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes()); // B .
        core.load_rom(&rom);

        run_deterministic(&mut core, 6, &ReplayLog::new()).expect("run");
        let regs = core.regs();
        assert_eq!(regs.r0, 2);
        assert_eq!(regs.r1, 5);
        assert_eq!(regs.r2, 4);
    }

    #[test]
    fn thumb_immediate_ops_execute_when_thumb_mode_set() {
        let mut core = GbaCore::default();
        let mut rom = vec![0; 32];
        // THUMB stream at ROM base
        rom[0..2].copy_from_slice(&0x2001u16.to_le_bytes()); // MOV R0,#1
        rom[2..4].copy_from_slice(&0x3002u16.to_le_bytes()); // ADD R0,#2
        rom[4..6].copy_from_slice(&0x3801u16.to_le_bytes()); // SUB R0,#1
        rom[6..8].copy_from_slice(&0xE7FFu16.to_le_bytes()); // B .
        core.load_rom(&rom);
        core.set_boot_thumb(true);

        run_deterministic(&mut core, 6, &ReplayLog::new()).expect("run");
        let regs = core.regs();
        assert!(regs.thumb);
        assert_eq!(regs.r0, 2);
    }

    #[test]
    fn unsupported_opcode_fails_rom_run() {
        let replay = replay_fixture();
        let mut rom = vec![0; 16];
        rom[0..4].copy_from_slice(&0xE120_0070u32.to_le_bytes()); // BKPT-like unsupported

        let error = run_rom_digest(2, &replay, &rom).expect_err("should fail");
        assert!(matches!(error, GbaRunError::Cpu(_)));
    }

    #[test]
    fn mode3_framebuffer_updates_visible_pixel() {
        let mut core = GbaCore::default();
        core.load_rom(&[0; 4]);
        core.reset_state();
        core.bus.write16(IO_BASE, 0x0003); // mode 3
        core.bus.write16(VRAM_BASE, 0x001F); // red

        for cycle in 0..8u64 {
            let _ = core.tick(cycle, &[]);
        }

        assert_eq!(core.frame_buffer_argb()[0], 0xFFFF_0000);
    }

    #[test]
    fn mode0_bg0_tile_renders_palette_color() {
        let mut core = GbaCore::default();
        core.load_rom(&[0; 4]);
        core.reset_state();
        core.bus.write16(IO_BASE, 0x0100); // mode 0 + BG0 enable
        core.bus.write16(IO_BASE + 0x0008, 0x0100); // BG0CNT: charblock 0/screenblock 1/4bpp
        core.bus.write16(PALETTE_BASE + 0x0002, 0x001F); // palette index 1 -> red
        core.bus.write16(VRAM_BASE + 0x0800, 0x0000); // map entry tile 0
        core.bus.write8(VRAM_BASE + 0x0000, 0x01); // tile pixel (0,0) uses palette index 1

        for cycle in 0..8u64 {
            let _ = core.tick(cycle, &[]);
        }

        assert_eq!(core.frame_buffer_argb()[0], 0xFFFF_0000);
    }

    #[test]
    fn keyinput_register_tracks_press_and_release() {
        let mut core = GbaCore::default();
        core.load_rom(&[0; 4]);
        core.reset_state();
        assert_eq!(core.bus.read16(IO_BASE + 0x0130) & 0x0001, 0x0001);

        let press = [InputEvent {
            cycle: 0,
            port: 0,
            button: InputButton::A,
            state: InputState::Pressed,
        }];
        let release = [InputEvent {
            cycle: 1,
            port: 0,
            button: InputButton::A,
            state: InputState::Released,
        }];

        let _ = core.tick(0, &press);
        assert_eq!(core.bus.read16(IO_BASE + 0x0130) & 0x0001, 0x0000);
        let _ = core.tick(1, &release);
        assert_eq!(core.bus.read16(IO_BASE + 0x0130) & 0x0001, 0x0001);
    }

    #[test]
    fn apu_outputs_non_zero_when_master_enabled() {
        let mut core = GbaCore::default();
        core.load_rom(&[0; 4]);
        core.reset_state();
        core.bus.write16(IO_BASE + 0x0084, 0x0080); // SOUNDCNT_X master enable
        core.bus.write16(IO_BASE + 0x0062, 0xF000); // envelope volume
        core.bus.write16(IO_BASE + 0x0064, 0x0400); // frequency

        let mut observed_non_zero = false;
        for cycle in 0..256u64 {
            let (_, sample) = core.tick(cycle, &[]);
            if sample != 0 {
                observed_non_zero = true;
                break;
            }
        }
        assert!(observed_non_zero);
    }

    #[test]
    fn arm_conditional_instruction_executes_when_flags_match() {
        let mut core = GbaCore::default();
        let mut rom = vec![0; 16];
        rom[0..4].copy_from_slice(&0xE3A0_0001u32.to_le_bytes()); // MOV R0,#1
        rom[4..8].copy_from_slice(&0xE350_0001u32.to_le_bytes()); // CMP R0,#1
        rom[8..12].copy_from_slice(&0x03A0_1007u32.to_le_bytes()); // MOVEQ R1,#7
        rom[12..16].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes()); // B .
        core.load_rom(&rom);

        run_deterministic(&mut core, 8, &ReplayLog::new()).expect("run");
        assert_eq!(core.gpr[1], 7);
    }

    #[test]
    fn arm_bl_sets_link_register_and_branches_with_pipeline_offset() {
        let mut core = GbaCore::default();
        let mut rom = vec![0; 32];
        rom[0..4].copy_from_slice(&0xEB00_0001u32.to_le_bytes()); // BL to 0x10
        rom[4..8].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes()); // B .
        rom[16..20].copy_from_slice(&0xE3A0_0009u32.to_le_bytes()); // MOV R0,#9
        rom[20..24].copy_from_slice(&0xE12F_FF1Eu32.to_le_bytes()); // BX LR
        core.load_rom(&rom);

        run_deterministic(&mut core, 10, &ReplayLog::new()).expect("run");
        assert_eq!(core.gpr[0], 9);
        assert_eq!(core.gpr[14], ROM_BASE + 4);
    }
}
