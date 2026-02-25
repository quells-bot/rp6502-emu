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
}
