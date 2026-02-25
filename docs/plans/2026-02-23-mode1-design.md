# Mode 1 (Character) Design

## Goal

Add Mode 1 (Character) rendering to the emulator, supporting all 5 color depth variants, both font sizes (8x8 and 8x16), with built-in CP437 IBM VGA fonts.

## Architecture

Mode 1 follows the same pattern as Mode 3: a dedicated `mode1.rs` module with its own config struct, format enum, and per-scanline renderer. The VGA state machine dispatches to Mode 1 or Mode 3 based on the MODE register value. A new `font.rs` module embeds the built-in CP437 font data.

## Module Layout

```
emu/src/vga/
├── mod.rs        (modify: Plane enum, program_mode1(), render dispatch)
├── mode1.rs      (new: Mode1Config, Mode1Plane, Mode1Format, render_mode1)
├── mode3.rs      (no changes)
├── font.rs       (new: FONT8[2048], FONT16[4096] CP437 data)
├── palette.rs    (no changes)
```

## Config Struct (16 bytes from XRAM)

Matches firmware `mode1_config_t`:

| Offset | Type    | Field            |
|--------|---------|------------------|
| 0      | bool    | x_wrap           |
| 1      | bool    | y_wrap           |
| 2      | i16     | x_pos_px         |
| 4      | i16     | y_pos_px         |
| 6      | i16     | width_chars      |
| 8      | i16     | height_chars     |
| 10     | u16     | xram_data_ptr    |
| 12     | u16     | xram_palette_ptr |
| 14     | u16     | xram_font_ptr    |

## Format Enum (10 attribute values)

Attribute encodes font size (bit 3) and color depth (bits 2:0):

| Attr | Font | Depth | Cell Size |
|------|------|-------|-----------|
| 0    | 8x8  | 1bpp  | 1 byte    |
| 1    | 8x8  | 4bpp-r| 2 bytes   |
| 2    | 8x8  | 4bpp  | 2 bytes   |
| 3    | 8x8  | 8bpp  | 3 bytes   |
| 4    | 8x8  | 16bpp | 6 bytes   |
| 8    | 8x16 | 1bpp  | 1 byte    |
| 9    | 8x16 | 4bpp-r| 2 bytes   |
| 10   | 8x16 | 4bpp  | 2 bytes   |
| 11   | 8x16 | 8bpp  | 3 bytes   |
| 12   | 8x16 | 16bpp | 6 bytes   |

Cell data formats:
- **1bpp**: `glyph_code` (1B) — palette[0]=bg, palette[1]=fg
- **4bpp-reversed**: `glyph_code, fg_bg_index` (2B) — high nibble=fg, low=bg
- **4bpp**: `glyph_code, bg_fg_index` (2B) — high nibble=bg, low=fg
- **8bpp**: `glyph_code, fg_index, bg_index` (3B)
- **16bpp**: `glyph_code, attributes, fg_color, bg_color` (6B) — colors are PICO_SCANVIDEO u16

## Rendering Algorithm

Per-scanline, matching firmware `mode1.c`:

1. Compute `row = scanline - y_pos_px`
2. Apply Y wrapping on `height_chars * font_height` pixels
3. Determine char row = `row / font_height`, get row data pointer from XRAM
4. Resolve font: bounds check `font_ptr`, fall back to built-in FONT8/FONT16
5. Font row byte = `font[256 * (row % font_height) + glyph_code]`
6. Compute `col = -x_pos_px`, apply X wrapping on `width_chars * 8` pixels
7. For each visible character:
   - Read cell data from XRAM
   - Resolve fg/bg colors (palette lookup or direct for 16bpp)
   - For each of 8 pixel bits (MSB first), write fg or bg to canvas

## Font Data

CP437 IBM VGA typeface (American English code page), embedded as const arrays:
- `FONT8: [u8; 2048]` — 8x8, 256 glyphs, "wide" format
- `FONT16: [u8; 4096]` — 8x16, 256 glyphs, "wide" format

"Wide" format: byte at `256 * row + glyph_code` gives the 8-pixel row for that glyph.

## Palette Resolution

Same logic as Mode 3:
- If `palette_ptr` is even and within bounds → custom palette from XRAM
- Else → PALETTE_2 (for 1bpp) or PALETTE_256 (for 4/8bpp)
- 16bpp uses direct PICO_SCANVIDEO colors via `rgb565_to_rgba()`

## VGA Integration

- `Plane` enum replaces `Option<Mode3Plane>`: `Plane::Mode1(Mode1Plane) | Plane::Mode3(Mode3Plane)`
- `planes: [Option<Plane>; 3]`
- `handle_reg` MODE=1 → `program_mode1()`, MODE=3 → `program_mode3()`
- `render_frame` matches plane variant for dispatch

## Test Harness

Add representative Mode 1 test modes:
- `text1bpp320x240`: 40x15 chars, 8x16 font, monochrome
- `text8bpp320x240`: 40x30 chars, 8x8 font, 256-color

## Out of Scope

- Code pages other than CP437
- Mode 0 (Console), Mode 2 (Tile), Mode 4 (Sprite)
- Custom font loading tests (font_ptr to XRAM)
