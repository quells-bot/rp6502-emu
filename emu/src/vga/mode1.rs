use super::font::{FONT8, FONT16};
use super::palette::{resolve_palette, rgb565_to_rgba};

/// Mode 1 configuration, read from XRAM at config_ptr.
/// Matches firmware mode1_config_t exactly (16 bytes):
///   bool x_wrap           (1 byte, offset 0)
///   bool y_wrap           (1 byte, offset 1)
///   int16_t x_pos_px      (2 bytes, offset 2)
///   int16_t y_pos_px      (2 bytes, offset 4)
///   int16_t width_chars   (2 bytes, offset 6)
///   int16_t height_chars  (2 bytes, offset 8)
///   uint16_t xram_data_ptr    (2 bytes, offset 10)
///   uint16_t xram_palette_ptr (2 bytes, offset 12)
///   uint16_t xram_font_ptr    (2 bytes, offset 14)
#[derive(Debug, Clone)]
pub struct Mode1Config {
    pub x_wrap: bool,
    pub y_wrap: bool,
    pub x_pos_px: i16,
    pub y_pos_px: i16,
    pub width_chars: i16,
    pub height_chars: i16,
    pub xram_data_ptr: u16,
    pub xram_palette_ptr: u16,
    pub xram_font_ptr: u16,
}

/// Mode 1 format, encoding both font size and color depth.
/// Matches firmware mode1_prog() attribute switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode1Format {
    Bpp1_8x8,    // attr 0
    Bpp4r_8x8,   // attr 1: 4bpp reversed (fg_bg nibble order)
    Bpp4_8x8,    // attr 2: 4bpp normal (bg_fg nibble order)
    Bpp8_8x8,    // attr 3
    Bpp16_8x8,   // attr 4
    Bpp1_8x16,   // attr 8
    Bpp4r_8x16,  // attr 9
    Bpp4_8x16,   // attr 10
    Bpp8_8x16,   // attr 11
    Bpp16_8x16,  // attr 12
}

impl Mode1Format {
    pub fn from_attr(attr: u16) -> Option<Self> {
        match attr {
            0 => Some(Self::Bpp1_8x8),
            1 => Some(Self::Bpp4r_8x8),
            2 => Some(Self::Bpp4_8x8),
            3 => Some(Self::Bpp8_8x8),
            4 => Some(Self::Bpp16_8x8),
            8 => Some(Self::Bpp1_8x16),
            9 => Some(Self::Bpp4r_8x16),
            10 => Some(Self::Bpp4_8x16),
            11 => Some(Self::Bpp8_8x16),
            12 => Some(Self::Bpp16_8x16),
            _ => None,
        }
    }

    /// Font height in pixels.
    pub fn font_height(&self) -> i16 {
        match self {
            Self::Bpp1_8x8 | Self::Bpp4r_8x8 | Self::Bpp4_8x8
            | Self::Bpp8_8x8 | Self::Bpp16_8x8 => 8,
            _ => 16,
        }
    }

    /// Bytes per character cell in the data array.
    pub fn cell_size(&self) -> usize {
        match self {
            Self::Bpp1_8x8 | Self::Bpp1_8x16 => 1,
            Self::Bpp4r_8x8 | Self::Bpp4_8x8
            | Self::Bpp4r_8x16 | Self::Bpp4_8x16 => 2,
            Self::Bpp8_8x8 | Self::Bpp8_8x16 => 3,
            Self::Bpp16_8x8 | Self::Bpp16_8x16 => 6,
        }
    }

    /// Bits per pixel for palette resolution.
    fn bpp(&self) -> u32 {
        match self {
            Self::Bpp1_8x8 | Self::Bpp1_8x16 => 1,
            Self::Bpp4r_8x8 | Self::Bpp4_8x8
            | Self::Bpp4r_8x16 | Self::Bpp4_8x16 => 4,
            Self::Bpp8_8x8 | Self::Bpp8_8x16 => 8,
            Self::Bpp16_8x8 | Self::Bpp16_8x16 => 16,
        }
    }
}

/// A programmed Mode 1 plane.
#[derive(Debug, Clone)]
pub struct Mode1Plane {
    pub config: Mode1Config,
    pub format: Mode1Format,
    pub scanline_begin: u16,
    pub scanline_end: u16,
    pub config_ptr: u16,
}

impl Mode1Config {
    /// Read config from XRAM at the given pointer.
    pub fn from_xram(xram: &[u8; 65536], ptr: u16) -> Self {
        let p = ptr as usize;
        if p + 16 > 65536 {
            return Self {
                x_wrap: false,
                y_wrap: false,
                x_pos_px: 0,
                y_pos_px: 0,
                width_chars: 0,
                height_chars: 0,
                xram_data_ptr: 0,
                xram_palette_ptr: 0,
                xram_font_ptr: 0,
            };
        }
        Self {
            x_wrap: xram[p] != 0,
            y_wrap: xram[p + 1] != 0,
            x_pos_px: i16::from_le_bytes([xram[p + 2], xram[p + 3]]),
            y_pos_px: i16::from_le_bytes([xram[p + 4], xram[p + 5]]),
            width_chars: i16::from_le_bytes([xram[p + 6], xram[p + 7]]),
            height_chars: i16::from_le_bytes([xram[p + 8], xram[p + 9]]),
            xram_data_ptr: u16::from_le_bytes([xram[p + 10], xram[p + 11]]),
            xram_palette_ptr: u16::from_le_bytes([xram[p + 12], xram[p + 13]]),
            xram_font_ptr: u16::from_le_bytes([xram[p + 14], xram[p + 15]]),
        }
    }
}

/// Resolve font: use XRAM font if pointer is in bounds, else built-in.
/// Matches firmware mode1_get_font():
///   if (font_ptr <= 0x10000 - 256 * font_height) return &xram[font_ptr]
///   else return built-in
fn resolve_font<'a>(xram: &'a [u8; 65536], font_ptr: u16, font_height: i16) -> &'a [u8] {
    let font_size = 256 * font_height as usize;
    if (font_ptr as usize) + font_size <= 0x10000 {
        &xram[font_ptr as usize..font_ptr as usize + font_size]
    } else if font_height == 8 {
        &FONT8
    } else {
        &FONT16
    }
}

/// Resolve fg/bg colors for a single character cell.
/// Returns (bg_rgba, fg_rgba).
fn resolve_cell_colors(
    xram: &[u8; 65536],
    format: &Mode1Format,
    cell_offset: usize,
    palette: &[u32],
) -> (u32, u32) {
    match format {
        Mode1Format::Bpp1_8x8 | Mode1Format::Bpp1_8x16 => {
            // 1bpp: palette[0] = bg, palette[1] = fg
            let bg = if !palette.is_empty() { palette[0] } else { 0 };
            let fg = if palette.len() > 1 { palette[1] } else { 0 };
            (bg, fg)
        }
        Mode1Format::Bpp4r_8x8 | Mode1Format::Bpp4r_8x16 => {
            // 4bpp reversed: byte[1] = fg_bg_index (high=fg, low=bg)
            let fb_byte = xram[cell_offset + 1];
            let fg_idx = (fb_byte >> 4) as usize;
            let bg_idx = (fb_byte & 0x0F) as usize;
            let bg = palette.get(bg_idx).copied().unwrap_or(0);
            let fg = palette.get(fg_idx).copied().unwrap_or(0);
            (bg, fg)
        }
        Mode1Format::Bpp4_8x8 | Mode1Format::Bpp4_8x16 => {
            // 4bpp normal: byte[1] = bg_fg_index (high=bg, low=fg)
            let bf_byte = xram[cell_offset + 1];
            let bg_idx = (bf_byte >> 4) as usize;
            let fg_idx = (bf_byte & 0x0F) as usize;
            let bg = palette.get(bg_idx).copied().unwrap_or(0);
            let fg = palette.get(fg_idx).copied().unwrap_or(0);
            (bg, fg)
        }
        Mode1Format::Bpp8_8x8 | Mode1Format::Bpp8_8x16 => {
            // 8bpp: byte[1] = fg_index, byte[2] = bg_index
            let fg_idx = xram[cell_offset + 1] as usize;
            let bg_idx = xram[cell_offset + 2] as usize;
            let bg = palette.get(bg_idx).copied().unwrap_or(0);
            let fg = palette.get(fg_idx).copied().unwrap_or(0);
            (bg, fg)
        }
        Mode1Format::Bpp16_8x8 | Mode1Format::Bpp16_8x16 => {
            // 16bpp: byte[1] = attributes (ignored), bytes[2..4] = fg_color, bytes[4..6] = bg_color
            let fg_raw = u16::from_le_bytes([
                xram[cell_offset + 2],
                xram[cell_offset + 3],
            ]);
            let bg_raw = u16::from_le_bytes([
                xram[cell_offset + 4],
                xram[cell_offset + 5],
            ]);
            (rgb565_to_rgba(bg_raw), rgb565_to_rgba(fg_raw))
        }
    }
}

/// Render a Mode 1 plane into the framebuffer.
///
/// The framebuffer is an array of RGBA u32 values (R in bits 31:24, G in 23:16,
/// B in 15:8, A in 7:0), laid out as canvas_width x canvas_height pixels.
///
/// Pixels are only written when alpha is non-zero (opaque).
pub fn render_mode1(
    plane: &Mode1Plane,
    xram: &[u8; 65536],
    framebuffer: &mut [u32],
    canvas_width: u16,
    canvas_height: u16,
) {
    let cfg = &plane.config;
    let font_height = plane.format.font_height();
    let cell_size = plane.format.cell_size();

    if cfg.width_chars < 1 || cfg.height_chars < 1 {
        return;
    }

    // Bounds check: character data must fit in XRAM
    let height_px = cfg.height_chars as i32 * font_height as i32;
    let sizeof_row = cfg.width_chars as usize * cell_size;
    let sizeof_data = cfg.height_chars as usize * sizeof_row;
    if sizeof_data > 0x10000usize.saturating_sub(cfg.xram_data_ptr as usize) {
        return;
    }

    let font = resolve_font(xram, cfg.xram_font_ptr, font_height);
    let palette = resolve_palette(xram, plane.format.bpp(), cfg.xram_palette_ptr);

    let y_start = plane.scanline_begin as i32;
    let y_end = if plane.scanline_end == 0 {
        canvas_height as i32
    } else {
        plane.scanline_end as i32
    };

    let width_px = cfg.width_chars as i32 * 8;

    for scanline in y_start..y_end {
        if scanline < 0 || scanline >= canvas_height as i32 {
            continue;
        }

        let mut row = scanline - cfg.y_pos_px as i32;

        // Y wrapping on height_chars * font_height pixels
        if cfg.y_wrap {
            row = row.rem_euclid(height_px);
        }

        if row < 0 || row >= height_px {
            continue;
        }

        let char_row = row / font_height as i32;
        let font_row_in_glyph = row & (font_height as i32 - 1);
        let font_row_offset = (font_row_in_glyph as usize) * 256;
        let row_data_offset = cfg.xram_data_ptr as usize + char_row as usize * sizeof_row;

        for screen_x in 0..canvas_width as i32 {
            let mut col = screen_x - cfg.x_pos_px as i32;

            // X wrapping on width_chars * 8 pixels
            if cfg.x_wrap {
                col = col.rem_euclid(width_px);
            }

            if col < 0 || col >= width_px {
                continue;
            }

            let char_col = col / 8;
            let bit_in_char = 7 - (col & 7); // MSB first

            let cell_offset = row_data_offset + char_col as usize * cell_size;
            if cell_offset >= 0x10000 {
                continue;
            }

            let glyph_code = xram[cell_offset] as usize;
            let font_byte = font[font_row_offset + glyph_code];
            let bit = (font_byte >> bit_in_char) & 1;

            let (bg, fg) = resolve_cell_colors(xram, &plane.format, cell_offset, &palette);
            let rgba = if bit == 1 { fg } else { bg };

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
    use crate::vga::palette::PALETTE_256;

    fn make_mode1_xram(
        config_ptr: u16,
        data_ptr: u16,
        width_chars: i16,
        height_chars: i16,
    ) -> Box<[u8; 65536]> {
        let mut xram = Box::new([0u8; 65536]);
        let p = config_ptr as usize;
        xram[p] = 0;     // x_wrap = false
        xram[p + 1] = 0; // y_wrap = false
        xram[p + 2..p + 4].copy_from_slice(&0i16.to_le_bytes()); // x_pos = 0
        xram[p + 4..p + 6].copy_from_slice(&0i16.to_le_bytes()); // y_pos = 0
        xram[p + 6..p + 8].copy_from_slice(&width_chars.to_le_bytes());
        xram[p + 8..p + 10].copy_from_slice(&height_chars.to_le_bytes());
        xram[p + 10..p + 12].copy_from_slice(&data_ptr.to_le_bytes());
        xram[p + 12..p + 14].copy_from_slice(&0xFFFFu16.to_le_bytes()); // palette_ptr = 0xFFFF (built-in)
        xram[p + 14..p + 16].copy_from_slice(&0xFFFFu16.to_le_bytes()); // font_ptr = 0xFFFF (built-in)
        xram
    }

    #[test]
    fn test_mode1_1bpp_single_char() {
        let config_ptr = 0x0000u16;
        let data_ptr = 0x0100u16;
        // 1 char wide, 1 char tall, 8x8 font
        let xram = make_mode1_xram(config_ptr, data_ptr, 1, 1);

        // Write glyph 0xDB (full block = all 0xFF in font)
        // xram[data_ptr] is already 0 but we set glyph 0xDB
        let mut xram = xram;
        xram[data_ptr as usize] = 0xDB;

        let plane = Mode1Plane {
            config: Mode1Config::from_xram(&xram, config_ptr),
            format: Mode1Format::Bpp1_8x8,
            scanline_begin: 0,
            scanline_end: 8,
            config_ptr,
        };

        // Canvas is 8x8 to fit exactly one character
        let mut fb = vec![0u32; 8 * 8];
        render_mode1(&plane, &xram, &mut fb, 8, 8);

        // Full block: every pixel should be palette[1] (fg, opaque)
        for y in 0..8 {
            for x in 0..8 {
                let px = fb[y * 8 + x];
                assert_ne!(px & 0xFF, 0, "pixel ({x},{y}) should be opaque");
            }
        }
    }

    #[test]
    fn test_mode1_1bpp_space_is_transparent() {
        let config_ptr = 0x0000u16;
        let data_ptr = 0x0100u16;
        let xram = make_mode1_xram(config_ptr, data_ptr, 1, 1);
        // glyph 0x20 (space) = all zeros, so all pixels should be bg (transparent for 1bpp)
        let mut xram = xram;
        xram[data_ptr as usize] = 0x20;

        let plane = Mode1Plane {
            config: Mode1Config::from_xram(&xram, config_ptr),
            format: Mode1Format::Bpp1_8x8,
            scanline_begin: 0,
            scanline_end: 8,
            config_ptr,
        };

        let mut fb = vec![0u32; 8 * 8];
        render_mode1(&plane, &xram, &mut fb, 8, 8);

        // Space with 1bpp default palette: bg is palette[0] which is transparent
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(fb[y * 8 + x], 0, "pixel ({x},{y}) should be transparent");
            }
        }
    }

    #[test]
    fn test_mode1_8bpp_fg_bg_colors() {
        let config_ptr = 0x0000u16;
        let data_ptr = 0x0100u16;
        let mut xram = make_mode1_xram(config_ptr, data_ptr, 1, 1);
        // 8bpp cell: glyph_code, fg_index, bg_index
        xram[data_ptr as usize] = 0xDB;     // full block
        xram[data_ptr as usize + 1] = 9;    // fg = bright red
        xram[data_ptr as usize + 2] = 12;   // bg = bright blue

        let plane = Mode1Plane {
            config: Mode1Config::from_xram(&xram, config_ptr),
            format: Mode1Format::Bpp8_8x8,
            scanline_begin: 0,
            scanline_end: 8,
            config_ptr,
        };

        let mut fb = vec![0u32; 8 * 8];
        render_mode1(&plane, &xram, &mut fb, 8, 8);

        // Full block: all pixels should be fg color (bright red = PALETTE_256[9])
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(fb[y * 8 + x], PALETTE_256[9],
                    "pixel ({x},{y}) should be bright red");
            }
        }
    }

    #[test]
    fn test_mode1_config_from_xram() {
        let mut xram = Box::new([0u8; 65536]);
        let p = 0x100usize;
        xram[p] = 1;     // x_wrap
        xram[p + 1] = 0; // y_wrap
        xram[p + 2..p + 4].copy_from_slice(&10i16.to_le_bytes());  // x_pos
        xram[p + 4..p + 6].copy_from_slice(&20i16.to_le_bytes());  // y_pos
        xram[p + 6..p + 8].copy_from_slice(&40i16.to_le_bytes());  // width_chars
        xram[p + 8..p + 10].copy_from_slice(&30i16.to_le_bytes()); // height_chars
        xram[p + 10..p + 12].copy_from_slice(&0x2000u16.to_le_bytes()); // data_ptr
        xram[p + 12..p + 14].copy_from_slice(&0x4000u16.to_le_bytes()); // palette_ptr
        xram[p + 14..p + 16].copy_from_slice(&0xFFFFu16.to_le_bytes()); // font_ptr

        let cfg = Mode1Config::from_xram(&xram, 0x100);
        assert!(cfg.x_wrap);
        assert!(!cfg.y_wrap);
        assert_eq!(cfg.x_pos_px, 10);
        assert_eq!(cfg.y_pos_px, 20);
        assert_eq!(cfg.width_chars, 40);
        assert_eq!(cfg.height_chars, 30);
        assert_eq!(cfg.xram_data_ptr, 0x2000);
        assert_eq!(cfg.xram_palette_ptr, 0x4000);
        assert_eq!(cfg.xram_font_ptr, 0xFFFF);
    }

    #[test]
    fn test_mode1_format_from_attr() {
        assert_eq!(Mode1Format::from_attr(0), Some(Mode1Format::Bpp1_8x8));
        assert_eq!(Mode1Format::from_attr(1), Some(Mode1Format::Bpp4r_8x8));
        assert_eq!(Mode1Format::from_attr(2), Some(Mode1Format::Bpp4_8x8));
        assert_eq!(Mode1Format::from_attr(3), Some(Mode1Format::Bpp8_8x8));
        assert_eq!(Mode1Format::from_attr(4), Some(Mode1Format::Bpp16_8x8));
        assert_eq!(Mode1Format::from_attr(8), Some(Mode1Format::Bpp1_8x16));
        assert_eq!(Mode1Format::from_attr(12), Some(Mode1Format::Bpp16_8x16));
        assert_eq!(Mode1Format::from_attr(5), None);
        assert_eq!(Mode1Format::from_attr(7), None);
    }

    #[test]
    fn test_mode1_y_wrap() {
        let config_ptr = 0x0000u16;
        let data_ptr = 0x0100u16;
        let mut xram = make_mode1_xram(config_ptr, data_ptr, 1, 1);
        // Enable y_wrap
        xram[config_ptr as usize] = 0; // x_wrap off
        xram[config_ptr as usize + 1] = 1; // y_wrap on
        // Full block glyph
        xram[data_ptr as usize] = 0xDB;

        let plane = Mode1Plane {
            config: Mode1Config::from_xram(&xram, config_ptr),
            format: Mode1Format::Bpp1_8x8,
            scanline_begin: 0,
            scanline_end: 16, // 16 scanlines but only 1 char tall (8px), should wrap
            config_ptr,
        };

        let mut fb = vec![0u32; 8 * 16];
        render_mode1(&plane, &xram, &mut fb, 8, 16);

        // Row 0 and row 8 should both have content (wrapped)
        assert_ne!(fb[0] & 0xFF, 0, "row 0 should have content");
        assert_ne!(fb[8 * 8] & 0xFF, 0, "row 8 should wrap and have content");
    }
}
