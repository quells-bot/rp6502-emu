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

/// Convert a 16-bit PICO_SCANVIDEO pixel value (as stored in XRAM custom palettes) to RGBA u32.
///
/// PICO_SCANVIDEO DPI format (from firmware scanvideo.h):
///   RSHIFT=0:  R5 at bits  4:0
///   GSHIFT=6:  G5 at bits 10:6
///   BSHIFT=11: B5 at bits 15:11
///   ALPHA_PIN=5: alpha at bit 5
///
/// Each 5-bit channel is scaled to 8-bit by (val << 3) | (val >> 2).
pub fn rgb565_to_rgba(raw: u16) -> u32 {
    let alpha = if raw & (1 << 5) != 0 { 0xFF } else { 0x00 };
    let r5 = (raw & 0x1F) as u8;
    let g5 = ((raw >> 6) & 0x1F) as u8;
    let b5 = ((raw >> 11) & 0x1F) as u8;
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
        // All bits set = alpha + white (all channels max)
        let rgba_val = rgb565_to_rgba(0xFFFF);
        assert_eq!(rgba_val & 0xFF, 0xFF); // alpha (bit 5 set)
        assert_eq!((rgba_val >> 24) & 0xFF, 0xFF); // R
        assert_eq!((rgba_val >> 16) & 0xFF, 0xFF); // G
        assert_eq!((rgba_val >> 8) & 0xFF, 0xFF);  // B
    }

    #[test]
    fn test_rgb565_to_rgba_transparent() {
        // Bit 5 clear = transparent; use a value with all channel bits set but bit 5 clear.
        // PICO_SCANVIDEO alpha is bit 5. A pixel with R5=0x1F, G5=0x1F, B5=0x1F but no alpha:
        // bits 4:0 = 0x1F (R), bits 10:6 = 0x1F (G -> bits 10:6 set), bits 15:11 = 0x1F (B),
        // bit 5 = 0 (no alpha). Value = 0xFFDF (all bits set except bit 5).
        let rgba_val = rgb565_to_rgba(0xFFDF);
        assert_eq!(rgba_val & 0xFF, 0x00); // alpha = 0
    }

    #[test]
    fn test_rgb565_to_rgba_red_only() {
        // Pure red: R5 = 0x1F at bits 4:0, G=0, B=0, alpha set (bit 5).
        // raw = 0x001F | (1 << 5) = 0x003F
        let raw: u16 = 0x003F;
        let rgba_val = rgb565_to_rgba(raw);
        assert_eq!(rgba_val & 0xFF, 0xFF);         // alpha opaque
        assert_eq!((rgba_val >> 24) & 0xFF, 0xFF); // R = 0xFF (0x1F scaled)
        assert_eq!((rgba_val >> 16) & 0xFF, 0x00); // G = 0
        assert_eq!((rgba_val >> 8) & 0xFF, 0x00);  // B = 0
    }

    #[test]
    fn test_palette_2() {
        assert_eq!(PALETTE_2[0] & 0xFF, 0x00); // transparent
        assert_eq!(PALETTE_2[1] & 0xFF, 0xFF); // opaque
    }
}
