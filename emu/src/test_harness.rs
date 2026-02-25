use crate::bus::BusTransaction;
use crate::ria_api::{self, TraceBuilder};

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
    /// 320x240 canvas, Mode 3, 4bpp LSB-first, Mandelbrot set (matches pico-examples/src/mandelbrot.c)
    Mandelbrot,
    /// 320x240 canvas, two planes: Mode 3 1bpp checkerboard (plane 0) + Mode 1 8bpp rainbow text on right half (plane 1)
    MultiPlane,
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
            TestMode::Mandelbrot => "mandelbrot",
            TestMode::MultiPlane => "multi_plane",
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
            "mandelbrot" => Ok(TestMode::Mandelbrot),
            "multi_plane" => Ok(TestMode::MultiPlane),
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
            TestMode::Mandelbrot,
            TestMode::MultiPlane,
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
            TestMode::Text1bpp320x240 | TestMode::Text8bpp320x240
            | TestMode::Mandelbrot | TestMode::MultiPlane => unreachable!(),
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
            TestMode::Text1bpp320x240 | TestMode::Text8bpp320x240
            | TestMode::Mandelbrot | TestMode::MultiPlane => unreachable!(),
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
fn generate_mode1_test_trace(mode: TestMode) -> Vec<BusTransaction> {
    let mut tb = TraceBuilder::new();
    let config_ptr: u16 = 0x0000;
    let data_ptr: u16 = 0x0100;

    let (width_chars, height_chars, attr, cell_size): (i16, i16, u16, usize) = match mode {
        TestMode::Text1bpp320x240 => (40, 15, 8, 1),
        TestMode::Text8bpp320x240 => (40, 30, 3, 3),
        _ => panic!("Not a Mode 1 test mode"),
    };

    // --- Write Mode1Config fields to XRAM ---
    use ria_api::vga_mode1_config_t::*;
    tb.xram0_struct_set(config_ptr, X_WRAP, &[0]);
    tb.xram0_struct_set(config_ptr, Y_WRAP, &[0]);
    tb.xram0_struct_set(config_ptr, X_POS_PX, &0i16.to_le_bytes());
    tb.xram0_struct_set(config_ptr, Y_POS_PX, &0i16.to_le_bytes());
    tb.xram0_struct_set(config_ptr, WIDTH_CHARS, &width_chars.to_le_bytes());
    tb.xram0_struct_set(config_ptr, HEIGHT_CHARS, &height_chars.to_le_bytes());
    tb.xram0_struct_set(config_ptr, XRAM_DATA_PTR, &data_ptr.to_le_bytes());
    tb.xram0_struct_set(config_ptr, XRAM_PALETTE_PTR, &0xFFFFu16.to_le_bytes());
    tb.xram0_struct_set(config_ptr, XRAM_FONT_PTR, &0xFFFFu16.to_le_bytes());

    // --- Write character data ---
    tb.set_addr0(data_ptr);
    for row in 0..height_chars as u32 {
        for col in 0..width_chars as u32 {
            let glyph = 0x21 + ((row * width_chars as u32 + col) % 94) as u8;
            tb.write(0xFFE4, glyph);

            if cell_size >= 2 {
                let fg = (1 + (col % 15)) as u8;
                let bg = 16u8;
                tb.write(0xFFE4, fg);
                if cell_size >= 3 {
                    tb.write(0xFFE4, bg);
                }
            }
        }
    }

    // --- Configure VGA ---
    tb.xreg_vga_canvas(1); // 320x240
    tb.xreg_vga_mode(&[1, attr, config_ptr, 0, 0, 0]);

    tb.wait_frames(1);
    tb.op_exit();
    tb.trace
}

/// Generate a bus trace that renders the Mandelbrot set.
///
/// Mirrors pico-examples/src/mandelbrot.c exactly:
/// - 320x240 canvas (canvas reg 1), 2x pixel doubling
/// - Mode 3, 4bpp LSB-first (attr=10): low nibble = even pixel, high nibble = odd pixel
/// - Config at 0xFF00, pixel data at 0x0000
/// - Palette: 0xFFFF (built-in 256-color, uses ANSI indices 0-15)
/// - 16 Mandelbrot iterations, fixed-point arithmetic (12 frac bits)
fn generate_mandelbrot_test_trace() -> Vec<BusTransaction> {
    let mut tb = TraceBuilder::new();
    let config_ptr: u16 = 0xFF00;
    let data_ptr: u16 = 0x0000;

    // --- Write Mode3Config fields to XRAM at 0xFF00 ---
    use ria_api::vga_mode3_config_t::*;
    tb.xram0_struct_set(config_ptr, X_WRAP, &[0]);
    tb.xram0_struct_set(config_ptr, Y_WRAP, &[0]);
    tb.xram0_struct_set(config_ptr, X_POS_PX, &0i16.to_le_bytes());
    tb.xram0_struct_set(config_ptr, Y_POS_PX, &0i16.to_le_bytes());
    tb.xram0_struct_set(config_ptr, WIDTH_PX, &320i16.to_le_bytes());
    tb.xram0_struct_set(config_ptr, HEIGHT_PX, &240i16.to_le_bytes());
    tb.xram0_struct_set(config_ptr, XRAM_DATA_PTR, &data_ptr.to_le_bytes());
    tb.xram0_struct_set(config_ptr, XRAM_PALETTE_PTR, &0xFFFFu16.to_le_bytes());

    // --- Write pixel data at 0x0000 (4bpp LSB-first: 160 bytes/row, 38400 total) ---
    let mut pixel_data = Vec::with_capacity(160 * 240);
    for py in 0..240i32 {
        let mut vbyte: u8 = 0;
        for px in 0..320i32 {
            let color = mandelbrot_color(px, py);
            if px & 1 == 0 {
                vbyte = color;                    // even px: hold in low nibble
            } else {
                pixel_data.push(vbyte | (color << 4)); // odd px: pack high nibble, flush
            }
        }
    }
    tb.xram0_write(data_ptr, &pixel_data);

    // --- Configure VGA ---
    tb.xreg_vga_canvas(1);                              // 320x240
    tb.xreg_vga_mode(&[3, 10, config_ptr as u16, 0, 0, 0]); // attr=10 = Bpp4Lsb

    tb.wait_frames(1);
    tb.op_exit();
    tb.trace
}

/// Compute Mandelbrot color index (0-15) for pixel (px, py).
///
/// Matches the fixed-point algorithm in pico-examples/src/mandelbrot.c exactly.
/// FRAC_BITS=12, 16 max iterations. Color 0 = escaped quickly, 15 = inside set.
fn mandelbrot_color(px: i32, py: i32) -> u8 {
    const FRAC_BITS: i32 = 12;
    const WIDTH: i32 = 320;
    const HEIGHT: i32 = 240;
    // Fixed-point constants from FINT32(whole, frac) = (whole << 12) | (frac >> 4)
    let x0 = px * 12288 / WIDTH - 9216;  // range [-2.25, 0.75]
    let y0 = py * 9175 / HEIGHT - 4587;  // range [-1.12, +1.12]
    let mut x: i32 = 0;
    let mut y: i32 = 0;
    let mut iter: i32 = 0;
    while iter < 16 {
        let xx = (x * x) >> FRAC_BITS;
        let yy = (y * y) >> FRAC_BITS;
        if xx + yy > (4 << FRAC_BITS) {
            break;
        }
        let xtemp = xx - yy + x0;
        y = ((x * y) >> (FRAC_BITS - 1)) + y0;
        x = xtemp;
        iter += 1;
    }
    (iter.wrapping_sub(1) as u8) & 0x0F
}

/// Generate a bus trace exercising two VGA planes simultaneously.
///
/// XRAM layout:
///   0x0000: Mode3Config (14 bytes)
///   0x0020: 1bpp checkerboard pixel data (9600 bytes)
///   0x2600: Mode1Config (16 bytes)
///   0x2700: character data (20 * 30 * 3 = 1800 bytes)
///
/// Plane 0 (Mode 3, 1bpp MSB): full 320x240 canvas, 8x8 pixel checkerboard.
///   Palette[0] = transparent black, palette[1] = light grey (built-in PALETTE_2).
///
/// Plane 1 (Mode 1, 8bpp 8x8): 20 chars wide × 30 chars tall, positioned at x=160
///   so it occupies only the right half (pixels 160-319). Rainbow foreground colors
///   (bright ANSI 9-14 cycling per column), transparent background (palette index 0)
///   so the Mode 3 checkerboard shows through.
fn generate_multi_plane_test_trace() -> Vec<BusTransaction> {
    let mut tb = TraceBuilder::new();

    let m3_config_ptr: u16 = 0x0000;
    let m3_data_ptr: u16 = 0x0020;
    let m1_config_ptr: u16 = 0x2600;
    let m1_data_ptr: u16 = 0x2700;

    // --- Plane 0: Mode 3, 1bpp MSB, full-screen checkerboard ---
    {
        use ria_api::vga_mode3_config_t::*;
        tb.xram0_struct_set(m3_config_ptr, X_WRAP, &[0]);
        tb.xram0_struct_set(m3_config_ptr, Y_WRAP, &[0]);
        tb.xram0_struct_set(m3_config_ptr, X_POS_PX, &0i16.to_le_bytes());
        tb.xram0_struct_set(m3_config_ptr, Y_POS_PX, &0i16.to_le_bytes());
        tb.xram0_struct_set(m3_config_ptr, WIDTH_PX, &320i16.to_le_bytes());
        tb.xram0_struct_set(m3_config_ptr, HEIGHT_PX, &240i16.to_le_bytes());
        tb.xram0_struct_set(m3_config_ptr, XRAM_DATA_PTR, &m3_data_ptr.to_le_bytes());
        tb.xram0_struct_set(m3_config_ptr, XRAM_PALETTE_PTR, &0xFFFFu16.to_le_bytes());
    }

    // 1bpp MSB: each byte covers 8 pixels; 40 bytes/row, 240 rows = 9600 bytes.
    // 8x8 pixel squares: block_x = byte index, block_y = row / 8.
    // Checkerboard: if (block_x + block_y) is odd → 0xFF (grey), else 0x00 (transparent).
    let mut checkerboard = Vec::with_capacity(40 * 240);
    for y in 0..240u32 {
        let block_y = y / 8;
        for bx in 0..40u32 {
            checkerboard.push(if (bx + block_y) % 2 != 0 { 0xFFu8 } else { 0x00u8 });
        }
    }
    tb.xram0_write(m3_data_ptr, &checkerboard);

    // --- Plane 1: Mode 1, 8bpp 8x8, right half only ---
    // x_pos_px = 160 places the char grid at pixel 160 (right half starts here).
    // width_chars = 20 * 8px = 160px, covering pixels 160-319.
    let width_chars: i16 = 20;
    let height_chars: i16 = 30;
    {
        use ria_api::vga_mode1_config_t::*;
        tb.xram0_struct_set(m1_config_ptr, X_WRAP, &[0]);
        tb.xram0_struct_set(m1_config_ptr, Y_WRAP, &[0]);
        tb.xram0_struct_set(m1_config_ptr, X_POS_PX, &160i16.to_le_bytes());
        tb.xram0_struct_set(m1_config_ptr, Y_POS_PX, &0i16.to_le_bytes());
        tb.xram0_struct_set(m1_config_ptr, WIDTH_CHARS, &width_chars.to_le_bytes());
        tb.xram0_struct_set(m1_config_ptr, HEIGHT_CHARS, &height_chars.to_le_bytes());
        tb.xram0_struct_set(m1_config_ptr, XRAM_DATA_PTR, &m1_data_ptr.to_le_bytes());
        tb.xram0_struct_set(m1_config_ptr, XRAM_PALETTE_PTR, &0xFFFFu16.to_le_bytes());
        tb.xram0_struct_set(m1_config_ptr, XRAM_FONT_PTR, &0xFFFFu16.to_le_bytes());
    }

    // Character data: 3 bytes per cell [glyph, fg_index, bg_index].
    // fg cycles through bright ANSI colors per column for a rainbow effect:
    //   bright red(9), yellow(11), green(10), cyan(14), blue(12), magenta(13).
    // bg = 0: palette[0] is transparent in PALETTE_256, so the checkerboard shows through.
    let rainbow: [u8; 6] = [9, 11, 10, 14, 12, 13];
    let mut char_data = Vec::with_capacity(width_chars as usize * height_chars as usize * 3);
    for row in 0..height_chars as u32 {
        for col in 0..width_chars as u32 {
            let glyph = 0x21 + ((row * width_chars as u32 + col) % 94) as u8;
            char_data.push(glyph);
            char_data.push(rainbow[(col % 6) as usize]);
            char_data.push(0); // bg = transparent
        }
    }
    tb.xram0_write(m1_data_ptr, &char_data);

    // --- Configure VGA: canvas, then both planes ---
    tb.xreg_vga_canvas(1);                                      // 320x240
    tb.xreg_vga_mode(&[3, 0, m3_config_ptr, 0, 0, 0]);         // plane 0: Mode 3, 1bpp MSB
    tb.xreg_vga_mode(&[1, 3, m1_config_ptr, 1, 0, 0]);         // plane 1: Mode 1, 8bpp 8x8

    tb.wait_frames(1);
    tb.op_exit();
    tb.trace
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
        TestMode::Mandelbrot => {
            return generate_mandelbrot_test_trace();
        }
        TestMode::MultiPlane => {
            return generate_multi_plane_test_trace();
        }
        _ => {}
    }

    let mut tb = TraceBuilder::new();
    let config_ptr: u16 = 0x0000;
    let data_ptr: u16 = 0x0100;
    let (bmp_w, bmp_h) = mode.bitmap_size();
    let bpp = mode.bpp();

    // --- Write Mode3Config fields to XRAM ---
    use ria_api::vga_mode3_config_t::*;
    tb.xram0_struct_set(config_ptr, X_WRAP, &[0]);
    tb.xram0_struct_set(config_ptr, Y_WRAP, &[0]);
    tb.xram0_struct_set(config_ptr, X_POS_PX, &0i16.to_le_bytes());
    tb.xram0_struct_set(config_ptr, Y_POS_PX, &0i16.to_le_bytes());
    tb.xram0_struct_set(config_ptr, WIDTH_PX, &bmp_w.to_le_bytes());
    tb.xram0_struct_set(config_ptr, HEIGHT_PX, &bmp_h.to_le_bytes());
    tb.xram0_struct_set(config_ptr, XRAM_DATA_PTR, &data_ptr.to_le_bytes());
    tb.xram0_struct_set(config_ptr, XRAM_PALETTE_PTR, &0u16.to_le_bytes());

    // --- Write pixel data ---
    let bytes_per_row = (bmp_w as u32 * bpp as u32).div_ceil(8);
    let mut pixel_data = Vec::new();
    for y in 0..bmp_h as u32 {
        for byte_x in 0..bytes_per_row {
            pixel_data.push(pattern_byte(byte_x, y, bpp, bmp_w as u32));
        }
    }
    tb.xram0_write(data_ptr, &pixel_data);

    // --- Configure VGA ---
    tb.xreg_vga_canvas(mode.canvas_reg());
    tb.xreg_vga_mode(&[3, mode.attr(), config_ptr, 0, 0, 0]);

    tb.wait_frames(1);
    tb.op_exit();
    tb.trace
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
