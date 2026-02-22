# Framebuffer Redesign Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Restructure the VGA rendering pipeline into a two-stage model where Mode 3 renders at canvas resolution into an internal buffer, then the VGA thread pixel-doubles into a fixed 640x480 display buffer for egui.

**Architecture:** The VGA thread owns two buffers: a `canvas_buf: Vec<u32>` (max 640*480, used at canvas resolution) and a display `Vec<u8>` (always 640x480 RGBA). Mode 3 renders into canvas_buf at native resolution. A new upscale pass pixel-doubles and converts to u8 RGBA. egui always receives 640x480.

**Tech Stack:** Rust, eframe/egui, crossbeam-channel

**Design doc:** `docs/plans/2026-02-22-framebuffer-redesign.md`

---

### Task 1: Change shared framebuffer type

Change the shared framebuffer from `Arc<Mutex<(u32, u32, Vec<u8>)>>` to `Arc<Mutex<Vec<u8>>>` in both `main.rs` and `vga/mod.rs`. The dimensions are always 640x480, so no need to carry them.

**Files:**
- Modify: `emu/src/main.rs:21-22` (framebuffer init)
- Modify: `emu/src/main.rs:60-62` (EmulatorApp struct)
- Modify: `emu/src/main.rs:71-75` (framebuffer snapshot in update)
- Modify: `emu/src/main.rs:77-79` (ColorImage creation)
- Modify: `emu/src/vga/mod.rs:18` (Vga struct field type)
- Modify: `emu/src/vga/mod.rs:23-26` (Vga::new signature and constructor)

**Step 1: Update `main.rs` framebuffer init and EmulatorApp**

Change the shared framebuffer type:

```rust
// main.rs line 20-22
// Shared framebuffer: always 640x480 RGBA bytes
let framebuffer: Arc<Mutex<Vec<u8>>> =
    Arc::new(Mutex::new(vec![0u8; 640 * 480 * 4]));
```

Update the struct:

```rust
// main.rs line 60-62
struct EmulatorApp {
    framebuffer: Arc<Mutex<Vec<u8>>>,
    texture: Option<egui::TextureHandle>,
}
```

Update the framebuffer snapshot in `update`:

```rust
// main.rs line 70-80
// Snapshot the framebuffer (always 640x480)
let pixels = if let Ok(fb) = self.framebuffer.lock() {
    fb.clone()
} else {
    vec![0u8; 640 * 480 * 4]
};

let image = egui::ColorImage::from_rgba_unmultiplied(
    [640, 480],
    &pixels,
);
```

**Step 2: Update `vga/mod.rs` framebuffer type**

```rust
// vga/mod.rs line 18
framebuffer: Arc<Mutex<Vec<u8>>>,
```

```rust
// vga/mod.rs Vga::new signature
pub fn new(
    pix_rx: Receiver<PixEvent>,
    backchannel_tx: Sender<Backchannel>,
    framebuffer: Arc<Mutex<Vec<u8>>>,
) -> Self {
```

Update `render_frame` to write directly (temporary — will be replaced in Task 2):

```rust
fn render_frame(&mut self) {
    let w = self.canvas_width;
    let h = self.canvas_height;
    let pixel_count = w as usize * h as usize;

    let mut fb_rgba = vec![0u32; pixel_count];

    for plane in self.planes.iter().flatten() {
        let fresh_config = Mode3Config::from_xram(&self.xram, plane.config_ptr);
        let current_plane = Mode3Plane { config: fresh_config, ..plane.clone() };
        render_mode3(&current_plane, &self.xram, &mut fb_rgba, w, h);
    }

    // Convert and write to fixed 640x480 display buffer
    // For now: 1:1 copy into top-left, rest stays black
    let mut display = vec![0u8; 640 * 480 * 4];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let src = y * w as usize + x;
            let dst = y * 640 + x;
            let pixel = fb_rgba[src];
            display[dst * 4]     = (pixel >> 24) as u8;
            display[dst * 4 + 1] = (pixel >> 16) as u8;
            display[dst * 4 + 2] = (pixel >> 8)  as u8;
            display[dst * 4 + 3] = (pixel & 0xFF) as u8;
        }
    }

    if let Ok(mut fb) = self.framebuffer.lock() {
        *fb = display;
    }
}
```

**Step 3: Build and verify**

Run: `cargo build --manifest-path emu/Cargo.toml 2>&1`
Expected: Compiles successfully.

Run: `cargo test --manifest-path emu/Cargo.toml 2>&1`
Expected: All existing tests pass.

**Step 4: Commit**

```bash
git add emu/src/main.rs emu/src/vga/mod.rs
git commit -m "refactor: change shared framebuffer to fixed 640x480 Vec<u8>"
```

---

### Task 2: Add canvas buffer and upscale pass

Add `canvas_buf: Vec<u32>` to the Vga struct and implement the two-stage render: render planes into canvas_buf, then upscale+convert to display buffer.

**Files:**
- Modify: `emu/src/vga/mod.rs` (Vga struct, new, render_frame)

**Step 1: Write the upscale test**

Add to `emu/src/vga/mod.rs` at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upscale_1x() {
        // 640x480 canvas -> 1x scale, direct copy
        let mut canvas = vec![0u32; 640 * 480];
        // Set pixel (0,0) to red
        canvas[0] = 0xFF0000FF; // R=FF, G=00, B=00, A=FF
        // Set pixel (639,479) to green
        canvas[639 + 479 * 640] = 0x00FF00FF;

        let mut display = vec![0u8; 640 * 480 * 4];
        upscale_canvas(&canvas, 640, 480, &mut display);

        // Check (0,0)
        assert_eq!(display[0], 0xFF); // R
        assert_eq!(display[1], 0x00); // G
        assert_eq!(display[2], 0x00); // B
        assert_eq!(display[3], 0xFF); // A

        // Check (639,479)
        let idx = (639 + 479 * 640) * 4;
        assert_eq!(display[idx], 0x00);
        assert_eq!(display[idx + 1], 0xFF);
        assert_eq!(display[idx + 2], 0x00);
        assert_eq!(display[idx + 3], 0xFF);
    }

    #[test]
    fn test_upscale_2x() {
        // 320x240 canvas -> 2x scale
        let mut canvas = vec![0u32; 320 * 240];
        // Set pixel (0,0) to blue
        canvas[0] = 0x0000FFFF;
        // Set pixel (1,0) to red
        canvas[1] = 0xFF0000FF;

        let mut display = vec![0u8; 640 * 480 * 4];
        upscale_canvas(&canvas, 320, 240, &mut display);

        // (0,0) in canvas maps to (0,0), (1,0), (0,1), (1,1) in display
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let idx = (dx + dy * 640) * 4;
            assert_eq!(display[idx], 0x00, "R at ({dx},{dy})");
            assert_eq!(display[idx + 1], 0x00, "G at ({dx},{dy})");
            assert_eq!(display[idx + 2], 0xFF, "B at ({dx},{dy})");
            assert_eq!(display[idx + 3], 0xFF, "A at ({dx},{dy})");
        }

        // (1,0) in canvas maps to (2,0), (3,0), (2,1), (3,1) in display
        for (dx, dy) in [(2, 0), (3, 0), (2, 1), (3, 1)] {
            let idx = (dx + dy * 640) * 4;
            assert_eq!(display[idx], 0xFF, "R at ({dx},{dy})");
            assert_eq!(display[idx + 1], 0x00, "G at ({dx},{dy})");
            assert_eq!(display[idx + 2], 0x00, "B at ({dx},{dy})");
            assert_eq!(display[idx + 3], 0xFF, "A at ({dx},{dy})");
        }
    }

    #[test]
    fn test_upscale_2x_16_9_black_below() {
        // 320x180 canvas -> 2x scale, content occupies 640x360, black below
        let mut canvas = vec![0u32; 320 * 180];
        canvas[0] = 0xFF0000FF; // red pixel at (0,0)

        let mut display = vec![0u8; 640 * 480 * 4];
        upscale_canvas(&canvas, 320, 180, &mut display);

        // (0,0) should be red (2x2 block)
        assert_eq!(display[0], 0xFF);
        assert_eq!(display[3], 0xFF);

        // Scanline 360 (first line below content) should be black
        let idx = 360 * 640 * 4;
        assert_eq!(display[idx], 0x00);
        assert_eq!(display[idx + 1], 0x00);
        assert_eq!(display[idx + 2], 0x00);
        assert_eq!(display[idx + 3], 0x00);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path emu/Cargo.toml vga::tests 2>&1`
Expected: FAIL — `upscale_canvas` not found.

**Step 3: Implement upscale_canvas and refactor render_frame**

Add the `upscale_canvas` function and `canvas_buf` field:

```rust
// Add to Vga struct
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
    canvas_buf: Vec<u32>,
}
```

Initialize in `new`:

```rust
Self {
    // ... existing fields ...
    canvas_buf: vec![0u32; 640 * 480],
}
```

Add the upscale function (module-level, not a method, so tests can call it):

```rust
/// Display is always 640x480.
const DISPLAY_WIDTH: usize = 640;
const DISPLAY_HEIGHT: usize = 480;

/// Upscale canvas buffer to 640x480 display buffer.
///
/// Computes integer scale factors from canvas dimensions.
/// 320-wide canvases get 2x, 640-wide get 1x.
/// 16:9 canvases (180 or 360 tall) are top-aligned with black below.
fn upscale_canvas(canvas: &[u32], canvas_w: u16, canvas_h: u16, display: &mut [u8]) {
    let cw = canvas_w as usize;
    let ch = canvas_h as usize;
    let sx = DISPLAY_WIDTH / cw;
    let sy = DISPLAY_HEIGHT / ch.max(1);
    // For 16:9: sy = 480/180 = 2 (360 scaled lines), or 480/360 = 1 (360 lines)
    // Content height in display pixels
    let content_h = ch * sy;

    // Clear entire display to black
    display.fill(0);

    for cy in 0..ch {
        for cx in 0..cw {
            let pixel = canvas[cy * cw + cx];
            let r = (pixel >> 24) as u8;
            let g = (pixel >> 16) as u8;
            let b = (pixel >> 8) as u8;
            let a = (pixel & 0xFF) as u8;

            for dy in 0..sy {
                let display_y = cy * sy + dy;
                if display_y >= DISPLAY_HEIGHT {
                    break;
                }
                for dx in 0..sx {
                    let display_x = cx * sx + dx;
                    if display_x >= DISPLAY_WIDTH {
                        break;
                    }
                    let idx = (display_y * DISPLAY_WIDTH + display_x) * 4;
                    display[idx]     = r;
                    display[idx + 1] = g;
                    display[idx + 2] = b;
                    display[idx + 3] = a;
                }
            }
        }
    }
}
```

Update `render_frame`:

```rust
fn render_frame(&mut self) {
    let w = self.canvas_width;
    let h = self.canvas_height;
    let pixel_count = w as usize * h as usize;

    // Clear canvas buffer (only the used portion)
    self.canvas_buf[..pixel_count].fill(0);

    // Render each plane into canvas buffer
    for plane in self.planes.iter().flatten() {
        let fresh_config = Mode3Config::from_xram(&self.xram, plane.config_ptr);
        let current_plane = Mode3Plane { config: fresh_config, ..plane.clone() };
        render_mode3(&current_plane, &self.xram, &mut self.canvas_buf[..pixel_count], w, h);
    }

    // Upscale canvas to 640x480 display buffer
    let mut display = vec![0u8; DISPLAY_WIDTH * DISPLAY_HEIGHT * 4];
    upscale_canvas(&self.canvas_buf[..pixel_count], w, h, &mut display);

    if let Ok(mut fb) = self.framebuffer.lock() {
        *fb = display;
    }
}
```

**Step 4: Run tests**

Run: `cargo test --manifest-path emu/Cargo.toml 2>&1`
Expected: All tests pass (new upscale tests + existing tests).

**Step 5: Commit**

```bash
git add emu/src/vga/mod.rs
git commit -m "feat: add canvas buffer and pixel-doubling upscale pass"
```

---

### Task 3: Add TestMode enum and refactor test harness

Replace `generate_gradient_trace()` with `generate_test_trace(mode: TestMode)` supporting all valid canvas+bpp combinations.

**Files:**
- Modify: `emu/src/test_harness.rs` (full rewrite)
- Modify: `emu/src/main.rs:38` (call site)

**Step 1: Write tests for the new test harness**

Add to the bottom of `emu/src/test_harness.rs`, replacing the existing test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_modes_produce_traces() {
        for mode in TestMode::all() {
            let trace = generate_test_trace(mode);
            assert!(trace.len() > 100, "Mode {:?} produced too few transactions", mode);
        }
    }

    #[test]
    fn test_trace_ends_with_exit() {
        for mode in TestMode::all() {
            let trace = generate_test_trace(mode);
            let last = trace.last().unwrap();
            assert_eq!(last.addr, 0xFFEF, "Mode {:?} missing exit", mode);
            assert_eq!(last.data, 0xFF, "Mode {:?} wrong exit opcode", mode);
        }
    }

    #[test]
    fn test_mono320x240_pixel_count() {
        let trace = generate_test_trace(TestMode::Mono320x240);
        // 320*240 / 8 = 9600 bytes of pixel data, plus overhead
        let rw0_writes = trace.iter().filter(|t| t.addr == 0xFFE4).count();
        // 14 bytes config + 9600 bytes pixel data = 9614 RW0 writes
        assert_eq!(rw0_writes, 14 + 9600);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path emu/Cargo.toml test_harness 2>&1`
Expected: FAIL — `TestMode` not found.

**Step 3: Implement TestMode enum and generate_test_trace**

```rust
use crate::bus::BusTransaction;

/// Valid canvas + color depth combinations that fit in 64KB XRAM.
#[derive(Debug, Clone, Copy)]
pub enum TestMode {
    Mono640x480,
    Mono640x360,
    Mono320x240,
    Mono320x180,
    Color2bpp640x360,
    Color2bpp320x240,
    Color2bpp320x180,
    Color4bpp320x240,
    Color4bpp320x180,
    Color8bpp320x180,
    Color16bpp320,
}

impl TestMode {
    /// All valid modes for iteration.
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
        ]
    }

    /// Canvas register value (1-4).
    fn canvas(&self) -> u16 {
        match self {
            TestMode::Mono320x240 | TestMode::Color2bpp320x240
            | TestMode::Color4bpp320x240 | TestMode::Color16bpp320 => 1,
            TestMode::Mono320x180 | TestMode::Color2bpp320x180
            | TestMode::Color4bpp320x180 | TestMode::Color8bpp320x180 => 2,
            TestMode::Mono640x480 => 3,
            TestMode::Mono640x360 | TestMode::Color2bpp640x360 => 4,
        }
    }

    /// Canvas pixel dimensions.
    fn canvas_size(&self) -> (i16, i16) {
        match self.canvas() {
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
        }
    }

    /// Mode 3 attribute value (color format index).
    fn attr(&self) -> u16 {
        match self.bpp() {
            1 => 0,  // Bpp1Msb
            2 => 1,  // Bpp2Msb
            4 => 2,  // Bpp4Msb
            8 => 3,  // Bpp8
            16 => 4, // Bpp16
            _ => unreachable!(),
        }
    }

    /// Bitmap dimensions. For 16bpp partial screen, height is limited by XRAM.
    fn bitmap_size(&self) -> (i16, i16) {
        let (w, h) = self.canvas_size();
        if self.bpp() == 16 {
            let config_reserve = 256u32;
            let bytes_per_row = w as u32 * 2;
            let max_rows = (65536 - config_reserve) / bytes_per_row;
            (w, max_rows as i16)
        } else {
            (w, h)
        }
    }
}

/// Generate a bus trace that sets up a test pattern in Mode 3.
pub fn generate_test_trace(mode: TestMode) -> Vec<BusTransaction> {
    let mut trace = Vec::new();
    let mut cycle: u64 = 0;

    let config_ptr: u16 = 0x0000;
    let data_ptr: u16 = 0x0100;
    let (bmp_w, bmp_h) = mode.bitmap_size();
    let bpp = mode.bpp();

    // --- Step 1: Write Mode3Config to XRAM via ADDR0/RW0 ---
    // Set ADDR0 to config_ptr
    trace.push(BusTransaction::write(cycle, 0xFFE6, (config_ptr & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFE7, (config_ptr >> 8) as u8));
    cycle += 1;

    // Write 14 bytes of Mode3Config via RW0
    let config_bytes: Vec<u8> = vec![
        0, 0,                                                      // x_wrap=false, y_wrap=false
        0, 0,                                                      // x_pos_px = 0
        0, 0,                                                      // y_pos_px = 0
        (bmp_w & 0xFF) as u8, (bmp_w >> 8) as u8,                // width_px
        (bmp_h & 0xFF) as u8, (bmp_h >> 8) as u8,                // height_px
        (data_ptr & 0xFF) as u8, (data_ptr >> 8) as u8,          // xram_data_ptr
        0, 0,                                                      // xram_palette_ptr = 0 (default)
    ];
    for &b in &config_bytes {
        trace.push(BusTransaction::write(cycle, 0xFFE4, b));
        cycle += 1;
    }

    // --- Step 2: Write pixel data ---
    // Set ADDR0 to data_ptr
    trace.push(BusTransaction::write(cycle, 0xFFE6, (data_ptr & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFE7, (data_ptr >> 8) as u8));
    cycle += 1;

    let bytes_per_row = ((bmp_w as u32 * bpp as u32) + 7) / 8;

    for y in 0..bmp_h as u32 {
        for byte_x in 0..bytes_per_row {
            let byte_val = generate_pattern_byte(byte_x, y, bpp, bmp_w as u32);
            trace.push(BusTransaction::write(cycle, 0xFFE4, byte_val));
            cycle += 1;
        }
    }

    // --- Step 3: Configure VGA via xreg ---
    // Push device=1, channel=0, start_addr=0
    trace.push(BusTransaction::write(cycle, 0xFFEC, 1));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0));
    cycle += 1;

    let reg_values: &[u16] = &[
        mode.canvas(),    // reg 0: CANVAS
        3,                // reg 1: MODE = Mode 3
        mode.attr(),      // reg 2: attributes
        config_ptr,       // reg 3: config_ptr
        0,                // reg 4: plane = 0
        0,                // reg 5: scanline_begin = 0
        0,                // reg 6: scanline_end = 0 (= canvas height)
    ];
    for &val in reg_values {
        trace.push(BusTransaction::write(cycle, 0xFFEC, (val >> 8) as u8));
        cycle += 1;
        trace.push(BusTransaction::write(cycle, 0xFFEC, (val & 0xFF) as u8));
        cycle += 1;
    }

    // Trigger xreg: OP = 0x01
    trace.push(BusTransaction::write(cycle, 0xFFEF, 0x01));
    cycle += 1;

    // Wait one frame then exit
    trace.push(BusTransaction::write(cycle + 200_000, 0xFFEF, 0xFF));

    trace
}

/// Generate a test pattern byte for a given position and color depth.
fn generate_pattern_byte(byte_x: u32, y: u32, bpp: u16, width: u32) -> u8 {
    match bpp {
        1 => {
            // Checkerboard: alternating 1/0 per pixel, invert each row
            let base_pixel = byte_x * 8;
            let mut byte = 0u8;
            for bit in 0..8 {
                let px = base_pixel + bit;
                if px < width {
                    // MSB-first: bit 7 = pixel 0 of this byte
                    let on = ((px + y) % 2) == 0;
                    if on {
                        byte |= 1 << (7 - bit);
                    }
                }
            }
            byte
        }
        2 => {
            // Gradient: cycle through 4 colors
            let base_pixel = byte_x * 4;
            let mut byte = 0u8;
            for px_in_byte in 0..4 {
                let px = base_pixel + px_in_byte;
                if px < width {
                    let color = ((px + y) % 4) as u8;
                    // MSB-first: bits[7:6] = pixel 0
                    byte |= color << (6 - px_in_byte * 2);
                }
            }
            byte
        }
        4 => {
            // Gradient: cycle through 16 colors
            let base_pixel = byte_x * 2;
            let mut byte = 0u8;
            for px_in_byte in 0..2 {
                let px = base_pixel + px_in_byte;
                if px < width {
                    let color = ((px + y) % 16) as u8;
                    // MSB-first: high nibble = pixel 0
                    if px_in_byte == 0 {
                        byte |= color << 4;
                    } else {
                        byte |= color;
                    }
                }
            }
            byte
        }
        8 => {
            // Gradient: cycle through 256 colors
            let px = byte_x;
            ((px + y) % 256) as u8
        }
        16 => {
            // Direct color: two bytes per pixel, this function returns one byte at a time.
            // byte_x indexes individual bytes. Even bytes = low byte, odd = high byte.
            let px = byte_x / 2;
            let r5 = ((px * 8 / width.max(1)) & 0x1F) as u16;
            let g5 = ((y * 8 / 240) & 0x1F) as u16;
            let b5 = (((px + y) * 8 / (width.max(1) + 240)) & 0x1F) as u16;
            let alpha = 1u16 << 5;
            let color = (b5 << 11) | (g5 << 6) | alpha | r5;
            if byte_x % 2 == 0 {
                (color & 0xFF) as u8
            } else {
                (color >> 8) as u8
            }
        }
        _ => 0,
    }
}
```

**Step 4: Update `main.rs` call site**

```rust
// main.rs line 38
let trace = test_harness::generate_test_trace(test_harness::TestMode::Mono320x240);
```

**Step 5: Run tests**

Run: `cargo test --manifest-path emu/Cargo.toml 2>&1`
Expected: All tests pass.

**Step 6: Commit**

```bash
git add emu/src/test_harness.rs emu/src/main.rs
git commit -m "feat: add TestMode enum with all valid canvas+bpp combinations"
```

---

### Task 4: Visual verification and cleanup

Run the emulator to visually verify the pixel-doubled output looks correct.

**Files:**
- No code changes expected unless issues are found

**Step 1: Run with Mono320x240 (2x doubling)**

Run: `cargo run --manifest-path emu/Cargo.toml --release`
Expected: A 640x480 window showing a black-and-white checkerboard filling the entire display, with pixels visibly doubled (each checker square is 2x2 display pixels).

**Step 2: Verify all tests pass**

Run: `cargo test --manifest-path emu/Cargo.toml 2>&1`
Expected: All tests pass.

**Step 3: Run clippy**

Run: `cargo clippy --manifest-path emu/Cargo.toml 2>&1`
Expected: No warnings.

**Step 4: Update CLAUDE.md if needed**

Update the shared framebuffer type description and test harness description in CLAUDE.md to reflect the new design.

**Step 5: Commit any cleanup**

```bash
git add -A
git commit -m "chore: visual verification and cleanup after framebuffer redesign"
```
