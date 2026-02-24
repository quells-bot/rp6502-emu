use crate::bus::BusTransaction;

/// Valid canvas + color depth combinations that fit in 64KB XRAM.
/// Each variant encodes both the canvas size and the bits-per-pixel.
#[derive(Debug, Clone, Copy)]
pub enum TestMode {
    /// 640x480 canvas, 1bpp = 38,400 bytes
    Mono640x480,
    /// 640x360 canvas, 1bpp = 28,800 bytes
    Mono640x360,
    /// 320x240 canvas, 1bpp = 9,600 bytes, 2x pixel doubling
    Mono320x240,
    /// 320x180 canvas, 1bpp = 7,200 bytes, 2x pixel doubling
    Mono320x180,
    /// 640x360 canvas, 2bpp = 57,600 bytes
    Color2bpp640x360,
    /// 320x240 canvas, 2bpp = 19,200 bytes, 2x pixel doubling
    Color2bpp320x240,
    /// 320x180 canvas, 2bpp = 14,400 bytes, 2x pixel doubling
    Color2bpp320x180,
    /// 320x240 canvas, 4bpp = 38,400 bytes, 2x pixel doubling
    Color4bpp320x240,
    /// 320x180 canvas, 4bpp = 28,800 bytes, 2x pixel doubling
    Color4bpp320x180,
    /// 320x180 canvas, 8bpp = 57,600 bytes, 2x pixel doubling
    Color8bpp320x180,
    /// 320x240 canvas (2x), 16bpp partial: 320x102 bitmap (~65KB)
    Color16bpp320,
    /// 320x240 canvas, Mode 1, 1bpp 8x16 font (40x15 chars)
    Text1bpp320x240,
    /// 320x240 canvas, Mode 1, 8bpp 8x8 font (40x30 chars)
    Text8bpp320x240,
}

impl std::fmt::Display for TestMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            TestMode::Mono640x480 => "mono640x480",
            TestMode::Mono640x360 => "mono640x360",
            TestMode::Mono320x240 => "mono320x240",
            TestMode::Mono320x180 => "mono320x180",
            TestMode::Color2bpp640x360 => "color2bpp640x360",
            TestMode::Color2bpp320x240 => "color2bpp320x240",
            TestMode::Color2bpp320x180 => "color2bpp320x180",
            TestMode::Color4bpp320x240 => "color4bpp320x240",
            TestMode::Color4bpp320x180 => "color4bpp320x180",
            TestMode::Color8bpp320x180 => "color8bpp320x180",
            TestMode::Color16bpp320 => "color16bpp320",
            TestMode::Text1bpp320x240 => "text1bpp320x240",
            TestMode::Text8bpp320x240 => "text8bpp320x240",
        };
        write!(f, "{}", name)
    }
}

impl std::str::FromStr for TestMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "mono640x480" => Ok(TestMode::Mono640x480),
            "mono640x360" => Ok(TestMode::Mono640x360),
            "mono320x240" => Ok(TestMode::Mono320x240),
            "mono320x180" => Ok(TestMode::Mono320x180),
            "color2bpp640x360" => Ok(TestMode::Color2bpp640x360),
            "color2bpp320x240" => Ok(TestMode::Color2bpp320x240),
            "color2bpp320x180" => Ok(TestMode::Color2bpp320x180),
            "color4bpp320x240" => Ok(TestMode::Color4bpp320x240),
            "color4bpp320x180" => Ok(TestMode::Color4bpp320x180),
            "color8bpp320x180" => Ok(TestMode::Color8bpp320x180),
            "color16bpp320" => Ok(TestMode::Color16bpp320),
            "text1bpp320x240" => Ok(TestMode::Text1bpp320x240),
            "text8bpp320x240" => Ok(TestMode::Text8bpp320x240),
            _ => Err(format!(
                "unknown mode '{}'. Valid modes: {}",
                s,
                TestMode::all().iter().map(|m| m.to_string()).collect::<Vec<_>>().join(", ")
            )),
        }
    }
}

impl TestMode {
    /// All valid modes for iteration in tests.
    pub fn all() -> &'static [TestMode] {
        &[
            TestMode::Mono640x480,
            TestMode::Mono640x360,
            TestMode::Mono320x240,
            TestMode::Mono320x180,
            TestMode::Color2bpp640x360,
            TestMode::Color2bpp320x240,
            TestMode::Color2bpp320x180,
            TestMode::Color4bpp320x240,
            TestMode::Color4bpp320x180,
            TestMode::Color8bpp320x180,
            TestMode::Color16bpp320,
            TestMode::Text1bpp320x240,
            TestMode::Text8bpp320x240,
        ]
    }

    /// Canvas register value (1-4).
    fn canvas_reg(&self) -> u16 {
        match self {
            TestMode::Mono320x240 | TestMode::Color2bpp320x240
            | TestMode::Color4bpp320x240 | TestMode::Color16bpp320 => 1,  // 320x240
            TestMode::Mono320x180 | TestMode::Color2bpp320x180
            | TestMode::Color4bpp320x180 | TestMode::Color8bpp320x180 => 2,  // 320x180
            TestMode::Mono640x480 => 3,  // 640x480
            TestMode::Mono640x360 | TestMode::Color2bpp640x360 => 4,  // 640x360
            TestMode::Text1bpp320x240 | TestMode::Text8bpp320x240 => unreachable!(),
        }
    }

    /// Canvas pixel dimensions.
    fn canvas_size(&self) -> (i16, i16) {
        match self.canvas_reg() {
            1 => (320, 240),
            2 => (320, 180),
            3 => (640, 480),
            4 => (640, 360),
            _ => unreachable!(),
        }
    }

    /// Bits per pixel.
    fn bpp(&self) -> u16 {
        match self {
            TestMode::Mono640x480 | TestMode::Mono640x360
            | TestMode::Mono320x240 | TestMode::Mono320x180 => 1,
            TestMode::Color2bpp640x360 | TestMode::Color2bpp320x240
            | TestMode::Color2bpp320x180 => 2,
            TestMode::Color4bpp320x240 | TestMode::Color4bpp320x180 => 4,
            TestMode::Color8bpp320x180 => 8,
            TestMode::Color16bpp320 => 16,
            TestMode::Text1bpp320x240 | TestMode::Text8bpp320x240 => unreachable!(),
        }
    }

    /// Mode 3 attribute value.
    fn attr(&self) -> u16 {
        match self.bpp() {
            1 => 0,   // Bpp1Msb
            2 => 1,   // Bpp2Msb
            4 => 2,   // Bpp4Msb
            8 => 3,   // Bpp8
            16 => 4,  // Bpp16
            _ => unreachable!(),
        }
    }

    /// Bitmap dimensions. For 16bpp the height is limited by XRAM capacity.
    fn bitmap_size(&self) -> (i16, i16) {
        let (w, _h) = self.canvas_size();
        if self.bpp() == 16 {
            // data_ptr = 0x0100 (256 bytes reserved for config)
            let bytes_per_row = w as u32 * 2;
            let max_rows = (65536u32 - 256) / bytes_per_row;
            (w, max_rows as i16)
        } else {
            self.canvas_size()
        }
    }
}

/// Generate a bus trace that programs Mode 1 with a test pattern.
///
/// The trace:
/// 1. Writes a Mode1Config struct to XRAM at address 0x0000 via ADDR0/RW0
/// 2. Writes character data at address 0x0100 via ADDR0/RW0
/// 3. Configures VGA via xreg: CANVAS, MODE=1, attr, config_ptr=0
/// 4. Exits after one frame worth of cycles
pub fn generate_mode1_test_trace(mode: TestMode) -> Vec<BusTransaction> {
    let mut trace = Vec::new();
    let mut cycle: u64 = 0;

    let config_ptr: u16 = 0x0000;
    let data_ptr: u16 = 0x0100;

    let (width_chars, height_chars, attr, cell_size): (i16, i16, u16, usize) = match mode {
        TestMode::Text1bpp320x240 => (40, 15, 8, 1),   // 8x16, 1bpp
        TestMode::Text8bpp320x240 => (40, 30, 3, 3),   // 8x8, 8bpp
        _ => panic!("Not a Mode 1 test mode"),
    };

    // --- Step 1: Write Mode1Config (16 bytes) to XRAM ---
    trace.push(BusTransaction::write(cycle, 0xFFE6, (config_ptr & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFE7, (config_ptr >> 8) as u8));
    cycle += 1;

    let config_bytes: [u8; 16] = [
        0, 0,                                                          // x_wrap=false, y_wrap=false
        0, 0,                                                          // x_pos_px = 0
        0, 0,                                                          // y_pos_px = 0
        (width_chars & 0xFF) as u8, (width_chars >> 8) as u8,         // width_chars
        (height_chars & 0xFF) as u8, (height_chars >> 8) as u8,       // height_chars
        (data_ptr & 0xFF) as u8, (data_ptr >> 8) as u8,               // xram_data_ptr
        0xFF, 0xFF,                                                     // xram_palette_ptr = 0xFFFF (built-in)
        0xFF, 0xFF,                                                     // xram_font_ptr = 0xFFFF (built-in)
    ];
    for &b in &config_bytes {
        trace.push(BusTransaction::write(cycle, 0xFFE4, b));
        cycle += 1;
    }

    // --- Step 2: Write character data ---
    trace.push(BusTransaction::write(cycle, 0xFFE6, (data_ptr & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFE7, (data_ptr >> 8) as u8));
    cycle += 1;

    for row in 0..height_chars as u32 {
        for col in 0..width_chars as u32 {
            // Cycle through printable ASCII glyphs
            let glyph = 0x21 + ((row * width_chars as u32 + col) % 94) as u8; // '!' to '~'
            trace.push(BusTransaction::write(cycle, 0xFFE4, glyph));
            cycle += 1;

            if cell_size >= 2 {
                // 8bpp: fg_index, bg_index
                let fg = (1 + (col % 15)) as u8;       // colors 1-15 (avoid 0 = transparent)
                let bg = 16;                             // opaque black (grey0)
                trace.push(BusTransaction::write(cycle, 0xFFE4, fg));
                cycle += 1;
                if cell_size >= 3 {
                    trace.push(BusTransaction::write(cycle, 0xFFE4, bg));
                    cycle += 1;
                }
            }
        }
    }

    // --- Step 3: Configure VGA via xreg ---
    // First xreg: CANVAS (320x240 = canvas value 1)
    trace.push(BusTransaction::write(cycle, 0xFFEC, 1)); // device
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0)); // channel
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0)); // start_addr
    cycle += 1;
    let canvas: u16 = 1; // 320x240
    trace.push(BusTransaction::write(cycle, 0xFFEC, (canvas >> 8) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, (canvas & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEF, 0x01)); // trigger xreg
    cycle += 1;

    // Second xreg: MODE=1 + attributes
    trace.push(BusTransaction::write(cycle, 0xFFEC, 1)); // device
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0)); // channel
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, 1)); // start_addr=1 (MODE register)
    cycle += 1;
    let reg_values: [u16; 6] = [
        1,              // reg 1: MODE = Mode 1 (Character)
        attr,           // reg 2: attributes
        config_ptr,     // reg 3: config_ptr
        0,              // reg 4: plane = 0
        0,              // reg 5: scanline_begin = 0
        0,              // reg 6: scanline_end = 0 (= canvas height)
    ];
    for &val in &reg_values {
        trace.push(BusTransaction::write(cycle, 0xFFEC, (val >> 8) as u8));
        cycle += 1;
        trace.push(BusTransaction::write(cycle, 0xFFEC, (val & 0xFF) as u8));
        cycle += 1;
    }
    trace.push(BusTransaction::write(cycle, 0xFFEF, 0x01)); // trigger xreg
    cycle += 1;

    // Wait one frame then exit
    trace.push(BusTransaction::write(cycle + 200_000, 0xFFEF, 0xFF));

    trace
}

/// Generate a bus trace that programs Mode 3 with a test pattern.
///
/// The trace:
/// 1. Writes a Mode3Config struct to XRAM at address 0x0000 via ADDR0/RW0
/// 2. Writes pixel data at address 0x0100 via ADDR0/RW0
/// 3. Configures VGA via xreg: CANVAS, MODE=3, attr, config_ptr=0
/// 4. Exits after one frame worth of cycles
pub fn generate_test_trace(mode: TestMode) -> Vec<BusTransaction> {
    match mode {
        TestMode::Text1bpp320x240 | TestMode::Text8bpp320x240 => {
            return generate_mode1_test_trace(mode);
        }
        _ => {}
    }

    let mut trace = Vec::new();
    let mut cycle: u64 = 0;

    let config_ptr: u16 = 0x0000;
    let data_ptr: u16 = 0x0100;
    let (bmp_w, bmp_h) = mode.bitmap_size();
    let bpp = mode.bpp();

    // --- Step 1: Write Mode3Config to XRAM at config_ptr via ADDR0/RW0 ---
    trace.push(BusTransaction::write(cycle, 0xFFE6, (config_ptr & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFE7, (config_ptr >> 8) as u8));
    cycle += 1;

    // 14 bytes: x_wrap, y_wrap, x_pos, y_pos, width, height, data_ptr, palette_ptr
    let config_bytes: [u8; 14] = [
        0, 0,                                                      // x_wrap=false, y_wrap=false
        0, 0,                                                      // x_pos_px = 0
        0, 0,                                                      // y_pos_px = 0
        (bmp_w & 0xFF) as u8, (bmp_w >> 8) as u8,                // width_px (little-endian)
        (bmp_h & 0xFF) as u8, (bmp_h >> 8) as u8,                // height_px
        (data_ptr & 0xFF) as u8, (data_ptr >> 8) as u8,          // xram_data_ptr
        0, 0,                                                      // xram_palette_ptr = 0 (built-in)
    ];
    for &b in &config_bytes {
        trace.push(BusTransaction::write(cycle, 0xFFE4, b));
        cycle += 1;
    }

    // --- Step 2: Write pixel data ---
    trace.push(BusTransaction::write(cycle, 0xFFE6, (data_ptr & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFE7, (data_ptr >> 8) as u8));
    cycle += 1;

    let bytes_per_row = (bmp_w as u32 * bpp as u32).div_ceil(8);

    for y in 0..bmp_h as u32 {
        for byte_x in 0..bytes_per_row {
            let byte_val = pattern_byte(byte_x, y, bpp, bmp_w as u32);
            trace.push(BusTransaction::write(cycle, 0xFFE4, byte_val));
            cycle += 1;
        }
    }

    // --- Step 3: Configure VGA via xreg ---
    // Two separate xreg calls, matching SDK usage:
    //   xreg(1, 0, 0, canvas)          — sets CANVAS (reg 0), resets planes
    //   xreg(1, 0, 1, mode, attr, ...) — programs MODE (reg 1) with attrs (regs 2-6)

    // First xreg: CANVAS only (device=1, channel=0, start_addr=0)
    trace.push(BusTransaction::write(cycle, 0xFFEC, 1));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0));
    cycle += 1;
    let canvas = mode.canvas_reg();
    trace.push(BusTransaction::write(cycle, 0xFFEC, (canvas >> 8) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, (canvas & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEF, 0x01));
    cycle += 1;

    // Second xreg: MODE + attributes (device=1, channel=0, start_addr=1)
    trace.push(BusTransaction::write(cycle, 0xFFEC, 1));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, 1));
    cycle += 1;
    // Push register values: hi byte first (so lo lands at lower XRAM address = correct LE)
    let reg_values: [u16; 6] = [
        3,                  // reg 1: MODE = Mode 3
        mode.attr(),        // reg 2: attributes (color format)
        config_ptr,         // reg 3: config_ptr
        0,                  // reg 4: plane = 0
        0,                  // reg 5: scanline_begin = 0
        0,                  // reg 6: scanline_end = 0 (= canvas height)
    ];
    for &val in &reg_values {
        trace.push(BusTransaction::write(cycle, 0xFFEC, (val >> 8) as u8));
        cycle += 1;
        trace.push(BusTransaction::write(cycle, 0xFFEC, (val & 0xFF) as u8));
        cycle += 1;
    }
    trace.push(BusTransaction::write(cycle, 0xFFEF, 0x01));
    cycle += 1;

    // Wait one frame (phi2_freq=8MHz, 60fps -> ~133333 cycles) then exit
    trace.push(BusTransaction::write(cycle + 200_000, 0xFFEF, 0xFF));

    trace
}

/// Generate one byte of test pattern data at position (byte_x, y) in a bitmap.
///
/// Pixel packing follows Mode 3 MSB-first convention:
/// - 1bpp: bit 7 = pixel 0, ..., bit 0 = pixel 7 (checkerboard)
/// - 2bpp: bits[7:6] = pixel 0, ..., bits[1:0] = pixel 3 (4-color cycle)
/// - 4bpp: bits[7:4] = pixel 0, bits[3:0] = pixel 1 (16-color cycle)
/// - 8bpp: one full byte per pixel (256-color cycle)
/// - 16bpp: two bytes per pixel (PICO_SCANVIDEO format, alpha bit set)
fn pattern_byte(byte_x: u32, y: u32, bpp: u16, width: u32) -> u8 {
    match bpp {
        1 => {
            // Checkerboard: alternating on/off per pixel, row inverted
            let base_px = byte_x * 8;
            let mut byte = 0u8;
            for bit in 0..8u32 {
                let px = base_px + bit;
                if px < width && (px + y).is_multiple_of(2) {
                    byte |= 1 << (7 - bit); // MSB-first
                }
            }
            byte
        }
        2 => {
            // 4-color gradient: cycle (px+y) % 4
            let base_px = byte_x * 4;
            let mut byte = 0u8;
            for i in 0..4u32 {
                let px = base_px + i;
                if px < width {
                    let color = ((px + y) % 4) as u8;
                    byte |= color << (6 - i * 2); // MSB-first
                }
            }
            byte
        }
        4 => {
            // 16-color gradient: cycle (px+y) % 16
            let base_px = byte_x * 2;
            let mut byte = 0u8;
            for i in 0..2u32 {
                let px = base_px + i;
                if px < width {
                    let color = ((px + y) % 16) as u8;
                    if i == 0 {
                        byte |= color << 4; // high nibble = pixel 0
                    } else {
                        byte |= color;      // low nibble = pixel 1
                    }
                }
            }
            byte
        }
        8 => {
            // 256-color gradient: (px+y) % 256
            ((byte_x + y) % 256) as u8
        }
        16 => {
            // PICO_SCANVIDEO format: R5[4:0], alpha[5], G5[10:6], B5[15:11]
            // Alpha bit MUST be set for pixel to be visible.
            // byte_x counts individual bytes; each pixel is 2 bytes (little-endian).
            let px = byte_x / 2;
            let r5 = (px % 32) as u16;
            let g5 = (y % 32) as u16;
            let b5 = ((px + y) % 32) as u16;
            let alpha = 1u16 << 5;
            let color: u16 = (b5 << 11) | (g5 << 6) | alpha | r5;
            if byte_x.is_multiple_of(2) {
                (color & 0xFF) as u8  // low byte first (little-endian)
            } else {
                (color >> 8) as u8    // high byte second
            }
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_modes_produce_traces() {
        for mode in TestMode::all() {
            let trace = generate_test_trace(*mode);
            assert!(trace.len() > 100, "Mode {:?} produced too few transactions", mode);
        }
    }

    #[test]
    fn test_trace_ends_with_exit() {
        for mode in TestMode::all() {
            let trace = generate_test_trace(*mode);
            let last = trace.last().unwrap();
            assert_eq!(last.addr, 0xFFEF, "Mode {:?} missing exit", mode);
            assert_eq!(last.data, 0xFF, "Mode {:?} wrong exit opcode", mode);
        }
    }

    #[test]
    fn test_mono320x240_pixel_count() {
        let trace = generate_test_trace(TestMode::Mono320x240);
        // 320x240 at 1bpp = 320*240/8 = 9600 bytes of pixel data
        // Plus 14 bytes config written via RW0
        let rw0_writes = trace.iter().filter(|t| t.addr == 0xFFE4).count();
        assert_eq!(rw0_writes, 14 + 9600);
    }

    #[test]
    fn test_mono640x480_pixel_count() {
        let trace = generate_test_trace(TestMode::Mono640x480);
        // 640x480 at 1bpp = 640*480/8 = 38400 bytes
        let rw0_writes = trace.iter().filter(|t| t.addr == 0xFFE4).count();
        assert_eq!(rw0_writes, 14 + 38400);
    }

    #[test]
    fn test_mode_from_str() {
        assert!(matches!("mono640x480".parse::<TestMode>(), Ok(TestMode::Mono640x480)));
        assert!(matches!("color8bpp320x180".parse::<TestMode>(), Ok(TestMode::Color8bpp320x180)));
        assert!(matches!("color16bpp320".parse::<TestMode>(), Ok(TestMode::Color16bpp320)));
        assert!("invalid".parse::<TestMode>().is_err());
    }

    #[test]
    fn test_color16bpp_partial_height() {
        let trace = generate_test_trace(TestMode::Color16bpp320);
        // config_ptr = 0x0000, data_ptr = 0x0100
        // bytes_per_row = 320 * 2 = 640
        // max_rows = (65536 - 256) / 640 = 102
        let rw0_writes = trace.iter().filter(|t| t.addr == 0xFFE4).count();
        assert_eq!(rw0_writes, 14 + 102 * 640);
    }
}
