use super::palette::resolve_palette;

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
    // NOTE: The firmware's mode2_fill_cols() hardcodes width_tiles*8 regardless of tile_size,
    // which is a bug for 16x16 tiles. The emulator uses width_tiles*tile_size (correct).
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
        assert_eq!(Mode2Format::from_attr(4), None);
        assert_eq!(Mode2Format::from_attr(5), None);
        assert_eq!(Mode2Format::from_attr(7), None);
        assert_eq!(Mode2Format::from_attr(12), None);
    }

    #[test]
    fn test_mode2_config_from_xram() {
        let mut xram = Box::new([0u8; 65536]);
        let p = 0xFF00usize;
        xram[p] = 1;
        xram[p + 1] = 0;
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

    fn make_mode2_xram(
        config_ptr: u16,
        data_ptr: u16,
        tile_ptr: u16,
        width_tiles: i16,
        height_tiles: i16,
    ) -> Box<[u8; 65536]> {
        let mut xram = Box::new([0u8; 65536]);
        let p = config_ptr as usize;
        xram[p] = 0;
        xram[p + 1] = 0;
        xram[p + 2..p + 4].copy_from_slice(&0i16.to_le_bytes());
        xram[p + 4..p + 6].copy_from_slice(&0i16.to_le_bytes());
        xram[p + 6..p + 8].copy_from_slice(&width_tiles.to_le_bytes());
        xram[p + 8..p + 10].copy_from_slice(&height_tiles.to_le_bytes());
        xram[p + 10..p + 12].copy_from_slice(&data_ptr.to_le_bytes());
        xram[p + 12..p + 14].copy_from_slice(&0xFFFFu16.to_le_bytes());
        xram[p + 14..p + 16].copy_from_slice(&tile_ptr.to_le_bytes());
        xram
    }

    #[test]
    fn test_mode2_1bpp_8x8_solid_tile() {
        let config_ptr = 0xFF00u16;
        let data_ptr = 0x0000u16;
        let tile_ptr = 0x1000u16;
        let mut xram = make_mode2_xram(config_ptr, data_ptr, tile_ptr, 1, 1);
        for row in 0..8 {
            xram[tile_ptr as usize + row] = 0xFF;
        }
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
        for y in 0..8 {
            for x in 0..8 {
                assert_ne!(fb[y * 8 + x] & 0xFF, 0, "pixel ({x},{y}) should be opaque");
            }
        }
    }

    #[test]
    fn test_mode2_1bpp_8x8_two_tiles() {
        let config_ptr = 0xFF00u16;
        let data_ptr = 0x0000u16;
        let tile_ptr = 0x1000u16;
        let mut xram = make_mode2_xram(config_ptr, data_ptr, tile_ptr, 2, 1);
        for row in 0..8 {
            xram[tile_ptr as usize + row] = 0x00;
            xram[tile_ptr as usize + 8 + row] = 0xFF;
        }
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
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(fb[y * 16 + x], 0, "left tile pixel ({x},{y}) should be transparent");
            }
        }
        for y in 0..8 {
            for x in 8..16 {
                assert_ne!(fb[y * 16 + x] & 0xFF, 0, "right tile pixel ({x},{y}) should be opaque");
            }
        }
    }

    #[test]
    fn test_mode2_8bpp_8x8() {
        let config_ptr = 0xFF00u16;
        let data_ptr = 0x0000u16;
        let tile_ptr = 0x1000u16;
        let mut xram = make_mode2_xram(config_ptr, data_ptr, tile_ptr, 1, 1);
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
                assert_eq!(fb[y * 8 + x], PALETTE_256[9], "pixel ({x},{y}) should be bright red");
            }
        }
    }

    #[test]
    fn test_mode2_y_wrap() {
        let config_ptr = 0xFF00u16;
        let data_ptr = 0x0000u16;
        let tile_ptr = 0x1000u16;
        let mut xram = make_mode2_xram(config_ptr, data_ptr, tile_ptr, 1, 1);
        xram[config_ptr as usize + 1] = 1; // y_wrap
        for row in 0..8 {
            xram[tile_ptr as usize + row] = 0xFF;
        }
        xram[data_ptr as usize] = 0;
        let plane = Mode2Plane {
            config: Mode2Config::from_xram(&xram, config_ptr),
            format: Mode2Format::Bpp1_8x8,
            scanline_begin: 0,
            scanline_end: 16,
            config_ptr,
        };
        let mut fb = vec![0u32; 8 * 16];
        render_mode2(&plane, &xram, &mut fb, 8, 16);
        assert_ne!(fb[0] & 0xFF, 0, "row 0 should have content");
        assert_ne!(fb[8 * 8] & 0xFF, 0, "row 8 should wrap and have content");
    }
}
