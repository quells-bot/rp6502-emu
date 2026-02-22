use super::palette::{PALETTE_2, PALETTE_256, rgb565_to_rgba};

/// Mode 3 configuration, read from XRAM at config_ptr.
/// Matches firmware mode3_config_t exactly:
///   bool x_wrap      (1 byte, offset 0)
///   bool y_wrap      (1 byte, offset 1)
///   int16_t x_pos_px (2 bytes, offset 2)
///   int16_t y_pos_px (2 bytes, offset 4)
///   int16_t width_px (2 bytes, offset 6)
///   int16_t height_px(2 bytes, offset 8)
///   uint16_t xram_data_ptr    (2 bytes, offset 10)
///   uint16_t xram_palette_ptr (2 bytes, offset 12)
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

/// Color format attributes, matching firmware mode3_prog() switch statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorFormat {
    Bpp1Msb,  // attr 0: 1bpp MSB-first (bit 7 = pixel 0)
    Bpp2Msb,  // attr 1: 2bpp MSB-first (bits[7:6] = pixel 0)
    Bpp4Msb,  // attr 2: 4bpp MSB-first (high nibble = pixel 0)
    Bpp8,     // attr 3: 8bpp indexed
    Bpp16,    // attr 4: 16bpp direct color (PICO_SCANVIDEO format)
    Bpp1Lsb,  // attr 8: 1bpp LSB-first (bit 0 = pixel 0)
    Bpp2Lsb,  // attr 9: 2bpp LSB-first (bits[1:0] = pixel 0)
    Bpp4Lsb,  // attr 10: 4bpp LSB-first (low nibble = pixel 0)
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
    /// The pointer must be word-aligned and leave room for the 14-byte struct.
    pub fn from_xram(xram: &[u8; 65536], ptr: u16) -> Self {
        let p = ptr as usize;
        // Guard: struct is 14 bytes; ptr must leave room
        if p + 14 > 65536 {
            return Self {
                x_wrap: false,
                y_wrap: false,
                x_pos_px: 0,
                y_pos_px: 0,
                width_px: 0,
                height_px: 0,
                xram_data_ptr: 0,
                xram_palette_ptr: 0,
            };
        }
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
///
/// Mirrors firmware mode3_get_palette():
///   - palette_ptr must be even (word-aligned) and fit in XRAM
///   - 1bpp falls back to color_2 (PALETTE_2)
///   - all others fall back to color_256 (PALETTE_256)
fn resolve_palette(xram: &[u8; 65536], format: &ColorFormat, palette_ptr: u16) -> Vec<u32> {
    match format {
        ColorFormat::Bpp16 => vec![], // direct color, no palette needed
        ColorFormat::Bpp1Msb | ColorFormat::Bpp1Lsb => {
            let count = 2usize;
            // Note: palette_ptr == 0 is treated as "use default palette" here.
            // The firmware would read XRAM[0] as a custom palette, but in practice
            // programs use 0 as a null sentinel (XRAM[0] is typically the config struct).
            // Check alignment and bounds (mirrors firmware: !(ptr & 1) && fits)
            if palette_ptr & 1 == 0
                && palette_ptr > 0
                && (palette_ptr as usize + count * 2) <= 0x10000
            {
                let mut pal = Vec::with_capacity(count);
                for i in 0..count {
                    let offset = palette_ptr as usize + i * 2;
                    let raw = u16::from_le_bytes([xram[offset], xram[offset + 1]]);
                    pal.push(rgb565_to_rgba(raw));
                }
                pal
            } else {
                PALETTE_2.to_vec()
            }
        }
        _ => {
            let count = 1usize << format.bits_per_pixel();
            // Note: the firmware uses `2 ^ bpp` here (C bitwise XOR, not exponentiation),
            // which is a firmware bug (e.g. bpp=8 gives 10 instead of 256). We use
            // the correct `1 << bpp` count to avoid loading garbage for large palettes.
            // Note: palette_ptr == 0 is treated as "use default palette" here.
            // The firmware would read XRAM[0] as a custom palette, but in practice
            // programs use 0 as a null sentinel (XRAM[0] is typically the config struct).
            // Check alignment and bounds (mirrors firmware: !(ptr & 1) && fits)
            if palette_ptr & 1 == 0
                && palette_ptr > 0
                && (palette_ptr as usize + count * 2) <= 0x10000
            {
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

/// Extract a pixel index from bitmap data at a given column offset.
///
/// Bit extraction matches firmware render functions exactly:
///
/// 1bpp MSB (mode3_render_1bpp_0r): bit7=px0, bit6=px1, ..., bit0=px7
/// 1bpp LSB (mode3_render_1bpp_1r): bit0=px0, bit1=px1, ..., bit7=px7
///
/// 2bpp MSB (mode3_render_2bpp_0r): bits[7:6]=px0, bits[5:4]=px1, bits[3:2]=px2, bits[1:0]=px3
/// 2bpp LSB (mode3_render_2bpp_1r): bits[1:0]=px0, bits[3:2]=px1, bits[5:4]=px2, bits[7:6]=px3
///
/// 4bpp MSB (mode3_render_4bpp_0r): high nibble=px0, low nibble=px1 (per byte)
/// 4bpp LSB (mode3_render_4bpp_1r): low nibble=px0, high nibble=px1 (per byte)
fn get_pixel(data: &[u8], col: usize, format: &ColorFormat) -> u8 {
    match format {
        ColorFormat::Bpp8 => data[col],
        // 4bpp MSB: high nibble is even pixel, low nibble is odd pixel
        ColorFormat::Bpp4Msb => {
            let byte = data[col / 2];
            if col % 2 == 0 { byte >> 4 } else { byte & 0x0F }
        }
        // 4bpp LSB: low nibble is even pixel, high nibble is odd pixel
        ColorFormat::Bpp4Lsb => {
            let byte = data[col / 2];
            if col % 2 == 0 { byte & 0x0F } else { byte >> 4 }
        }
        // 2bpp MSB: bits[7:6]=px0, bits[5:4]=px1, bits[3:2]=px2, bits[1:0]=px3
        ColorFormat::Bpp2Msb => {
            let byte = data[col / 4];
            let shift = 6 - (col % 4) * 2;
            (byte >> shift) & 0x03
        }
        // 2bpp LSB: bits[1:0]=px0, bits[3:2]=px1, bits[5:4]=px2, bits[7:6]=px3
        ColorFormat::Bpp2Lsb => {
            let byte = data[col / 4];
            let shift = (col % 4) * 2;
            (byte >> shift) & 0x03
        }
        // 1bpp MSB: bit7=px0, bit6=px1, ..., bit0=px7
        ColorFormat::Bpp1Msb => {
            let byte = data[col / 8];
            let shift = 7 - (col % 8);
            (byte >> shift) & 0x01
        }
        // 1bpp LSB: bit0=px0, bit1=px1, ..., bit7=px7
        ColorFormat::Bpp1Lsb => {
            let byte = data[col / 8];
            let shift = col % 8;
            (byte >> shift) & 0x01
        }
        ColorFormat::Bpp16 => {
            // Not used via get_pixel — handled separately in render loop
            0
        }
    }
}

/// Render a Mode 3 plane into the framebuffer.
///
/// The framebuffer is an array of RGBA u32 values (R in bits 31:24, G in 23:16,
/// B in 15:8, A in 7:0), laid out as canvas_width x canvas_height pixels.
///
/// The framebuffer must be zero-initialized before calling this function.
/// Out-of-bounds and transparent pixels are skipped (not explicitly cleared),
/// so stale content will show through if the caller does not clear first.
///
/// Pixels are only written when alpha is non-zero (opaque), mirroring the
/// transparency convention used throughout the palette module.
pub fn render_mode3(
    plane: &Mode3Plane,
    xram: &[u8; 65536],
    framebuffer: &mut [u32],
    canvas_width: u16,
    canvas_height: u16,
) {
    let cfg = &plane.config;

    // Validate: width and height must be positive, matching firmware NULL-return check
    if cfg.width_px < 1 || cfg.height_px < 1 {
        return;
    }

    let bpp = plane.format.bits_per_pixel();
    let sizeof_row = ((cfg.width_px as u32 * bpp + 7) / 8) as usize;

    // Bounds check: entire bitmap must fit in XRAM, matching firmware check:
    //   sizeof_bitmap > 0x10000 - config->xram_data_ptr
    let sizeof_bitmap = cfg.height_px as usize * sizeof_row;
    if sizeof_bitmap > 0x10000usize.saturating_sub(cfg.xram_data_ptr as usize) {
        return;
    }

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

        // Y wrapping — mirrors firmware mode3_scanline_to_data():
        //   if (row < 0) row += (-(row+1)/height + 1) * height;
        //   if (row >= height) row -= ((row-height)/height + 1) * height;
        // This is equivalent to rem_euclid for valid height > 0.
        if cfg.y_wrap {
            row = row.rem_euclid(cfg.height_px as i32);
        }

        if row < 0 || row >= cfg.height_px as i32 {
            continue;
        }

        let row_offset = cfg.xram_data_ptr as usize + row as usize * sizeof_row;

        for screen_x in 0..canvas_width as i32 {
            let mut col = screen_x - cfg.x_pos_px as i32;

            // X wrapping — mirrors firmware mode3_fill_cols():
            //   if (col < 0 && x_wrap) col += (-(col+1)/width + 1) * width;
            //   if (col >= width && x_wrap) col -= ((col-width)/width + 1) * width;
            // Equivalent to rem_euclid for valid width > 0.
            if cfg.x_wrap {
                col = col.rem_euclid(cfg.width_px as i32);
            }

            if col < 0 || col >= cfg.width_px as i32 {
                // Out of bounds and no wrap: leave framebuffer pixel unchanged (transparent)
                continue;
            }

            let fb_idx = scanline as usize * canvas_width as usize + screen_x as usize;

            let rgba = if plane.format == ColorFormat::Bpp16 {
                // Direct color: 2 bytes per pixel in PICO_SCANVIDEO format
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

            // Only draw if pixel is opaque (alpha != 0), matching firmware transparency convention
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
        xram[p] = 0;     // x_wrap = false
        xram[p + 1] = 0; // y_wrap = false
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

        // Pixel (0,0) should be bright red (PALETTE_256[9])
        assert_eq!(fb[0], PALETTE_256[9]);
    }

    #[test]
    fn test_mode3_1bpp_msb() {
        let config_ptr = 0x0000u16;
        let data_ptr = 0x0100u16;
        let mut xram = make_xram_with_config(config_ptr, data_ptr, 8, 1);

        // Byte 0b10100101 -> pixels MSB-first: 1,0,1,0,0,1,0,1
        xram[data_ptr as usize] = 0b10100101;

        let plane = Mode3Plane {
            config: Mode3Config::from_xram(&xram, config_ptr),
            format: ColorFormat::Bpp1Msb,
            scanline_begin: 0,
            scanline_end: 1,
        };

        let mut fb = vec![0u32; 8];
        render_mode3(&plane, &xram, &mut fb, 8, 1);

        // bit7=1 -> opaque; bit6=0 -> transparent (palette[0] has alpha=0, fb stays 0)
        assert_ne!(fb[0], 0); // pixel 0 = 1 (opaque)
        assert_eq!(fb[1], 0); // pixel 1 = 0 (transparent, fb unchanged)
        assert_ne!(fb[2], 0); // pixel 2 = 1 (opaque)
        assert_eq!(fb[3], 0); // pixel 3 = 0 (transparent)
    }

    #[test]
    fn test_mode3_y_wrap() {
        let config_ptr = 0x0000u16;
        let data_ptr = 0x0100u16;
        let mut xram = make_xram_with_config(config_ptr, data_ptr, 1, 2);
        // Enable y_wrap
        xram[config_ptr as usize + 1] = 1;
        // Row 0: color index 1, Row 1: color index 2
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

        // 4 scanlines wrapping over 2-row bitmap: rows 0,2 = color 1; rows 1,3 = color 2
        assert_eq!(fb[0], PALETTE_256[1]);
        assert_eq!(fb[1], PALETTE_256[2]);
        assert_eq!(fb[2], PALETTE_256[1]);
        assert_eq!(fb[3], PALETTE_256[2]);
    }
}
