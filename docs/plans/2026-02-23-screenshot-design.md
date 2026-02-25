# Headless Screenshot Feature

## Goal

Add a CLI subcommand to render a test pattern framebuffer as a PNG file without launching the egui window. Useful for documentation, debugging, and regression testing.

## CLI Interface

```
cargo run                                              # egui window (default, unchanged)
cargo run -- screenshot --mode mono320x240 -o out.png  # headless screenshot
```

The `screenshot` subcommand requires `--mode <name>` and `-o <path>` (or `--output <path>`).

Mode names map 1:1 to `TestMode` variants in snake_case: `mono640x480`, `mono640x360`, `mono320x240`, `mono320x180`, `color2bpp640x360`, `color2bpp320x240`, `color2bpp320x180`, `color4bpp320x240`, `color4bpp320x180`, `color8bpp320x180`, `color16bpp320`.

## Architecture

The headless path reuses the existing threaded RIA+VGA pipeline (required because the RIA blocks on backchannel responses for xreg ACK and VSYNC). The only difference from the GUI path is that no egui window is spawned.

```
[TestMode] -> generate_test_trace()
           -> spawn RIA thread (processes trace, exits when done)
           -> spawn VGA thread (processes PIX events)
           -> RIA thread finishes -> drop pix_tx -> VGA thread exits
           -> grab framebuffer Vec<u8> (640x480 RGBA)
           -> encode as PNG -> write to output path
```

## Dependencies

- `clap` (derive feature) — CLI argument parsing
- `png` — PNG encoding

## Files Changed

| File | Change |
|------|--------|
| `Cargo.toml` | Add `clap` and `png` dependencies |
| `src/main.rs` | Add clap CLI structure; default = egui, `screenshot` subcommand = headless pipeline |
| `src/test_harness.rs` | Add `FromStr` impl for `TestMode` to support clap parsing |
| `src/screenshot.rs` (new) | `save_png(path, &[u8], width, height)` — encodes RGBA buffer as PNG |

## Files Not Changed

`ria.rs`, `vga/mod.rs`, `vga/mode3.rs`, `vga/palette.rs` — the rendering pipeline is reused as-is.
