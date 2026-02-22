# RP6502 Emulator Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a Rust emulator for the RP6502 Picocomputer's RIA and VGA components that replays bus traces and displays Mode 3 bitmap graphics via egui.

**Architecture:** RIA and VGA run as independent threads communicating via typed PIX messages over crossbeam channels. A test harness generates bus traces that drive the RIA. The VGA renders Mode 3 bitmaps into a shared framebuffer displayed by egui. See `docs/plans/2026-02-21-emulator-design.md` for the full design.

**Tech Stack:** Rust, eframe/egui 0.33, crossbeam-channel, bytemuck

**Prerequisite:** Install Rust toolchain with `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` and restart shell.

---

### Task 1: Project Scaffold and Smoke Test

**Files:**
- Create: `emu/Cargo.toml`
- Create: `emu/src/main.rs`

**Step 1: Create the Cargo project**

```bash
cd /home/sprite/rp6502
cargo init emu
```

**Step 2: Set up Cargo.toml with dependencies**

Write `emu/Cargo.toml`:

```toml
[package]
name = "rp6502-emu"
version = "0.1.0"
edition = "2021"

[dependencies]
eframe = { version = "0.33", default-features = false, features = [
    "default_fonts",
    "glow",
    "wayland",
    "x11",
] }
crossbeam-channel = "0.5"
bytemuck = { version = "1", features = ["derive"] }
```

**Step 3: Write minimal egui app that opens a window with a colored rect**

Write `emu/src/main.rs`:

```rust
use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([660.0, 500.0])
            .with_title("RP6502 Emulator"),
        ..Default::default()
    };
    eframe::run_native(
        "rp6502-emu",
        options,
        Box::new(|_cc| Ok(Box::new(EmulatorApp::default()))),
    )
}

#[derive(Default)]
struct EmulatorApp {
    texture: Option<egui::TextureHandle>,
}

impl eframe::App for EmulatorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("RP6502 Emulator");
            // Blue 640x480 test pattern
            let width = 640;
            let height = 480;
            let mut pixels = vec![0u8; width * height * 4];
            for y in 0..height {
                for x in 0..width {
                    let i = (y * width + x) * 4;
                    pixels[i] = (x % 256) as u8;
                    pixels[i + 1] = (y % 256) as u8;
                    pixels[i + 2] = 128;
                    pixels[i + 3] = 255;
                }
            }
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [width, height],
                &pixels,
            );
            match &mut self.texture {
                Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
                None => {
                    self.texture = Some(ctx.load_texture(
                        "screen",
                        image,
                        egui::TextureOptions::NEAREST,
                    ));
                }
            }
            if let Some(tex) = &self.texture {
                ui.add(
                    egui::Image::from_texture(tex)
                        .fit_to_exact_size(egui::vec2(640.0, 480.0))
                );
            }
        });
    }
}
```

**Step 4: Build and run**

Run: `cd /home/sprite/rp6502/emu && cargo run`
Expected: Window opens showing a gradient test pattern.

**Step 5: Commit**

```bash
git add emu/
git commit -m "feat: scaffold emulator project with egui test pattern"
```

---

### Task 2: Core Data Types (pix.rs and bus.rs)

**Files:**
- Create: `emu/src/pix.rs`
- Create: `emu/src/bus.rs`
- Modify: `emu/src/main.rs` (add mod declarations)

**Step 1: Write tests for PIX message packing/unpacking**

Add to `emu/src/pix.rs`:

```rust
/// XRAM broadcast (device 0, channel 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XramWrite {
    pub addr: u16,
    pub data: u8,
}

/// Register write to a PIX device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixRegWrite {
    pub channel: u8,
    pub register: u8,
    pub value: u16,
}

/// Events sent from RIA to VGA over the PIX channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixEvent {
    Xram(XramWrite),
    Reg(PixRegWrite),
    FrameSync,
}

/// Backchannel messages from VGA to RIA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backchannel {
    Vsync(u8),
    Ack,
    Nak,
}

/// Pack a PIX message into the 32-bit hardware format.
/// Format: [31:29]=device, [28]=1 (framing), [27:24]=channel, [23:16]=register, [15:0]=value
pub fn pix_pack(device: u8, channel: u8, register: u8, value: u16) -> u32 {
    0x1000_0000
        | ((device as u32) << 29)
        | ((channel as u32) << 24)
        | ((register as u32) << 16)
        | (value as u32)
}

/// Unpack a 32-bit PIX message. Returns None if framing bit is not set.
pub fn pix_unpack(raw: u32) -> Option<(u8, u8, u8, u16)> {
    if raw & 0x1000_0000 == 0 {
        return None;
    }
    let device = ((raw >> 29) & 0x7) as u8;
    let channel = ((raw >> 24) & 0xF) as u8;
    let register = ((raw >> 16) & 0xFF) as u8;
    let value = (raw & 0xFFFF) as u16;
    Some((device, channel, register, value))
}

/// Pack an XRAM write into PIX format.
/// Matches firmware: PIX_SEND_XRAM(addr, data) = PIX_MESSAGE(0, 0, data, addr)
/// Note: data goes in the register field (bits 23:16), addr in value field (bits 15:0).
pub fn pix_pack_xram(addr: u16, data: u8) -> u32 {
    pix_pack(0, 0, data, addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pix_pack_roundtrip() {
        let raw = pix_pack(1, 0, 0x42, 0x1234);
        let (dev, ch, reg, val) = pix_unpack(raw).unwrap();
        assert_eq!(dev, 1);
        assert_eq!(ch, 0);
        assert_eq!(reg, 0x42);
        assert_eq!(val, 0x1234);
    }

    #[test]
    fn test_pix_pack_vga_canvas() {
        // VGA canvas 640x480 = device 1, channel 0, register 0, value 3
        let raw = pix_pack(1, 0, 0, 3);
        assert_eq!(raw, 0x3000_0003);
    }

    #[test]
    fn test_pix_pack_xram() {
        // XRAM write: addr=0x1234, data=0xAB
        // Matches firmware: PIX_MESSAGE(0, 0, 0xAB, 0x1234)
        let raw = pix_pack_xram(0x1234, 0xAB);
        assert_eq!(raw, 0x10AB_1234);
    }

    #[test]
    fn test_pix_unpack_invalid_framing() {
        assert_eq!(pix_unpack(0x0000_0000), None);
    }

    #[test]
    fn test_pix_pack_idle() {
        // Device 7 idle frame
        let raw = pix_pack(7, 0, 0, 0);
        assert_eq!(raw, 0xF000_0000);
    }
}
```

**Step 2: Write bus.rs**

```rust
/// A single 6502 bus transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusTransaction {
    pub cycle: u64,
    pub addr: u16,
    pub data: u8,
    /// true = read (6502 reading from bus), false = write (6502 writing to bus)
    pub rw: bool,
}

impl BusTransaction {
    pub fn write(cycle: u64, addr: u16, data: u8) -> Self {
        Self { cycle, addr, data, rw: false }
    }

    pub fn read(cycle: u64, addr: u16, data: u8) -> Self {
        Self { cycle, addr, data, rw: true }
    }

    /// Returns true if this transaction targets the RIA register space ($FFE0-$FFFF).
    pub fn hits_ria(&self) -> bool {
        self.addr >= 0xFFE0
    }

    /// Returns the RIA register index (0-31) for this address.
    pub fn ria_reg(&self) -> u8 {
        (self.addr & 0x1F) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hits_ria() {
        assert!(BusTransaction::write(0, 0xFFE4, 0x42).hits_ria());
        assert!(!BusTransaction::write(0, 0x1000, 0x42).hits_ria());
    }

    #[test]
    fn test_ria_reg() {
        assert_eq!(BusTransaction::write(0, 0xFFE4, 0).ria_reg(), 0x04);
        assert_eq!(BusTransaction::write(0, 0xFFFF, 0).ria_reg(), 0x1F);
        assert_eq!(BusTransaction::write(0, 0xFFE0, 0).ria_reg(), 0x00);
    }
}
```

**Step 3: Add mod declarations to main.rs**

Add at top of `emu/src/main.rs`:

```rust
mod bus;
mod pix;
```

**Step 4: Run tests**

Run: `cd /home/sprite/rp6502/emu && cargo test`
Expected: All tests pass.

**Step 5: Commit**

```bash
git add emu/src/pix.rs emu/src/bus.rs emu/src/main.rs
git commit -m "feat: add PIX protocol and bus transaction types with tests"
```

---

### Task 3: VGA Palettes

**Files:**
- Create: `emu/src/vga/mod.rs`
- Create: `emu/src/vga/palette.rs`
- Modify: `emu/src/main.rs` (add mod declaration)

**Reference:** `firmware/src/vga/term/color.c` for exact RGB values.

**Step 1: Write palette module with ANSI 256 color table**

Write `emu/src/vga/palette.rs`. The palette stores colors as RGBA `u32` values (0xRRGGBBAA).

Generate the 256-color ANSI palette matching the firmware exactly:
- Indices 0-15: hardcoded ANSI colors from `color.c` (note: index 0 is transparent/black)
- Indices 16-231: 6x6x6 RGB cube with levels [0, 95, 135, 175, 215, 255]
- Indices 232-255: greyscale ramp [8, 18, 28, ..., 238] in steps of 10

The 2-color palette for 1bpp:
- Index 0: black (0, 0, 0) transparent
- Index 1: light grey (192, 192, 192) opaque

Helper to convert RGB565 to RGBA u32 (for custom palettes stored in XRAM).

```rust
/// Convert RGB8 to RGBA u32 (opaque).
const fn rgba(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | 0xFF
}

/// Convert RGB8 to RGBA u32 (transparent - alpha 0).
const fn rgba_transparent(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8)
}

/// 1bpp default palette. Index 0 = transparent black, index 1 = opaque light grey.
pub const PALETTE_2: [u32; 2] = [
    rgba_transparent(0, 0, 0),
    rgba(192, 192, 192),
];

/// ANSI 256-color palette matching firmware color_256[] in color.c.
/// Index 0 (Black) is transparent. Index 16 (Grey0) is opaque black.
pub const PALETTE_256: [u32; 256] = {
    let mut p = [0u32; 256];

    // 0-15: standard + bright ANSI colors
    // Exact values from firmware color.c
    p[0] = rgba_transparent(0, 0, 0);  // Black (transparent)
    p[1] = rgba(205, 0, 0);            // Red
    p[2] = rgba(0, 205, 0);            // Green
    p[3] = rgba(205, 205, 0);          // Yellow
    p[4] = rgba(0, 0, 238);            // Blue
    p[5] = rgba(205, 0, 205);          // Magenta
    p[6] = rgba(0, 205, 205);          // Cyan
    p[7] = rgba(229, 229, 229);        // White
    p[8] = rgba(127, 127, 127);        // Bright Black
    p[9] = rgba(255, 0, 0);            // Bright Red
    p[10] = rgba(0, 255, 0);           // Bright Green
    p[11] = rgba(255, 255, 0);         // Bright Yellow
    p[12] = rgba(92, 92, 255);         // Bright Blue
    p[13] = rgba(255, 0, 255);         // Bright Magenta
    p[14] = rgba(0, 255, 255);         // Bright Cyan
    p[15] = rgba(255, 255, 255);       // Bright White

    // 16-231: 6x6x6 RGB cube
    // Levels: [0, 95, 135, 175, 215, 255]
    let levels: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let mut i = 16;
    let mut ri = 0;
    while ri < 6 {
        let mut gi = 0;
        while gi < 6 {
            let mut bi = 0;
            while bi < 6 {
                p[i] = rgba(levels[ri], levels[gi], levels[bi]);
                i += 1;
                bi += 1;
            }
            gi += 1;
        }
        ri += 1;
    }

    // 232-255: greyscale ramp (8, 18, 28, ..., 238)
    let mut g = 0u16;
    while g < 24 {
        let v = (8 + g * 10) as u8;
        p[232 + g as usize] = rgba(v, v, v);
        g += 1;
    }

    p
};

/// Convert a 16-bit RGB565 value (as stored in XRAM custom palettes) to RGBA u32.
/// The firmware uses PICO_SCANVIDEO format: bit 15 = alpha, bits 14:10 = R, 9:5 = G, 4:0 = B.
/// Each 5-bit channel is scaled to 8-bit by (val << 3) | (val >> 2).
pub fn rgb565_to_rgba(raw: u16) -> u32 {
    let alpha = if raw & 0x8000 != 0 { 0xFF } else { 0x00 };
    let r5 = ((raw >> 10) & 0x1F) as u8;
    let g5 = ((raw >> 5) & 0x1F) as u8;
    let b5 = (raw & 0x1F) as u8;
    let r = (r5 << 3) | (r5 >> 2);
    let g = (g5 << 3) | (g5 >> 2);
    let b = (b5 << 3) | (b5 >> 2);
    ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | (alpha as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_palette_256_ansi_colors() {
        // Black is transparent
        assert_eq!(PALETTE_256[0] & 0xFF, 0x00);
        // Red
        assert_eq!(PALETTE_256[1], rgba(205, 0, 0));
        // Grey0 (index 16) is opaque black
        assert_eq!(PALETTE_256[16], rgba(0, 0, 0));
        // Bright white
        assert_eq!(PALETTE_256[15], rgba(255, 255, 255));
    }

    #[test]
    fn test_palette_256_rgb_cube() {
        // Index 16 = (0,0,0), index 21 = (0,0,255)
        assert_eq!(PALETTE_256[21], rgba(0, 0, 255));
        // Index 196 = (255,0,0)
        assert_eq!(PALETTE_256[196], rgba(255, 0, 0));
    }

    #[test]
    fn test_palette_256_greyscale() {
        // Index 232 = grey(8)
        assert_eq!(PALETTE_256[232], rgba(8, 8, 8));
        // Index 255 = grey(238)
        assert_eq!(PALETTE_256[255], rgba(238, 238, 238));
    }

    #[test]
    fn test_rgb565_to_rgba_white() {
        // All bits set = alpha + white
        let rgba_val = rgb565_to_rgba(0xFFFF);
        assert_eq!(rgba_val & 0xFF, 0xFF); // alpha
        assert_eq!((rgba_val >> 24) & 0xFF, 0xFF); // R
    }

    #[test]
    fn test_rgb565_to_rgba_transparent() {
        // Bit 15 clear = transparent
        let rgba_val = rgb565_to_rgba(0x7FFF);
        assert_eq!(rgba_val & 0xFF, 0x00); // alpha = 0
    }

    #[test]
    fn test_palette_2() {
        assert_eq!(PALETTE_2[0] & 0xFF, 0x00); // transparent
        assert_eq!(PALETTE_2[1] & 0xFF, 0xFF); // opaque
    }
}
```

**Step 2: Write vga/mod.rs stub**

```rust
pub mod palette;
```

**Step 3: Add mod declaration to main.rs**

Add: `mod vga;`

**Step 4: Run tests**

Run: `cd /home/sprite/rp6502/emu && cargo test`
Expected: All tests pass.

**Step 5: Commit**

```bash
git add emu/src/vga/
git commit -m "feat: add VGA palette module with ANSI 256-color table"
```

---

### Task 4: RIA Core State Machine

**Files:**
- Create: `emu/src/ria.rs`
- Modify: `emu/src/main.rs` (add mod declaration)

**Reference:** `firmware/src/ria/sys/ria.c` act_loop (lines 245-407), `firmware/src/ria/api/api.c` api_run (lines 99-111).

This is the largest task. The RIA processes bus transactions and emits PIX events.

**Step 1: Write RIA struct and constructor with reset defaults**

```rust
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use crate::bus::BusTransaction;
use crate::pix::{Backchannel, PixEvent, PixRegWrite, XramWrite};

const XSTACK_SIZE: usize = 0x200;

pub struct Ria {
    /// Register file: $FFE0-$FFFF mapped to indices 0-31.
    pub regs: [u8; 32],
    /// 64KB extended RAM.
    pub xram: Box<[u8; 65536]>,
    /// 512-byte stack + 1 zero byte for cstring safety.
    pub xstack: [u8; XSTACK_SIZE + 1],
    /// Stack pointer. Starts at XSTACK_SIZE (empty), decrements on push.
    pub xstack_ptr: usize,
    /// IRQ enable register (bit 0 enables VSYNC IRQ).
    pub irq_enabled: u8,
    /// IRQ pin state. true = high (inactive), false = low (asserted).
    pub irq_pin: bool,
    /// Current PHI2 cycle count.
    pub cycle_count: u64,
    /// PHI2 frequency in Hz (default 8 MHz).
    pub phi2_freq: u64,
    /// Cycles per frame (phi2_freq / 60).
    cycles_per_frame: u64,
    /// Cycle count of next frame boundary.
    next_frame_cycle: u64,
    /// Frame counter for VSYNC backchannel.
    frame_count: u8,
    /// PIX transmit channel (RIA -> VGA).
    pix_tx: Sender<PixEvent>,
    /// Backchannel receive (VGA -> RIA).
    backchannel_rx: Receiver<Backchannel>,
    /// Whether the emulator is running.
    pub running: bool,
}

impl Ria {
    pub fn new(
        pix_tx: Sender<PixEvent>,
        backchannel_rx: Receiver<Backchannel>,
    ) -> Self {
        let phi2_freq = 8_000_000;
        let cycles_per_frame = phi2_freq / 60;
        let mut ria = Self {
            regs: [0; 32],
            xram: Box::new([0; 65536]),
            xstack: [0; XSTACK_SIZE + 1],
            xstack_ptr: XSTACK_SIZE,
            irq_enabled: 0,
            irq_pin: true,
            cycle_count: 0,
            phi2_freq,
            cycles_per_frame,
            next_frame_cycle: cycles_per_frame,
            frame_count: 0,
            pix_tx,
            backchannel_rx,
            running: true,
        };
        ria.reset();
        ria
    }

    /// Reset registers to power-on defaults.
    /// Matches api_run() in firmware/src/ria/api/api.c lines 99-111.
    pub fn reset(&mut self) {
        // Zero registers 0-15 ($FFE0-$FFEF), skip register 3 ($FFE3 VSYNC)
        for i in 0..16 {
            if i != 3 {
                self.regs[i] = 0;
            }
        }
        // STEP0 = 1 (signed +1)
        self.regs[0x05] = 1;
        // RW0 = xram[0]
        self.regs[0x04] = self.xram[0];
        // STEP1 = 1 (signed +1)
        self.regs[0x09] = 1;
        // RW1 = xram[0]
        self.regs[0x08] = self.xram[0];
        // Reset xstack
        self.xstack_ptr = XSTACK_SIZE;
        self.irq_enabled = 0;
        self.irq_pin = true;
        self.running = true;
    }

    // --- Register accessors matching firmware macros ---

    fn addr0(&self) -> u16 {
        u16::from_le_bytes([self.regs[0x06], self.regs[0x07]])
    }

    fn set_addr0(&mut self, val: u16) {
        let bytes = val.to_le_bytes();
        self.regs[0x06] = bytes[0];
        self.regs[0x07] = bytes[1];
    }

    fn step0(&self) -> i8 {
        self.regs[0x05] as i8
    }

    fn addr1(&self) -> u16 {
        u16::from_le_bytes([self.regs[0x0A], self.regs[0x0B]])
    }

    fn set_addr1(&mut self, val: u16) {
        let bytes = val.to_le_bytes();
        self.regs[0x0A] = bytes[0];
        self.regs[0x0B] = bytes[1];
    }

    fn step1(&self) -> i8 {
        self.regs[0x09] as i8
    }

    /// Refresh RW0 and RW1 from XRAM.
    /// Matches act_loop lines 249-250: RIA_RW0 = xram[RIA_ADDR0]; RIA_RW1 = xram[RIA_ADDR1];
    fn refresh_rw(&mut self) {
        let addr0 = self.addr0();
        self.regs[0x04] = self.xram[addr0 as usize];
        let addr1 = self.addr1();
        self.regs[0x08] = self.xram[addr1 as usize];
    }

    /// Process a single bus transaction.
    /// Returns the data byte for reads (value placed on data bus).
    pub fn process(&mut self, txn: &BusTransaction) -> u8 {
        self.cycle_count = txn.cycle;

        // Check for frame boundary
        if self.cycle_count >= self.next_frame_cycle {
            self.next_frame_cycle += self.cycles_per_frame;
            let _ = self.pix_tx.send(PixEvent::FrameSync);
            // Process backchannel
            self.poll_backchannel();
        }

        // Refresh RW0/RW1 before processing (matches act_loop continuous refresh)
        self.refresh_rw();

        if !txn.hits_ria() {
            return txn.data;
        }

        if txn.rw {
            self.handle_read(txn)
        } else {
            self.handle_write(txn);
            txn.data
        }
    }

    /// Poll backchannel for VGA responses.
    fn poll_backchannel(&mut self) {
        loop {
            match self.backchannel_rx.try_recv() {
                Ok(Backchannel::Vsync(frame)) => {
                    self.regs[0x03] = frame;
                    if self.irq_enabled & 0x01 != 0 {
                        self.irq_pin = false;
                    }
                }
                Ok(Backchannel::Ack) | Ok(Backchannel::Nak) => {
                    // For MVP, we don't track ack/nak state
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.running = false;
                    break;
                }
            }
        }
    }

    /// Handle a 6502 write to RIA register space.
    /// Matches act_loop switch cases for CASE_WRITE.
    fn handle_write(&mut self, txn: &BusTransaction) {
        let data = txn.data;
        let reg = txn.ria_reg();

        match reg {
            // $FFE1: UART TX
            0x01 => {
                // MVP: just store the byte, no real UART
                self.regs[0x00] |= 0b1000_0000; // TX always ready
            }

            // $FFE4: Write XRAM via portal 0
            0x04 => {
                let addr = self.addr0();
                self.xram[addr as usize] = data;
                let _ = self.pix_tx.send(PixEvent::Xram(XramWrite { addr, data }));
                // Fallthrough: auto-increment addr0
                let new_addr = addr.wrapping_add(self.step0() as i16 as u16);
                self.set_addr0(new_addr);
            }

            // $FFE5: STEP0
            0x05 => {
                self.regs[0x05] = data;
            }

            // $FFE6: ADDR0 low
            0x06 => {
                self.regs[0x06] = data;
            }

            // $FFE7: ADDR0 high
            0x07 => {
                self.regs[0x07] = data;
            }

            // $FFE8: Write XRAM via portal 1
            0x08 => {
                let addr = self.addr1();
                self.xram[addr as usize] = data;
                let _ = self.pix_tx.send(PixEvent::Xram(XramWrite { addr, data }));
                // Fallthrough: auto-increment addr1
                let new_addr = addr.wrapping_add(self.step1() as i16 as u16);
                self.set_addr1(new_addr);
            }

            // $FFE9: STEP1
            0x09 => {
                self.regs[0x09] = data;
            }

            // $FFEA: ADDR1 low
            0x0A => {
                self.regs[0x0A] = data;
            }

            // $FFEB: ADDR1 high
            0x0B => {
                self.regs[0x0B] = data;
            }

            // $FFEC: XSTACK push
            0x0C => {
                if self.xstack_ptr > 0 {
                    self.xstack_ptr -= 1;
                    self.xstack[self.xstack_ptr] = data;
                }
                self.regs[0x0C] = self.xstack[self.xstack_ptr];
            }

            // $FFED: ERRNO low
            0x0D => {
                self.regs[0x0D] = data;
            }

            // $FFEE: ERRNO high
            0x0E => {
                self.regs[0x0E] = data;
            }

            // $FFEF: OP - trigger OS operation
            0x0F => {
                self.regs[0x0F] = data;
                self.handle_op(data);
            }

            // $FFF0: IRQ enable/disable + clear
            0x10 => {
                self.irq_enabled = data;
                self.irq_pin = true; // clear interrupt
            }

            // $FFF4: A register
            0x14 => { self.regs[0x14] = data; }

            // $FFF6: X register
            0x16 => { self.regs[0x16] = data; }

            // $FFF8: SREG low
            0x18 => { self.regs[0x18] = data; }

            // $FFF9: SREG high
            0x19 => { self.regs[0x19] = data; }

            // All other writes: store in register file
            _ => {
                self.regs[reg as usize] = data;
            }
        }
    }

    /// Handle a 6502 read from RIA register space.
    /// Matches act_loop switch cases for CASE_READ.
    fn handle_read(&mut self, txn: &BusTransaction) -> u8 {
        let reg = txn.ria_reg();

        match reg {
            // $FFE0: UART flow control
            0x00 => {
                // MVP: TX always ready, no RX
                self.regs[0x00] |= 0b1000_0000; // TX ready
                self.regs[0x00] &= !0b0100_0000; // no RX data
                self.regs[0x00]
            }

            // $FFE2: UART RX
            0x02 => {
                // MVP: no UART data
                self.regs[0x00] &= !0b0100_0000;
                self.regs[0x02] = 0;
                0
            }

            // $FFE4: Read XRAM via portal 0 (auto-increment after)
            0x04 => {
                let val = self.regs[0x04]; // already refreshed
                let addr = self.addr0();
                let new_addr = addr.wrapping_add(self.step0() as i16 as u16);
                self.set_addr0(new_addr);
                val
            }

            // $FFE8: Read XRAM via portal 1 (auto-increment after)
            0x08 => {
                let val = self.regs[0x08]; // already refreshed
                let addr = self.addr1();
                let new_addr = addr.wrapping_add(self.step1() as i16 as u16);
                self.set_addr1(new_addr);
                val
            }

            // $FFEC: XSTACK pop
            0x0C => {
                if self.xstack_ptr < XSTACK_SIZE {
                    self.xstack_ptr += 1;
                }
                self.regs[0x0C] = self.xstack[self.xstack_ptr];
                self.regs[0x0C]
            }

            // $FFF0: IRQ acknowledge
            0x10 => {
                self.irq_pin = true; // clear interrupt
                self.regs[0x10]
            }

            // All other reads: return current register value
            _ => self.regs[reg as usize],
        }
    }

    /// Handle OS operation triggered by writing to $FFEF.
    fn handle_op(&mut self, op: u8) {
        match op {
            // 0x00: zxstack - zero stack and reset pointer
            0x00 => {
                self.regs[0x0C] = 0; // API_STACK = 0
                self.xstack_ptr = XSTACK_SIZE;
                self.api_return_ax(0);
            }

            // 0x01: xreg - send extended register to PIX device
            0x01 => {
                self.handle_xreg();
            }

            // 0xFF: exit - stop CPU
            0xFF => {
                self.running = false;
            }

            // All others: return ENOSYS (not implemented)
            _ => {
                // Set blocked then return error
                // For MVP, just unblock immediately
                self.api_return_ax(0xFFFF); // -1
            }
        }
    }

    /// Handle xreg OS operation.
    /// Sends accumulated xstack data as PIX register writes.
    /// Reference: firmware/src/ria/sys/pix.c pix_api_xreg() lines 76-184.
    ///
    /// Xstack layout (pushed by 6502, top-down):
    ///   [XSTACK_SIZE-1] = device
    ///   [XSTACK_SIZE-2] = channel
    ///   [XSTACK_SIZE-3] = start_addr
    ///   [XSTACK_SIZE-4..xstack_ptr] = uint16 data values (pairs of bytes)
    ///
    /// Data is sent in REVERSE order: highest address register first, counting down.
    fn handle_xreg(&mut self) {
        if self.xstack_ptr >= XSTACK_SIZE - 3 {
            self.api_return_ax(0xFFFF);
            return;
        }

        let device = self.xstack[XSTACK_SIZE - 1];
        let channel = self.xstack[XSTACK_SIZE - 2];
        let start_addr = self.xstack[XSTACK_SIZE - 3];
        let data_bytes = XSTACK_SIZE - self.xstack_ptr - 3;

        if data_bytes < 2 || data_bytes % 2 != 0 || device > 7 || channel > 15 {
            self.api_return_ax(0xFFFF);
            return;
        }

        let count = data_bytes / 2;

        // Send in reverse order (highest register first, counting down)
        // This matches firmware: pix_send(dev, ch, addr + --count, data)
        for i in (0..count).rev() {
            let offset = self.xstack_ptr + i * 2;
            let value = u16::from_le_bytes([
                self.xstack[offset],
                self.xstack[offset + 1],
            ]);
            let register = start_addr + i as u8;
            let _ = self.pix_tx.send(PixEvent::Reg(PixRegWrite {
                channel,
                register,
                value,
            }));
        }

        self.xstack_ptr = XSTACK_SIZE;
        self.api_return_ax(0);
    }

    /// Set return registers to unblocked state with AX return value.
    /// Matches api_return_ax() in firmware/src/ria/api/api.h.
    fn api_return_ax(&mut self, val: u16) {
        // Released state: NOP, BRA +0, LDA #lo, LDX #hi, RTS
        self.regs[0x10] = 0xEA; // NOP
        self.regs[0x11] = 0x80; // BRA
        self.regs[0x12] = 0x00; // offset 0 (fall through)
        self.regs[0x13] = 0xA9; // LDA #imm
        self.regs[0x14] = (val & 0xFF) as u8; // A
        self.regs[0x15] = 0xA2; // LDX #imm
        self.regs[0x16] = (val >> 8) as u8; // X
        self.regs[0x17] = 0x60; // RTS
        // Update XSTACK register
        self.regs[0x0C] = self.xstack[self.xstack_ptr];
    }
}
```

**Step 2: Write tests for RIA**

Add at bottom of `emu/src/ria.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use crate::bus::BusTransaction;

    fn make_ria() -> (Ria, Receiver<PixEvent>, Sender<Backchannel>) {
        let (pix_tx, pix_rx) = unbounded();
        let (back_tx, back_rx) = unbounded();
        let ria = Ria::new(pix_tx, back_rx);
        (ria, pix_rx, back_tx)
    }

    #[test]
    fn test_reset_defaults() {
        let (ria, _, _) = make_ria();
        assert_eq!(ria.regs[0x05], 1); // STEP0 = 1
        assert_eq!(ria.regs[0x09], 1); // STEP1 = 1
        assert_eq!(ria.xstack_ptr, XSTACK_SIZE);
    }

    #[test]
    fn test_xram_write_portal0() {
        let (mut ria, pix_rx, _) = make_ria();
        // Set ADDR0 to 0x0100
        ria.process(&BusTransaction::write(1, 0xFFE6, 0x00)); // low byte
        ria.process(&BusTransaction::write(2, 0xFFE7, 0x01)); // high byte
        // Write 0x42 to XRAM via RW0
        ria.process(&BusTransaction::write(3, 0xFFE4, 0x42));

        assert_eq!(ria.xram[0x0100], 0x42);
        // ADDR0 should have auto-incremented to 0x0101
        assert_eq!(ria.addr0(), 0x0101);

        // Check PIX event was emitted
        let evt = pix_rx.try_recv().unwrap();
        assert_eq!(evt, PixEvent::Xram(XramWrite { addr: 0x0100, data: 0x42 }));
    }

    #[test]
    fn test_xram_read_portal0_auto_increment() {
        let (mut ria, _, _) = make_ria();
        ria.xram[0x0050] = 0xAB;
        // Set ADDR0 to 0x0050
        ria.process(&BusTransaction::write(1, 0xFFE6, 0x50));
        ria.process(&BusTransaction::write(2, 0xFFE7, 0x00));
        // Read from RW0
        let val = ria.process(&BusTransaction::read(3, 0xFFE4, 0));
        assert_eq!(val, 0xAB);
        // ADDR0 auto-incremented to 0x0051
        assert_eq!(ria.addr0(), 0x0051);
    }

    #[test]
    fn test_xram_step_negative() {
        let (mut ria, _, _) = make_ria();
        // Set STEP0 to -1 (0xFF as i8)
        ria.process(&BusTransaction::write(1, 0xFFE5, 0xFF));
        // Set ADDR0 to 0x0010
        ria.process(&BusTransaction::write(2, 0xFFE6, 0x10));
        ria.process(&BusTransaction::write(3, 0xFFE7, 0x00));
        // Write then check auto-decrement
        ria.process(&BusTransaction::write(4, 0xFFE4, 0x01));
        assert_eq!(ria.addr0(), 0x000F);
    }

    #[test]
    fn test_xstack_push_pop() {
        let (mut ria, _, _) = make_ria();
        // Push 0x42
        ria.process(&BusTransaction::write(1, 0xFFEC, 0x42));
        assert_eq!(ria.xstack_ptr, XSTACK_SIZE - 1);
        assert_eq!(ria.regs[0x0C], 0x42);

        // Push 0x43
        ria.process(&BusTransaction::write(2, 0xFFEC, 0x43));
        assert_eq!(ria.xstack_ptr, XSTACK_SIZE - 2);
        assert_eq!(ria.regs[0x0C], 0x43);

        // Pop (read)
        let val = ria.process(&BusTransaction::read(3, 0xFFEC, 0));
        assert_eq!(val, 0x42); // 0x43 was popped, now TOS is 0x42
        assert_eq!(ria.xstack_ptr, XSTACK_SIZE - 1);
    }

    #[test]
    fn test_op_zxstack() {
        let (mut ria, _, _) = make_ria();
        // Push some data
        ria.process(&BusTransaction::write(1, 0xFFEC, 0x42));
        ria.process(&BusTransaction::write(2, 0xFFEC, 0x43));
        // Trigger zxstack (OP 0x00)
        ria.process(&BusTransaction::write(3, 0xFFEF, 0x00));
        assert_eq!(ria.xstack_ptr, XSTACK_SIZE);
        assert_eq!(ria.regs[0x0C], 0); // API_STACK = 0
    }

    #[test]
    fn test_op_exit() {
        let (mut ria, _, _) = make_ria();
        ria.process(&BusTransaction::write(1, 0xFFEF, 0xFF));
        assert!(!ria.running);
    }

    #[test]
    fn test_irq_enable_and_ack() {
        let (mut ria, _, back_tx) = make_ria();
        // Enable IRQ
        ria.process(&BusTransaction::write(1, 0xFFF0, 0x01));
        assert_eq!(ria.irq_enabled, 0x01);
        assert!(ria.irq_pin); // cleared by write

        // Simulate VSYNC backchannel
        back_tx.send(Backchannel::Vsync(0x81)).unwrap();
        ria.poll_backchannel();
        assert!(!ria.irq_pin); // IRQ asserted

        // Acknowledge by reading $FFF0
        ria.process(&BusTransaction::read(2, 0xFFF0, 0));
        assert!(ria.irq_pin); // cleared
    }

    #[test]
    fn test_vsync_preserved_across_reset() {
        let (mut ria, _, _) = make_ria();
        ria.regs[0x03] = 0x42; // set VSYNC counter
        ria.reset();
        assert_eq!(ria.regs[0x03], 0x42); // preserved
    }
}
```

**Step 3: Add mod declaration to main.rs**

Add: `mod ria;`

**Step 4: Run tests**

Run: `cd /home/sprite/rp6502/emu && cargo test`
Expected: All tests pass.

**Step 5: Commit**

```bash
git add emu/src/ria.rs emu/src/main.rs
git commit -m "feat: implement RIA state machine with register handling and tests"
```

---

### Task 5: Mode 3 Bitmap Renderer

**Files:**
- Create: `emu/src/vga/mode3.rs`
- Modify: `emu/src/vga/mod.rs`

**Reference:** `firmware/src/vga/modes/mode3.c` and `firmware/src/vga/modes/mode3.h`.

**Step 1: Write Mode 3 renderer**

Write `emu/src/vga/mode3.rs`:

```rust
use super::palette::{PALETTE_2, PALETTE_256, rgb565_to_rgba};

/// Mode 3 configuration, read from XRAM at config_ptr.
/// Matches firmware mode3_config_t.
#[derive(Debug, Clone)]
pub struct Mode3Config {
    pub x_wrap: bool,
    pub y_wrap: bool,
    pub x_pos_px: i16,
    pub y_pos_px: i16,
    pub width_px: i16,
    pub height_px: i16,
    pub xram_data_ptr: u16,
    pub xram_palette_ptr: u16,
}

/// Color format attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorFormat {
    Bpp1Msb,  // 0
    Bpp2Msb,  // 1
    Bpp4Msb,  // 2
    Bpp8,     // 3
    Bpp16,    // 4
    Bpp1Lsb,  // 8
    Bpp2Lsb,  // 9
    Bpp4Lsb,  // 10
}

impl ColorFormat {
    pub fn from_attr(attr: u16) -> Option<Self> {
        match attr {
            0 => Some(Self::Bpp1Msb),
            1 => Some(Self::Bpp2Msb),
            2 => Some(Self::Bpp4Msb),
            3 => Some(Self::Bpp8),
            4 => Some(Self::Bpp16),
            8 => Some(Self::Bpp1Lsb),
            9 => Some(Self::Bpp2Lsb),
            10 => Some(Self::Bpp4Lsb),
            _ => None,
        }
    }

    pub fn bits_per_pixel(&self) -> u32 {
        match self {
            Self::Bpp1Msb | Self::Bpp1Lsb => 1,
            Self::Bpp2Msb | Self::Bpp2Lsb => 2,
            Self::Bpp4Msb | Self::Bpp4Lsb => 4,
            Self::Bpp8 => 8,
            Self::Bpp16 => 16,
        }
    }
}

/// A programmed Mode 3 plane.
#[derive(Debug, Clone)]
pub struct Mode3Plane {
    pub config: Mode3Config,
    pub format: ColorFormat,
    pub scanline_begin: u16,
    pub scanline_end: u16,
}

impl Mode3Config {
    /// Read config from XRAM at the given pointer.
    pub fn from_xram(xram: &[u8; 65536], ptr: u16) -> Self {
        let p = ptr as usize;
        Self {
            x_wrap: xram[p] != 0,
            y_wrap: xram[p + 1] != 0,
            x_pos_px: i16::from_le_bytes([xram[p + 2], xram[p + 3]]),
            y_pos_px: i16::from_le_bytes([xram[p + 4], xram[p + 5]]),
            width_px: i16::from_le_bytes([xram[p + 6], xram[p + 7]]),
            height_px: i16::from_le_bytes([xram[p + 8], xram[p + 9]]),
            xram_data_ptr: u16::from_le_bytes([xram[p + 10], xram[p + 11]]),
            xram_palette_ptr: u16::from_le_bytes([xram[p + 12], xram[p + 13]]),
        }
    }
}

/// Resolve palette for a given format from XRAM or built-in.
fn resolve_palette(xram: &[u8; 65536], format: &ColorFormat, palette_ptr: u16) -> Vec<u32> {
    match format {
        ColorFormat::Bpp16 => vec![], // direct color, no palette needed
        ColorFormat::Bpp1Msb | ColorFormat::Bpp1Lsb => {
            // Use built-in 2-color palette (no custom palette support for 1bpp in MVP)
            PALETTE_2.to_vec()
        }
        _ => {
            let count = 1 << format.bits_per_pixel();
            // Check if custom palette pointer is valid (non-zero, fits in XRAM)
            if palette_ptr > 0 && (palette_ptr as usize + count * 2) <= 0x10000 {
                let mut pal = Vec::with_capacity(count);
                for i in 0..count {
                    let offset = palette_ptr as usize + i * 2;
                    let raw = u16::from_le_bytes([xram[offset], xram[offset + 1]]);
                    pal.push(rgb565_to_rgba(raw));
                }
                pal
            } else {
                // Use built-in 256-color palette (truncated to count)
                PALETTE_256[..count].to_vec()
            }
        }
    }
}

/// Extract a pixel index from bitmap data at a given column.
fn get_pixel(data: &[u8], col: usize, format: &ColorFormat) -> u8 {
    match format {
        ColorFormat::Bpp8 => data[col],
        ColorFormat::Bpp4Msb => {
            let byte = data[col / 2];
            if col % 2 == 0 { byte >> 4 } else { byte & 0x0F }
        }
        ColorFormat::Bpp4Lsb => {
            let byte = data[col / 2];
            if col % 2 == 0 { byte & 0x0F } else { byte >> 4 }
        }
        ColorFormat::Bpp2Msb => {
            let byte = data[col / 4];
            let shift = 6 - (col % 4) * 2;
            (byte >> shift) & 0x03
        }
        ColorFormat::Bpp2Lsb => {
            let byte = data[col / 4];
            let shift = (col % 4) * 2;
            (byte >> shift) & 0x03
        }
        ColorFormat::Bpp1Msb => {
            let byte = data[col / 8];
            let shift = 7 - (col % 8);
            (byte >> shift) & 0x01
        }
        ColorFormat::Bpp1Lsb => {
            let byte = data[col / 8];
            let shift = col % 8;
            (byte >> shift) & 0x01
        }
        ColorFormat::Bpp16 => {
            // Not used via get_pixel - handled separately
            0
        }
    }
}

/// Render a Mode 3 plane into the framebuffer.
/// framebuffer is RGBA u32 values, canvas_width x canvas_height.
pub fn render_mode3(
    plane: &Mode3Plane,
    xram: &[u8; 65536],
    framebuffer: &mut [u32],
    canvas_width: u16,
    canvas_height: u16,
) {
    let cfg = &plane.config;
    let bpp = plane.format.bits_per_pixel();
    let sizeof_row = ((cfg.width_px as u32 * bpp + 7) / 8) as usize;
    let palette = resolve_palette(xram, &plane.format, cfg.xram_palette_ptr);

    let y_start = plane.scanline_begin as i32;
    let y_end = if plane.scanline_end == 0 {
        canvas_height as i32
    } else {
        plane.scanline_end as i32
    };

    for scanline in y_start..y_end {
        if scanline < 0 || scanline >= canvas_height as i32 {
            continue;
        }

        let mut row = scanline as i32 - cfg.y_pos_px as i32;

        // Y wrapping
        if cfg.y_wrap && cfg.height_px > 0 {
            row = row.rem_euclid(cfg.height_px as i32);
        }

        if row < 0 || row >= cfg.height_px as i32 {
            // Out of bounds: leave framebuffer pixels unchanged (transparent)
            continue;
        }

        let row_offset = cfg.xram_data_ptr as usize + row as usize * sizeof_row;

        for screen_x in 0..canvas_width as i32 {
            let mut col = screen_x - cfg.x_pos_px as i32;

            // X wrapping
            if cfg.x_wrap && cfg.width_px > 0 {
                col = col.rem_euclid(cfg.width_px as i32);
            }

            if col < 0 || col >= cfg.width_px as i32 {
                continue; // out of bounds, skip
            }

            let fb_idx = scanline as usize * canvas_width as usize + screen_x as usize;

            let rgba = if plane.format == ColorFormat::Bpp16 {
                let byte_offset = row_offset + col as usize * 2;
                if byte_offset + 1 < 0x10000 {
                    let raw = u16::from_le_bytes([
                        xram[byte_offset],
                        xram[byte_offset + 1],
                    ]);
                    rgb565_to_rgba(raw)
                } else {
                    0
                }
            } else {
                let pixel_idx = get_pixel(
                    &xram[row_offset..],
                    col as usize,
                    &plane.format,
                );
                if (pixel_idx as usize) < palette.len() {
                    palette[pixel_idx as usize]
                } else {
                    0
                }
            };

            // Only draw if pixel is opaque (alpha != 0)
            if rgba & 0xFF != 0 {
                framebuffer[fb_idx] = rgba;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_xram_with_config(
        config_ptr: u16,
        data_ptr: u16,
        width: i16,
        height: i16,
    ) -> Box<[u8; 65536]> {
        let mut xram = Box::new([0u8; 65536]);
        let p = config_ptr as usize;
        xram[p] = 0;     // x_wrap
        xram[p + 1] = 0; // y_wrap
        // x_pos = 0, y_pos = 0
        xram[p + 2..p + 4].copy_from_slice(&0i16.to_le_bytes());
        xram[p + 4..p + 6].copy_from_slice(&0i16.to_le_bytes());
        xram[p + 6..p + 8].copy_from_slice(&width.to_le_bytes());
        xram[p + 8..p + 10].copy_from_slice(&height.to_le_bytes());
        xram[p + 10..p + 12].copy_from_slice(&data_ptr.to_le_bytes());
        xram[p + 12..p + 14].copy_from_slice(&0u16.to_le_bytes()); // no custom palette
        xram
    }

    #[test]
    fn test_mode3_8bpp_single_pixel() {
        let config_ptr = 0x0000u16;
        let data_ptr = 0x0100u16;
        let mut xram = make_xram_with_config(config_ptr, data_ptr, 4, 4);

        // Set pixel (0,0) to color index 9 (bright red)
        xram[data_ptr as usize] = 9;

        let plane = Mode3Plane {
            config: Mode3Config::from_xram(&xram, config_ptr),
            format: ColorFormat::Bpp8,
            scanline_begin: 0,
            scanline_end: 4,
        };

        let mut fb = vec![0u32; 4 * 4];
        render_mode3(&plane, &xram, &mut fb, 4, 4);

        // Pixel (0,0) should be bright red
        assert_eq!(fb[0], PALETTE_256[9]);
    }

    #[test]
    fn test_mode3_1bpp_msb() {
        let config_ptr = 0x0000u16;
        let data_ptr = 0x0100u16;
        let mut xram = make_xram_with_config(config_ptr, data_ptr, 8, 1);

        // Byte 0b10100101 -> pixels: 1,0,1,0,0,1,0,1 (MSB first)
        xram[data_ptr as usize] = 0b10100101;

        let plane = Mode3Plane {
            config: Mode3Config::from_xram(&xram, config_ptr),
            format: ColorFormat::Bpp1Msb,
            scanline_begin: 0,
            scanline_end: 1,
        };

        let mut fb = vec![0u32; 8];
        render_mode3(&plane, &xram, &mut fb, 8, 1);

        // Bit 7 = 1 (opaque), bit 6 = 0 (transparent, fb stays 0)
        assert_ne!(fb[0], 0); // pixel 0 = 1
        assert_eq!(fb[1], 0); // pixel 1 = 0
        assert_ne!(fb[2], 0); // pixel 2 = 1
        assert_eq!(fb[3], 0); // pixel 3 = 0
    }

    #[test]
    fn test_mode3_y_wrap() {
        let config_ptr = 0x0000u16;
        let data_ptr = 0x0100u16;
        let mut xram = make_xram_with_config(config_ptr, data_ptr, 1, 2);
        // Enable y_wrap
        xram[config_ptr as usize + 1] = 1;
        // Row 0: color 1, Row 1: color 2
        xram[data_ptr as usize] = 1;
        xram[data_ptr as usize + 1] = 2;

        let plane = Mode3Plane {
            config: Mode3Config::from_xram(&xram, config_ptr),
            format: ColorFormat::Bpp8,
            scanline_begin: 0,
            scanline_end: 4,
        };

        let mut fb = vec![0u32; 4];
        render_mode3(&plane, &xram, &mut fb, 1, 4);

        // Rows 0,2 should be color 1; rows 1,3 should be color 2
        assert_eq!(fb[0], PALETTE_256[1]);
        assert_eq!(fb[1], PALETTE_256[2]);
        assert_eq!(fb[2], PALETTE_256[1]);
        assert_eq!(fb[3], PALETTE_256[2]);
    }
}
```

**Step 2: Update vga/mod.rs**

```rust
pub mod mode3;
pub mod palette;
```

**Step 3: Run tests**

Run: `cd /home/sprite/rp6502/emu && cargo test`
Expected: All tests pass.

**Step 4: Commit**

```bash
git add emu/src/vga/mode3.rs emu/src/vga/mod.rs
git commit -m "feat: implement Mode 3 bitmap renderer with palette support"
```

---

### Task 6: VGA Thread

**Files:**
- Modify: `emu/src/vga/mod.rs`

**Step 1: Implement VGA state machine and thread**

Replace `emu/src/vga/mod.rs`:

```rust
pub mod mode3;
pub mod palette;

use std::sync::{Arc, Mutex};
use crossbeam_channel::{Receiver, Sender};
use crate::pix::{Backchannel, PixEvent, PixRegWrite};
use mode3::{ColorFormat, Mode3Config, Mode3Plane, render_mode3};

/// VGA state machine.
pub struct Vga {
    pub xram: Box<[u8; 65536]>,
    pub planes: [Option<Mode3Plane>; 3],
    pub canvas_width: u16,
    pub canvas_height: u16,
    xregs: [u16; 8],
    pix_rx: Receiver<PixEvent>,
    backchannel_tx: Sender<Backchannel>,
    framebuffer: Arc<Mutex<Vec<u8>>>,
    frame_count: u8,
}

impl Vga {
    pub fn new(
        pix_rx: Receiver<PixEvent>,
        backchannel_tx: Sender<Backchannel>,
        framebuffer: Arc<Mutex<Vec<u8>>>,
    ) -> Self {
        let canvas_width = 640;
        let canvas_height = 480;
        Self {
            xram: Box::new([0; 65536]),
            planes: [None, None, None],
            canvas_width,
            canvas_height,
            xregs: [0; 8],
            pix_rx,
            backchannel_tx,
            framebuffer,
            frame_count: 0,
        }
    }

    /// Run the VGA event loop. Call from a dedicated thread.
    pub fn run(&mut self) {
        loop {
            match self.pix_rx.recv() {
                Ok(event) => self.handle_event(event),
                Err(_) => break, // channel disconnected
            }
        }
    }

    fn handle_event(&mut self, event: PixEvent) {
        match event {
            PixEvent::Xram(write) => {
                self.xram[write.addr as usize] = write.data;
            }
            PixEvent::Reg(reg) => {
                self.handle_reg(reg);
            }
            PixEvent::FrameSync => {
                self.render_frame();
                self.frame_count = self.frame_count.wrapping_add(1);
                let _ = self.backchannel_tx.send(
                    Backchannel::Vsync(0x80 | (self.frame_count & 0x0F))
                );
            }
        }
    }

    /// Handle a PIX register write.
    /// Matches firmware vga/sys/pix.c pix_ch0_xreg().
    fn handle_reg(&mut self, reg: PixRegWrite) {
        if reg.channel == 0 {
            // Accumulate xregs
            if (reg.register as usize) < self.xregs.len() {
                self.xregs[reg.register as usize] = reg.value;
            }

            match reg.register {
                0 => {
                    // CANVAS - configure canvas size
                    match reg.value {
                        1 => { self.canvas_width = 320; self.canvas_height = 240; }
                        2 => { self.canvas_width = 320; self.canvas_height = 180; }
                        3 => { self.canvas_width = 640; self.canvas_height = 480; }
                        4 => { self.canvas_width = 640; self.canvas_height = 360; }
                        _ => { self.canvas_width = 640; self.canvas_height = 480; }
                    }
                    // Reset all planes
                    self.planes = [None, None, None];
                    self.xregs = [0; 8];
                    let _ = self.backchannel_tx.send(Backchannel::Ack);
                }
                1 => {
                    // MODE - program a graphics mode
                    let mode = reg.value;
                    if mode == 3 {
                        self.program_mode3();
                        let _ = self.backchannel_tx.send(Backchannel::Ack);
                    } else {
                        // Only Mode 3 supported in MVP
                        let _ = self.backchannel_tx.send(Backchannel::Nak);
                    }
                    self.xregs = [0; 8];
                }
                _ => {
                    // Registers 2-7: just accumulate, no ack needed
                }
            }
        }
        // Channel 15: display config etc. - ignored in MVP
    }

    /// Program Mode 3 from accumulated xregs.
    fn program_mode3(&mut self) {
        let attr = self.xregs[2];
        let config_ptr = self.xregs[3];
        let plane_idx = self.xregs[4] as usize;
        let scanline_begin = self.xregs[5];
        let scanline_end = self.xregs[6];

        if plane_idx >= 3 {
            return;
        }

        let format = match ColorFormat::from_attr(attr) {
            Some(f) => f,
            None => return,
        };

        let config = Mode3Config::from_xram(&self.xram, config_ptr);

        self.planes[plane_idx] = Some(Mode3Plane {
            config,
            format,
            scanline_begin,
            scanline_end,
        });
    }

    /// Render all planes to the framebuffer.
    fn render_frame(&mut self) {
        let w = self.canvas_width;
        let h = self.canvas_height;
        let pixel_count = w as usize * h as usize;

        // Black background
        let mut fb_rgba = vec![0u32; pixel_count];

        // Render each plane in order (0 = back, 2 = front)
        for plane_opt in &self.planes {
            if let Some(plane) = plane_opt {
                // Re-read config from XRAM each frame (it may have changed)
                let mut current_plane = plane.clone();
                current_plane.config = Mode3Config::from_xram(&self.xram, plane.config.xram_data_ptr);
                // Actually, config_ptr was stored when programmed, we need to keep it.
                // The config is at the xregs[3] address, not the data pointer.
                // Let's just use the plane as-is; config was read at program time.
                render_mode3(plane, &self.xram, &mut fb_rgba, w, h);
            }
        }

        // Convert u32 RGBA to u8 RGBA for egui
        let mut rgba_bytes = vec![0u8; pixel_count * 4];
        for (i, &pixel) in fb_rgba.iter().enumerate() {
            rgba_bytes[i * 4] = (pixel >> 24) as u8;     // R
            rgba_bytes[i * 4 + 1] = (pixel >> 16) as u8; // G
            rgba_bytes[i * 4 + 2] = (pixel >> 8) as u8;  // B
            rgba_bytes[i * 4 + 3] = (pixel & 0xFF) as u8; // A
        }

        // Update shared framebuffer
        if let Ok(mut fb) = self.framebuffer.lock() {
            *fb = rgba_bytes;
        }
    }
}
```

**Step 2: Run tests (existing tests should still pass)**

Run: `cd /home/sprite/rp6502/emu && cargo test`
Expected: All tests pass.

**Step 3: Commit**

```bash
git add emu/src/vga/mod.rs
git commit -m "feat: implement VGA thread with PIX message handling and frame rendering"
```

---

### Task 7: Test Harness

**Files:**
- Create: `emu/src/test_harness.rs`
- Modify: `emu/src/main.rs` (add mod declaration)

**Step 1: Write test harness that generates a bus trace for a colored gradient**

The harness needs to:
1. Write a Mode3Config struct to XRAM
2. Write pixel data to XRAM
3. Push xreg parameters to XSTACK and trigger OP 0x01 to configure VGA

```rust
use crate::bus::BusTransaction;

/// Generate a bus trace that fills a 640x480 8bpp bitmap with a gradient pattern,
/// then configures VGA Mode 3 to display it.
pub fn generate_gradient_trace() -> Vec<BusTransaction> {
    let mut trace = Vec::new();
    let mut cycle: u64 = 0;

    let config_ptr: u16 = 0x0000;
    let data_ptr: u16 = 0x0100;
    let canvas_width: i16 = 640;
    let canvas_height: i16 = 480;

    // --- Step 1: Write Mode3Config to XRAM at config_ptr ---
    // Use portal 0. Set ADDR0 to config_ptr.
    // Write $FFE6 (ADDR0 low), $FFE7 (ADDR0 high)
    trace.push(BusTransaction::write(cycle, 0xFFE6, (config_ptr & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFE7, (config_ptr >> 8) as u8));
    cycle += 1;

    let config_bytes: Vec<u8> = vec![
        0,                                              // x_wrap = false
        0,                                              // y_wrap = false
        0, 0,                                           // x_pos = 0
        0, 0,                                           // y_pos = 0
        (canvas_width & 0xFF) as u8, (canvas_width >> 8) as u8,   // width
        (canvas_height & 0xFF) as u8, (canvas_height >> 8) as u8, // height
        (data_ptr & 0xFF) as u8, (data_ptr >> 8) as u8,           // xram_data_ptr
        0, 0,                                           // xram_palette_ptr (use default)
    ];

    // Write config bytes via RW0 ($FFE4) - auto-increment handles address
    for &b in &config_bytes {
        trace.push(BusTransaction::write(cycle, 0xFFE4, b));
        cycle += 1;
    }

    // --- Step 2: Write pixel data to XRAM at data_ptr ---
    // Set ADDR0 to data_ptr
    trace.push(BusTransaction::write(cycle, 0xFFE6, (data_ptr & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFE7, (data_ptr >> 8) as u8));
    cycle += 1;

    // Generate gradient pattern: color index = (x + y) % 256
    for y in 0..canvas_height as u16 {
        for x in 0..canvas_width as u16 {
            let color = ((x + y) % 256) as u8;
            trace.push(BusTransaction::write(cycle, 0xFFE4, color));
            cycle += 1;
        }
    }

    // --- Step 3: Configure VGA via xreg OP ---
    // Push xreg parameters to XSTACK ($FFEC) in correct order.
    //
    // The xreg API reads from xstack:
    //   [XSTACK_SIZE-1] = device (1 = VGA)
    //   [XSTACK_SIZE-2] = channel (0)
    //   [XSTACK_SIZE-3] = start_addr (0 for CANVAS)
    //   Remaining: uint16 values in order
    //
    // We need to send:
    //   Register 0 (CANVAS): value 3 (640x480)
    //   Register 1 (MODE): value 3 (Mode 3 bitmap)
    //   Register 2: attributes = 3 (8bpp)
    //   Register 3: config_ptr
    //   Register 4: plane = 0
    //   Register 5: scanline_begin = 0
    //   Register 6: scanline_end = 0 (= canvas height)
    //
    // Push order: data values first (in reverse register order for the stack),
    // then start_addr, channel, device.
    // Wait - the firmware sends in reverse, popping from the stack.
    // The 6502 pushes: device, channel, start_addr, then data uint16s in order.
    // Since xstack grows downward, the first push ends up at XSTACK_SIZE-1.

    // Push device = 1 (VGA)
    trace.push(BusTransaction::write(cycle, 0xFFEC, 1));
    cycle += 1;
    // Push channel = 0
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0));
    cycle += 1;
    // Push start_addr = 0
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0));
    cycle += 1;

    // Push data as uint16 LE pairs:
    // Canvas = 3 (640x480)
    let canvas_val: u16 = 3;
    trace.push(BusTransaction::write(cycle, 0xFFEC, (canvas_val & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, (canvas_val >> 8) as u8));
    cycle += 1;

    // Mode = 3
    let mode_val: u16 = 3;
    trace.push(BusTransaction::write(cycle, 0xFFEC, (mode_val & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, (mode_val >> 8) as u8));
    cycle += 1;

    // Attributes = 3 (8bpp)
    let attr_val: u16 = 3;
    trace.push(BusTransaction::write(cycle, 0xFFEC, (attr_val & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, (attr_val >> 8) as u8));
    cycle += 1;

    // Config ptr
    trace.push(BusTransaction::write(cycle, 0xFFEC, (config_ptr & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, (config_ptr >> 8) as u8));
    cycle += 1;

    // Plane = 0
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0));
    cycle += 1;

    // Scanline begin = 0
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0));
    cycle += 1;

    // Scanline end = 0
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0));
    cycle += 1;

    // Trigger xreg operation (OP = 0x01)
    trace.push(BusTransaction::write(cycle, 0xFFEF, 0x01));
    cycle += 1;

    // Let enough cycles pass for a frame to render
    trace.push(BusTransaction::write(cycle + 200_000, 0xFFEF, 0xFF)); // exit

    trace
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gradient_trace_not_empty() {
        let trace = generate_gradient_trace();
        assert!(trace.len() > 100);
        // First transaction should write to ADDR0 low
        assert_eq!(trace[0].addr, 0xFFE6);
    }
}
```

**Step 2: Add mod declaration to main.rs**

Add: `mod test_harness;`

**Step 3: Run tests**

Run: `cd /home/sprite/rp6502/emu && cargo test`
Expected: All tests pass.

**Step 4: Commit**

```bash
git add emu/src/test_harness.rs emu/src/main.rs
git commit -m "feat: add test harness for generating gradient bus trace"
```

---

### Task 8: Wire Everything Together in main.rs

**Files:**
- Modify: `emu/src/main.rs`

**Step 1: Rewrite main.rs to connect RIA, VGA, and egui**

```rust
mod bus;
mod pix;
mod ria;
mod test_harness;
mod vga;

use std::sync::{Arc, Mutex};
use std::thread;
use eframe::egui;
use crate::vga::Vga;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([660.0, 500.0])
            .with_title("RP6502 Emulator"),
        ..Default::default()
    };

    // Shared framebuffer (RGBA bytes)
    let framebuffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(vec![0u8; 640 * 480 * 4]));

    // Channels
    let (pix_tx, pix_rx) = crossbeam_channel::unbounded();
    let (back_tx, back_rx) = crossbeam_channel::unbounded();

    // Spawn VGA thread
    let fb_vga = framebuffer.clone();
    thread::spawn(move || {
        let mut vga = Vga::new(pix_rx, back_tx, fb_vga);
        vga.run();
    });

    // Spawn RIA thread with test harness trace
    thread::spawn(move || {
        let mut ria = ria::Ria::new(pix_tx, back_rx);
        let trace = test_harness::generate_gradient_trace();
        for txn in &trace {
            if !ria.running {
                break;
            }
            ria.process(txn);
        }
    });

    // Run egui
    eframe::run_native(
        "rp6502-emu",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(EmulatorApp {
                framebuffer,
                texture: None,
            }))
        }),
    )
}

struct EmulatorApp {
    framebuffer: Arc<Mutex<Vec<u8>>>,
    texture: Option<egui::TextureHandle>,
}

impl eframe::App for EmulatorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("RP6502 Emulator");

            // Read framebuffer
            let pixels = if let Ok(fb) = self.framebuffer.lock() {
                fb.clone()
            } else {
                vec![0u8; 640 * 480 * 4]
            };

            let image = egui::ColorImage::from_rgba_unmultiplied(
                [640, 480],
                &pixels,
            );

            match &mut self.texture {
                Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
                None => {
                    self.texture = Some(ctx.load_texture(
                        "screen",
                        image,
                        egui::TextureOptions::NEAREST,
                    ));
                }
            }

            if let Some(tex) = &self.texture {
                ui.add(
                    egui::Image::from_texture(tex)
                        .fit_to_exact_size(egui::vec2(640.0, 480.0))
                );
            }
        });

        // Request repaint to update display
        ctx.request_repaint();
    }
}
```

**Step 2: Build and run**

Run: `cd /home/sprite/rp6502/emu && cargo run`
Expected: Window opens showing a colorful gradient pattern rendered through the full RIA -> PIX -> VGA -> egui pipeline.

**Step 3: Run all tests one final time**

Run: `cd /home/sprite/rp6502/emu && cargo test`
Expected: All tests pass.

**Step 4: Commit**

```bash
git add emu/src/main.rs
git commit -m "feat: wire RIA, VGA, and egui together for end-to-end emulation"
```

---

### Task 9: Final Integration Test and Cleanup

**Step 1: Run the full application and verify it works**

Run: `cd /home/sprite/rp6502/emu && cargo run --release`
Expected: Window with gradient, smooth rendering.

**Step 2: Run clippy**

Run: `cd /home/sprite/rp6502/emu && cargo clippy -- -D warnings`
Fix any warnings.

**Step 3: Add emu/ to .gitignore for build artifacts**

Create `/home/sprite/rp6502/emu/.gitignore`:

```
/target
```

**Step 4: Final commit and push**

```bash
git add -A
git commit -m "chore: final cleanup, clippy fixes, gitignore"
git push origin main
```
