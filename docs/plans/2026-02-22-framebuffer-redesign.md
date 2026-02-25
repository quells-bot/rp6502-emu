# Framebuffer Redesign: Two-Stage Render with Pixel Doubling

## Problem

The current implementation renders Mode 3 directly at canvas resolution and passes a variable-size framebuffer to egui. The egui display size changes with the canvas setting, and there is no pixel doubling for 320-wide canvases. On real hardware, the VGA output is always 640x480 and 320-wide canvases are pixel-doubled 2x.

## Design

### Two-stage rendering pipeline

1. **Canvas buffer** — `Vec<u32>` allocated at 640*480 (max canvas size), reused across frames. Only the first `canvas_width * canvas_height` entries are used. `render_mode3` writes post-palette-lookup RGBA u32 values into this buffer at native canvas resolution.

2. **Display buffer** — always 640x480, stored as `Vec<u8>` (640 * 480 * 4 bytes). After all planes are composited into the canvas buffer, the VGA thread upscales into the display buffer with u32-to-u8 RGBA conversion in the same pass.

### Pixel doubling

Scale factors by canvas:

| Canvas | Resolution | X Scale | Y Scale | Notes |
|--------|-----------|---------|---------|-------|
| 1 | 320x240 | 2x | 2x | Full fill |
| 2 | 320x180 | 2x | 2x | Top-aligned, black below 360 |
| 3 | 640x480 | 1x | 1x | Direct copy |
| 4 | 640x360 | 1x | 1x | Top-aligned, black below 360 |

For 2x scale: each canvas pixel at (x, y) maps to a 2x2 block in the display buffer. For 1x: direct copy with conversion.

16:9 canvases are top-aligned with black filling the remaining scanlines below.

### Shared framebuffer to egui

Changes from `Arc<Mutex<(u32, u32, Vec<u8>)>>` to `Arc<Mutex<Vec<u8>>>`. Always 640x480. egui creates a fixed 640x480 texture.

## File changes

### `vga/mod.rs`

- `Vga` struct gains `canvas_buf: Vec<u32>` (allocated at 640*480, reused)
- `framebuffer` type becomes `Arc<Mutex<Vec<u8>>>`
- `render_frame` becomes two steps: (1) clear and render planes into `canvas_buf`, (2) upscale+convert into display `Vec<u8>`
- New helper for the upscale pass

### `main.rs`

- Shared framebuffer init changes from `(u32, u32, Vec<u8>)` to `Vec<u8>` pre-sized to 640*480*4
- egui texture uses fixed 640x480 dimensions

### `mode3.rs`

- No changes. `render_mode3` already takes `canvas_width`/`canvas_height` and writes u32 RGBA.

### `test_harness.rs`

- `generate_gradient_trace()` replaced by `generate_test_trace(mode: TestMode)`
- `TestMode` enum encodes valid canvas+bpp combinations (constrained by 64KB XRAM)

## TestMode enum

Valid full-screen combinations (bitmap fills entire canvas, fits in 64KB XRAM):

| Variant | Canvas | BPP | Bitmap | Data Size | Scale |
|---------|--------|-----|--------|-----------|-------|
| Mono640x480 | 640x480 | 1 | 640x480 | 37.5KB | 1x |
| Mono640x360 | 640x360 | 1 | 640x360 | 28.1KB | 1x |
| Mono320x240 | 320x240 | 1 | 320x240 | 9.4KB | 2x |
| Mono320x180 | 320x180 | 1 | 320x180 | 7KB | 2x |
| Color2bpp640x360 | 640x360 | 2 | 640x360 | 56.3KB | 1x |
| Color2bpp320x240 | 320x240 | 2 | 320x240 | 18.8KB | 2x |
| Color2bpp320x180 | 320x180 | 2 | 320x180 | 14.1KB | 2x |
| Color4bpp320x240 | 320x240 | 4 | 320x240 | 37.5KB | 2x |
| Color4bpp320x180 | 320x180 | 4 | 320x180 | 28.1KB | 2x |
| Color8bpp320x180 | 320x180 | 8 | 320x180 | 56.3KB | 2x |

Partial-screen 16bpp (max rows that fit at 320 wide):

| Variant | Canvas | BPP | Bitmap | Data Size | Scale |
|---------|--------|-----|--------|-----------|-------|
| Color16bpp320 | 320x240 | 16 | 320x102 | ~65KB | 2x, partial |

320 pixels * 2 bytes = 640 bytes/row. ~256 bytes reserved for config. (65536 - 256) / 640 = 102 rows.

### Test patterns

- 1bpp: checkerboard or stripe pattern
- 2/4/8bpp: gradient cycling through available palette colors
- 16bpp: direct color gradient
