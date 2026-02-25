# Mode 2 (Tile) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add Mode 2 (Tile) rendering to the RP6502 emulator, matching firmware behavior.

**Architecture:** New `mode2.rs` module mirroring `mode1.rs` structure. Tile bitmaps use "tall" format (all rows of one tile contiguous) vs Mode 1's "wide" font format. Integration via `Plane::Mode2` in `vga/mod.rs`. Test patterns replicate `pico-examples/src/mode2.c`.

**Tech Stack:** Rust, existing palette/XRAM infrastructure from `vga/palette.rs` and `vga/mod.rs`.

**Reference files:**
- Firmware: `firmware/src/vga/modes/mode2.c` — ground truth for rendering
- Docs: `pico-docs/docs/source/vga.rst` — Mode 2 section
- Example: `pico-examples/src/mode2.c` — test pattern source
- Existing emulator: `emu/src/vga/mode1.rs` — structural template

---

### Task 1: Mode 2 config struct and format enum

**Files:**
- Create: `emu/src/vga/mode2.rs`

**Step 1: Write failing tests for Mode2Config and Mode2Format**

```rust
// At bottom of mode2.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode2_format_from_attr() {
        assert_eq!(Mode2Format::from_attr(0), Some(Mode2Format::Bpp1_8x8));
        assert_eq!(Mode2Format::from_attr(1), Some(Mode2Format::Bpp2_8x8));
        assert_eq!(Mode2Format::from_attr(2), Some(Mode2Format::Bpp4_8x8));
        assert_eq!(Mode2Format::from_attr(3), Some(Mode2Format::Bpp8_8x8));
        assert_eq!(Mode2Format::from_attr(8), Some(Mode2Format::Bpp1_16x16));
        assert_eq!(Mode2Format::from_attr(9), Some(Mode2Format::Bpp2_16x16));
        assert_eq!(Mode2Format::from_attr(10), Some(Mode2Format::Bpp4_16x16));
        assert_eq!(Mode2Format::from_attr(11), Some(Mode2Format::Bpp8_16x16));
        // Invalid attrs
        assert_eq!(Mode2Format::from_attr(4), None);  // no 16bpp tiles
        assert_eq!(Mode2Format::from_attr(5), None);
        assert_eq!(Mode2Format::from_attr(7), None);
        assert_eq!(Mode2Format::from_attr(12), None);
    }

    #[test]
    fn test_mode2_config_from_xram() {
        let mut xram = Box::new([0u8; 65536]);
        let p = 0xFF00usize;
        xram[p] = 1;     // x_wrap
        xram[p + 1] = 0; // y_wrap
        xram[p + 2..p + 4].copy_from_slice(&10i16.to_le_bytes());
        xram[p + 4..p + 6].copy_from_slice(&20i16.to_le_bytes());
        xram[p + 6..p + 8].copy_from_slice(&40i16.to_le_bytes());
        xram[p + 8..p + 10].copy_from_slice(&30i16.to_le_bytes());
        xram[p + 10..p + 12].copy_from_slice(&0x0000u16.to_le_bytes());
        xram[p + 12..p + 14].copy_from_slice(&0xFFFFu16.to_le_bytes());
        xram[p + 14..p + 16].copy_from_slice(&0x1000u16.to_le_bytes());

        let cfg = Mode2Config::from_xram(&xram, 0xFF00);
        assert!(cfg.x_wrap);
        assert!(!cfg.y_wrap);
        assert_eq!(cfg.x_pos_px, 10);
        assert_eq!(cfg.y_pos_px, 20);
        assert_eq!(cfg.width_tiles, 40);
        assert_eq!(cfg.height_tiles, 30);
        assert_eq!(cfg.xram_data_ptr, 0x0000);
        assert_eq!(cfg.xram_palette_ptr, 0xFFFF);
        assert_eq!(cfg.xram_tile_ptr, 0x1000);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p rp6502-emu mode2 -- --nocapture`
Expected: Compilation error — `mode2` module doesn't exist yet.

**Step 3: Implement Mode2Config, Mode2Format, Mode2Plane**

```rust
use super::palette::{resolve_palette, rgb565_to_rgba};

/// Mode 2 configuration, read from XRAM at config_ptr.
/// Matches firmware mode2_config_t exactly (16 bytes):
///   bool x_wrap             (1 byte, offset 0)
///   bool y_wrap             (1 byte, offset 1)
///   int16_t x_pos_px        (2 bytes, offset 2)
///   int16_t y_pos_px        (2 bytes, offset 4)
///   int16_t width_tiles     (2 bytes, offset 6)
///   int16_t height_tiles    (2 bytes, offset 8)
///   uint16_t xram_data_ptr  (2 bytes, offset 10)
///   uint16_t xram_palette_ptr (2 bytes, offset 12)
///   uint16_t xram_tile_ptr  (2 bytes, offset 14)
#[derive(Debug, Clone)]
pub struct Mode2Config {
    pub x_wrap: bool,
    pub y_wrap: bool,
    pub x_pos_px: i16,
    pub y_pos_px: i16,
    pub width_tiles: i16,
    pub height_tiles: i16,
    pub xram_data_ptr: u16,
    pub xram_palette_ptr: u16,
    pub xram_tile_ptr: u16,
}

/// Mode 2 format, encoding both tile size and color depth.
/// Matches firmware mode2_prog() attribute switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode2Format {
    Bpp1_8x8,    // attr 0
    Bpp2_8x8,    // attr 1
    Bpp4_8x8,    // attr 2
    Bpp8_8x8,    // attr 3
    Bpp1_16x16,  // attr 8
    Bpp2_16x16,  // attr 9
    Bpp4_16x16,  // attr 10
    Bpp8_16x16,  // attr 11
}

impl Mode2Format {
    pub fn from_attr(attr: u16) -> Option<Self> {
        match attr {
            0 => Some(Self::Bpp1_8x8),
            1 => Some(Self::Bpp2_8x8),
            2 => Some(Self::Bpp4_8x8),
            3 => Some(Self::Bpp8_8x8),
            8 => Some(Self::Bpp1_16x16),
            9 => Some(Self::Bpp2_16x16),
            10 => Some(Self::Bpp4_16x16),
            11 => Some(Self::Bpp8_16x16),
            _ => None,
        }
    }

    /// Tile size in pixels (8 or 16).
    pub fn tile_size(&self) -> i16 {
        match self {
            Self::Bpp1_8x8 | Self::Bpp2_8x8 | Self::Bpp4_8x8 | Self::Bpp8_8x8 => 8,
            _ => 16,
        }
    }

    /// Bits per pixel.
    pub fn bpp(&self) -> u32 {
        match self {
            Self::Bpp1_8x8 | Self::Bpp1_16x16 => 1,
            Self::Bpp2_8x8 | Self::Bpp2_16x16 => 2,
            Self::Bpp4_8x8 | Self::Bpp4_16x16 => 4,
            Self::Bpp8_8x8 | Self::Bpp8_16x16 => 8,
        }
    }

    /// Bytes per row within a single tile's bitmap.
    /// 8x8: bpp bytes; 16x16: 2*bpp bytes.
    pub fn row_size(&self) -> usize {
        let bpp = self.bpp() as usize;
        if self.tile_size() == 8 { bpp } else { 2 * bpp }
    }

    /// Total bytes per tile = row_size * tile_size.
    pub fn tile_bytes(&self) -> usize {
        self.row_size() * self.tile_size() as usize
    }
}

/// A programmed Mode 2 plane.
#[derive(Debug, Clone)]
pub struct Mode2Plane {
    pub config: Mode2Config,
    pub format: Mode2Format,
    pub scanline_begin: u16,
    pub scanline_end: u16,
    pub config_ptr: u16,
}

impl Mode2Config {
    pub fn from_xram(xram: &[u8; 65536], ptr: u16) -> Self {
        let p = ptr as usize;
        if p + 16 > 65536 {
            return Self {
                x_wrap: false, y_wrap: false,
                x_pos_px: 0, y_pos_px: 0,
                width_tiles: 0, height_tiles: 0,
                xram_data_ptr: 0, xram_palette_ptr: 0, xram_tile_ptr: 0,
            };
        }
        Self {
            x_wrap: xram[p] != 0,
            y_wrap: xram[p + 1] != 0,
            x_pos_px: i16::from_le_bytes([xram[p + 2], xram[p + 3]]),
            y_pos_px: i16::from_le_bytes([xram[p + 4], xram[p + 5]]),
            width_tiles: i16::from_le_bytes([xram[p + 6], xram[p + 7]]),
            height_tiles: i16::from_le_bytes([xram[p + 8], xram[p + 9]]),
            xram_data_ptr: u16::from_le_bytes([xram[p + 10], xram[p + 11]]),
            xram_palette_ptr: u16::from_le_bytes([xram[p + 12], xram[p + 13]]),
            xram_tile_ptr: u16::from_le_bytes([xram[p + 14], xram[p + 15]]),
        }
    }
}
```

Also add `pub mod mode2;` to `emu/src/vga/mod.rs` (after the `mode1` line).

**Step 4: Run tests to verify they pass**

Run: `cargo test -p rp6502-emu mode2 -- --nocapture`
Expected: 2 tests pass.

**Step 5: Commit**

```bash
git add emu/src/vga/mode2.rs emu/src/vga/mod.rs
git commit -m "feat: add Mode 2 config struct and format enum"
```

---

### Task 2: Mode 2 tile renderer

**Files:**
- Modify: `emu/src/vga/mode2.rs`

**Step 1: Write failing test for 1bpp 8x8 tile rendering**

Add to `mode2.rs` tests module:

```rust
    fn make_mode2_xram(
        config_ptr: u16,
        data_ptr: u16,
        tile_ptr: u16,
        width_tiles: i16,
        height_tiles: i16,
    ) -> Box<[u8; 65536]> {
        let mut xram = Box::new([0u8; 65536]);
        let p = config_ptr as usize;
        xram[p] = 0;     // x_wrap
        xram[p + 1] = 0; // y_wrap
        xram[p + 2..p + 4].copy_from_slice(&0i16.to_le_bytes());
        xram[p + 4..p + 6].copy_from_slice(&0i16.to_le_bytes());
        xram[p + 6..p + 8].copy_from_slice(&width_tiles.to_le_bytes());
        xram[p + 8..p + 10].copy_from_slice(&height_tiles.to_le_bytes());
        xram[p + 10..p + 12].copy_from_slice(&data_ptr.to_le_bytes());
        xram[p + 12..p + 14].copy_from_slice(&0xFFFFu16.to_le_bytes()); // built-in palette
        xram[p + 14..p + 16].copy_from_slice(&tile_ptr.to_le_bytes());
        xram
    }

    #[test]
    fn test_mode2_1bpp_8x8_solid_tile() {
        let config_ptr = 0xFF00u16;
        let data_ptr = 0x0000u16;
        let tile_ptr = 0x1000u16;
        let mut xram = make_mode2_xram(config_ptr, data_ptr, tile_ptr, 1, 1);

        // Tile 0: all 0xFF (solid) — 8 bytes, 1 byte/row, 8 rows
        for row in 0..8 {
            xram[tile_ptr as usize + row] = 0xFF;
        }
        // Tile map: single cell = tile 0
        xram[data_ptr as usize] = 0;

        let plane = Mode2Plane {
            config: Mode2Config::from_xram(&xram, config_ptr),
            format: Mode2Format::Bpp1_8x8,
            scanline_begin: 0,
            scanline_end: 8,
            config_ptr,
        };

        let mut fb = vec![0u32; 8 * 8];
        render_mode2(&plane, &xram, &mut fb, 8, 8);

        // Every pixel should be opaque (palette[1])
        for y in 0..8 {
            for x in 0..8 {
                assert_ne!(fb[y * 8 + x] & 0xFF, 0,
                    "pixel ({x},{y}) should be opaque");
            }
        }
    }

    #[test]
    fn test_mode2_1bpp_8x8_two_tiles() {
        let config_ptr = 0xFF00u16;
        let data_ptr = 0x0000u16;
        let tile_ptr = 0x1000u16;
        // 2 tiles wide, 1 tile tall = 16x8 pixels
        let mut xram = make_mode2_xram(config_ptr, data_ptr, tile_ptr, 2, 1);

        // Tile 0: all 0x00 (empty/transparent)
        // Tile 1: all 0xFF (solid)
        for row in 0..8 {
            xram[tile_ptr as usize + row] = 0x00;         // tile 0
            xram[tile_ptr as usize + 8 + row] = 0xFF;     // tile 1
        }
        // Tile map: [0, 1]
        xram[data_ptr as usize] = 0;
        xram[data_ptr as usize + 1] = 1;

        let plane = Mode2Plane {
            config: Mode2Config::from_xram(&xram, config_ptr),
            format: Mode2Format::Bpp1_8x8,
            scanline_begin: 0,
            scanline_end: 8,
            config_ptr,
        };

        let mut fb = vec![0u32; 16 * 8];
        render_mode2(&plane, &xram, &mut fb, 16, 8);

        // Left 8 pixels (tile 0) should be transparent
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(fb[y * 16 + x], 0,
                    "left tile pixel ({x},{y}) should be transparent");
            }
        }
        // Right 8 pixels (tile 1) should be opaque
        for y in 0..8 {
            for x in 8..16 {
                assert_ne!(fb[y * 16 + x] & 0xFF, 0,
                    "right tile pixel ({x},{y}) should be opaque");
            }
        }
    }

    #[test]
    fn test_mode2_8bpp_8x8() {
        let config_ptr = 0xFF00u16;
        let data_ptr = 0x0000u16;
        let tile_ptr = 0x1000u16;
        let mut xram = make_mode2_xram(config_ptr, data_ptr, tile_ptr, 1, 1);

        // Tile 0 at 8bpp 8x8: 8 bytes/row * 8 rows = 64 bytes
        // Fill every pixel with palette index 9 (bright red)
        for i in 0..64 {
            xram[tile_ptr as usize + i] = 9;
        }
        xram[data_ptr as usize] = 0;

        let plane = Mode2Plane {
            config: Mode2Config::from_xram(&xram, config_ptr),
            format: Mode2Format::Bpp8_8x8,
            scanline_begin: 0,
            scanline_end: 8,
            config_ptr,
        };

        let mut fb = vec![0u32; 8 * 8];
        render_mode2(&plane, &xram, &mut fb, 8, 8);

        use crate::vga::palette::PALETTE_256;
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(fb[y * 8 + x], PALETTE_256[9],
                    "pixel ({x},{y}) should be bright red");
            }
        }
    }

    #[test]
    fn test_mode2_y_wrap() {
        let config_ptr = 0xFF00u16;
        let data_ptr = 0x0000u16;
        let tile_ptr = 0x1000u16;
        let mut xram = make_mode2_xram(config_ptr, data_ptr, tile_ptr, 1, 1);
        // Enable y_wrap
        xram[config_ptr as usize + 1] = 1;

        // Tile 0: all solid
        for row in 0..8 {
            xram[tile_ptr as usize + row] = 0xFF;
        }
        xram[data_ptr as usize] = 0;

        let plane = Mode2Plane {
            config: Mode2Config::from_xram(&xram, config_ptr),
            format: Mode2Format::Bpp1_8x8,
            scanline_begin: 0,
            scanline_end: 16,  // 16 scanlines, 1 tile tall (8px) -> wraps
            config_ptr,
        };

        let mut fb = vec![0u32; 8 * 16];
        render_mode2(&plane, &xram, &mut fb, 8, 16);

        // Row 0 and row 8 should both have content (wrapped)
        assert_ne!(fb[0] & 0xFF, 0, "row 0 should have content");
        assert_ne!(fb[8 * 8] & 0xFF, 0, "row 8 should wrap and have content");
    }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p rp6502-emu mode2 -- --nocapture`
Expected: FAIL — `render_mode2` not defined.

**Step 3: Implement render_mode2**

Add to `mode2.rs` before the tests module. The renderer follows the same pattern as `render_mode1` but uses the "tall" tile bitmap layout instead of "wide" font layout.

Key differences from Mode 1:
- Tile map cells are always 1 byte (tile_id), regardless of bpp
- Tile lookup: `tile_ptr + tile_id * tile_bytes + within_row * row_size + byte_col`
- Pixel extraction from tile byte uses MSB-first packing (same as Mode 3 default)
- No per-cell color data — color comes from the tile bitmap through palette lookup

```rust
/// Extract a pixel's palette index from a tile bitmap byte.
/// Packing is MSB-first, matching firmware mode2 render functions.
fn get_tile_pixel(tile_byte: u8, pixel_in_byte: usize, bpp: u32) -> u8 {
    match bpp {
        1 => (tile_byte >> (7 - pixel_in_byte)) & 1,
        2 => (tile_byte >> (6 - pixel_in_byte * 2)) & 0x03,
        4 => if pixel_in_byte == 0 { tile_byte >> 4 } else { tile_byte & 0x0F },
        8 => tile_byte,
        _ => 0,
    }
}

/// Render a Mode 2 plane into the framebuffer.
///
/// Pixels are only written when alpha is non-zero (opaque).
pub fn render_mode2(
    plane: &Mode2Plane,
    xram: &[u8; 65536],
    framebuffer: &mut [u32],
    canvas_width: u16,
    canvas_height: u16,
) {
    let cfg = &plane.config;
    let tile_size = plane.format.tile_size();
    let bpp = plane.format.bpp();
    let row_size = plane.format.row_size();
    let tile_bytes = plane.format.tile_bytes();
    let pixels_per_byte = 8 / bpp as usize;

    if cfg.width_tiles < 1 || cfg.height_tiles < 1 {
        return;
    }

    let height_px = cfg.height_tiles as i32 * tile_size as i32;
    let width_px = cfg.width_tiles as i32 * tile_size as i32;

    // Bounds check: tile map must fit in XRAM
    let sizeof_tilemap = cfg.height_tiles as usize * cfg.width_tiles as usize;
    if sizeof_tilemap > 0x10000usize.saturating_sub(cfg.xram_data_ptr as usize) {
        return;
    }

    let palette = resolve_palette(xram, bpp, cfg.xram_palette_ptr);

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

        let mut row = scanline - cfg.y_pos_px as i32;

        if cfg.y_wrap {
            row = row.rem_euclid(height_px);
        }

        if row < 0 || row >= height_px {
            continue;
        }

        let tile_row = row / tile_size as i32;
        let within_tile_row = row & (tile_size as i32 - 1);

        for screen_x in 0..canvas_width as i32 {
            let mut col = screen_x - cfg.x_pos_px as i32;

            if cfg.x_wrap {
                col = col.rem_euclid(width_px);
            }

            if col < 0 || col >= width_px {
                continue;
            }

            let tile_col = col / tile_size as i32;

            // Look up tile ID from tile map
            let map_offset = cfg.xram_data_ptr as usize
                + tile_row as usize * cfg.width_tiles as usize
                + tile_col as usize;
            if map_offset >= 0x10000 {
                continue;
            }
            let tile_id = xram[map_offset] as usize;

            // Look up pixel in tile bitmap (tall format)
            let pixel_in_tile_col = col & (tile_size as i32 - 1);
            let byte_col = pixel_in_tile_col as usize / pixels_per_byte;
            let pixel_in_byte = pixel_in_tile_col as usize % pixels_per_byte;

            let tile_addr = cfg.xram_tile_ptr as usize
                + tile_id * tile_bytes
                + within_tile_row as usize * row_size
                + byte_col;

            if tile_addr >= 0x10000 {
                continue;
            }

            let tile_byte = xram[tile_addr];
            let pixel_idx = get_tile_pixel(tile_byte, pixel_in_byte, bpp);

            let rgba = if (pixel_idx as usize) < palette.len() {
                palette[pixel_idx as usize]
            } else {
                0
            };

            if rgba & 0xFF != 0 {
                let fb_idx = scanline as usize * canvas_width as usize + screen_x as usize;
                framebuffer[fb_idx] = rgba;
            }
        }
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p rp6502-emu mode2 -- --nocapture`
Expected: All 6 tests pass.

**Step 5: Commit**

```bash
git add emu/src/vga/mode2.rs
git commit -m "feat: add Mode 2 tile renderer"
```

---

### Task 3: Wire Mode 2 into VGA state machine

**Files:**
- Modify: `emu/src/vga/mod.rs`

**Step 1: Add Mode 2 to Plane enum and handle_reg dispatch**

Add import at top of `mod.rs`:
```rust
use mode2::{Mode2Config, Mode2Format, Mode2Plane, render_mode2};
```

Add variant to `Plane` enum:
```rust
pub enum Plane {
    Mode1(Mode1Plane),
    Mode2(Mode2Plane),
    Mode3(Mode3Plane),
}
```

Add `program_mode2` method (mirrors `program_mode1` exactly, just different types):
```rust
    fn program_mode2(&mut self) {
        let attr = self.xregs[2];
        let config_ptr = self.xregs[3];
        let plane_idx = self.xregs[4] as usize;
        let scanline_begin = self.xregs[5];
        let scanline_end = self.xregs[6];

        if plane_idx >= 3 || config_ptr & 1 != 0 {
            return;
        }

        if config_ptr as usize + 16 > 0x10000 {
            return;
        }

        let format = match Mode2Format::from_attr(attr) {
            Some(f) => f,
            None => return,
        };

        let config = Mode2Config::from_xram(&self.xram, config_ptr);

        self.planes[plane_idx] = Some(Plane::Mode2(Mode2Plane {
            config,
            format,
            scanline_begin,
            scanline_end,
            config_ptr,
        }));
    }
```

Add mode 2 to `handle_reg` match:
```rust
                    match mode {
                        1 => {
                            self.program_mode1();
                            let _ = self.backchannel_tx.send(Backchannel::Ack);
                        }
                        2 => {
                            self.program_mode2();
                            let _ = self.backchannel_tx.send(Backchannel::Ack);
                        }
                        3 => {
                            self.program_mode3();
                            let _ = self.backchannel_tx.send(Backchannel::Ack);
                        }
                        _ => {
                            let _ = self.backchannel_tx.send(Backchannel::Nak);
                        }
                    }
```

Add Mode 2 to `render_frame`:
```rust
                Plane::Mode2(p) => {
                    let fresh_config = Mode2Config::from_xram(&self.xram, p.config_ptr);
                    let current_plane = Mode2Plane { config: fresh_config, ..p.clone() };
                    render_mode2(&current_plane, &self.xram, &mut self.canvas_buf[..pixel_count], w, h);
                }
```

**Step 2: Run existing tests to verify nothing broke**

Run: `cargo test -p rp6502-emu`
Expected: All tests pass (existing + new Mode 2 tests).

**Step 3: Commit**

```bash
git add emu/src/vga/mod.rs
git commit -m "feat: wire Mode 2 into VGA state machine"
```

---

### Task 4: Add TraceBuilder config offset constants

**Files:**
- Modify: `emu/src/ria_api.rs`

**Step 1: Add vga_mode2_config_t offset module**

Add after the `vga_mode1_config_t` module:

```rust
/// Field offsets for `vga_mode2_config_t` from `cc65/include/rp6502.h`.
#[allow(non_upper_case_globals)]
pub mod vga_mode2_config_t {
    pub const X_WRAP: u16 = 0;
    pub const Y_WRAP: u16 = 1;
    pub const X_POS_PX: u16 = 2;
    pub const Y_POS_PX: u16 = 4;
    pub const WIDTH_TILES: u16 = 6;
    pub const HEIGHT_TILES: u16 = 8;
    pub const XRAM_DATA_PTR: u16 = 10;
    pub const XRAM_PALETTE_PTR: u16 = 12;
    pub const XRAM_TILE_PTR: u16 = 14;
}
```

**Step 2: Run tests**

Run: `cargo test -p rp6502-emu ria_api`
Expected: All pass.

**Step 3: Commit**

```bash
git add emu/src/ria_api.rs
git commit -m "feat: add Mode 2 config offset constants"
```

---

### Task 5: Add Mode 2 test patterns and CLI modes

**Files:**
- Modify: `emu/src/test_harness.rs`
- Modify: `emu/src/main.rs` (update CLAUDE.md valid mode list if needed)

**Step 1: Add TestMode variants for Mode 2**

Add to `TestMode` enum:
```rust
    /// 320x240 canvas, Mode 2, 1bpp 8x8 tiles (40x30 tile map, 2 tiles)
    Tile1bpp8x8,
    /// 320x240 canvas, Mode 2, 1bpp 16x16 tiles (20x15 tile map, 2 tiles)
    Tile1bpp16x16,
```

Add Display, FromStr, and all() entries for both. String names: `"tile1bpp8x8"`, `"tile1bpp16x16"`.

**Step 2: Write test trace generator**

Add `generate_mode2_test_trace` function. This replicates the `pico-examples/src/mode2.c` setup:
- 320x240 canvas (canvas reg 1)
- Config at 0xFF00, tile map at 0x0000, tile bitmaps at 0x1000
- x_wrap=true, y_wrap=true, width_tiles=40, height_tiles=30
- For 1bpp 8x8: 2 tiles (diagonal stripes, 8 bytes each), random tile map
- For 1bpp 16x16: 2 tiles (X pattern and diamond, 32 bytes each), random tile map

```rust
fn generate_mode2_test_trace(mode: TestMode) -> Vec<BusTransaction> {
    let mut tb = TraceBuilder::new();
    let config_ptr: u16 = 0xFF00;
    let data_ptr: u16 = 0x0000;
    let tile_ptr: u16 = 0x1000;

    let (width_tiles, height_tiles, attr, tile_size): (i16, i16, u16, usize) = match mode {
        TestMode::Tile1bpp8x8 => (40, 30, 0, 8),
        TestMode::Tile1bpp16x16 => (20, 15, 8, 16),
        _ => panic!("Not a Mode 2 test mode"),
    };

    // Write Mode2Config
    use ria_api::vga_mode2_config_t::*;
    tb.xram0_struct_set(config_ptr, X_WRAP, &[1]);
    tb.xram0_struct_set(config_ptr, Y_WRAP, &[1]);
    tb.xram0_struct_set(config_ptr, X_POS_PX, &0i16.to_le_bytes());
    tb.xram0_struct_set(config_ptr, Y_POS_PX, &0i16.to_le_bytes());
    tb.xram0_struct_set(config_ptr, WIDTH_TILES, &width_tiles.to_le_bytes());
    tb.xram0_struct_set(config_ptr, HEIGHT_TILES, &height_tiles.to_le_bytes());
    tb.xram0_struct_set(config_ptr, XRAM_DATA_PTR, &data_ptr.to_le_bytes());
    tb.xram0_struct_set(config_ptr, XRAM_PALETTE_PTR, &0xFFFFu16.to_le_bytes());
    tb.xram0_struct_set(config_ptr, XRAM_TILE_PTR, &tile_ptr.to_le_bytes());

    // Write tile bitmaps (matching pico-examples/src/mode2.c)
    if tile_size == 8 {
        // Tile 0: diagonal stripe (1,2,4,8,16,32,64,128)
        let tile0: [u8; 8] = [1, 2, 4, 8, 16, 32, 64, 128];
        // Tile 1: reverse diagonal (128,64,32,16,8,4,2,1)
        let tile1: [u8; 8] = [128, 64, 32, 16, 8, 4, 2, 1];
        tb.xram0_write(tile_ptr, &tile0);
        tb.xram0_write(tile_ptr + 8, &tile1);
    } else {
        // 16x16 tiles: 2 bytes/row, 16 rows = 32 bytes each
        // Tile 0: "X" pattern (from pico-examples)
        let tile0: [u8; 32] = [
            1, 128, 2, 64, 4, 32, 8, 16,
            16, 8, 32, 4, 64, 2, 128, 1,
            128, 1, 64, 2, 32, 4, 16, 8,
            8, 16, 4, 32, 2, 64, 1, 128,
        ];
        // Tile 1: "diamond" pattern (from pico-examples)
        let tile1: [u8; 32] = [
            128, 1, 64, 2, 32, 4, 16, 8,
            8, 16, 4, 32, 2, 64, 1, 128,
            1, 128, 2, 64, 4, 32, 8, 16,
            16, 8, 32, 4, 64, 2, 128, 1,
        ];
        tb.xram0_write(tile_ptr, &tile0);
        tb.xram0_write(tile_ptr + 32, &tile1);
    }

    // Write tile map: deterministic pattern (alternating 0,1)
    let map_size = width_tiles as usize * height_tiles as usize;
    let mut tile_map = Vec::with_capacity(map_size);
    for i in 0..map_size {
        tile_map.push((i % 2) as u8);
    }
    tb.xram0_write(data_ptr, &tile_map);

    // Configure VGA
    tb.xreg_vga_canvas(1); // 320x240
    tb.xreg_vga_mode(&[2, attr, config_ptr, 0, 0, 0]);

    tb.wait_frames(1);
    tb.op_exit();
    tb.trace
}
```

Wire into `generate_test_trace`:
```rust
        TestMode::Tile1bpp8x8 | TestMode::Tile1bpp16x16 => {
            return generate_mode2_test_trace(mode);
        }
```

**Step 3: Run all tests**

Run: `cargo test -p rp6502-emu`
Expected: All tests pass, including the `test_all_modes_produce_traces` and `test_trace_ends_with_exit` which now include the new Mode 2 variants.

**Step 4: Verify with screenshot**

Run: `cargo run -p rp6502-emu -- screenshot --mode tile1bpp8x8 -o /tmp/tile_test.png`
Expected: PNG shows diagonal stripe pattern in a tile grid.

**Step 5: Commit**

```bash
git add emu/src/test_harness.rs
git commit -m "feat: add Mode 2 test patterns (1bpp 8x8 and 16x16)"
```

---

### Task 6: Update CLAUDE.md with new modes

**Files:**
- Modify: `CLAUDE.md`

**Step 1: Add tile mode names to CLI usage section**

Add `tile1bpp8x8`, `tile1bpp16x16` to the valid `--mode` values list. Add Mode 2 to the "Current Scope" section.

**Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add Mode 2 to CLAUDE.md"
```
