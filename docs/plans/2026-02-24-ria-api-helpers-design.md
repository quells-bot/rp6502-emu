# RIA API Test Harness Helpers

## Problem

The test harness (`emu/src/test_harness.rs`) manually constructs bus transactions byte-by-byte to simulate what 6502 programs do. This is verbose, error-prone, and hard to cross-reference against real C programs like `pico-examples/src/mode1.c`.

## Goal

Create helper functions that mirror the C API from `cc65/include/rp6502.h` so that:

1. Writing new test patterns is faster and uses the same mental model as the C SDK
2. Existing C examples provide a grounded input/output reference for correctness
3. The helpers are the single place to fix if the bus write sequence is wrong

## Design

### New file: `emu/src/ria_api.rs`

A pure helper module with no dependencies beyond `BusTransaction`.

### TraceBuilder

Wraps a `Vec<BusTransaction>` and a cycle counter. Each helper method appends transactions and advances the cycle automatically.

```rust
pub struct TraceBuilder {
    pub trace: Vec<BusTransaction>,
    pub cycle: u64,
}
```

### Core helpers (methods on TraceBuilder)

| Method | C equivalent | Behavior |
|---|---|---|
| `write(addr, data)` | `RIA.reg = val` | Append one write, advance cycle |
| `set_addr0(addr)` | `RIA.addr0 = addr` | Two writes to `$FFE6`, `$FFE7` |
| `set_step0(step)` | `RIA.step0 = step` | Write to `$FFE5` |
| `set_addr1(addr)` | `RIA.addr1 = addr` | Two writes to `$FFEA`, `$FFEB` |
| `set_step1(step)` | `RIA.step1 = step` | Write to `$FFE9` |
| `xram0_write(addr, &[u8])` | Sequential `RIA.rw0` writes | `set_addr0` then stream bytes to `$FFE4` |
| `xram0_struct_set(base, offset, &[u8])` | `xram0_struct_set(base, T, field, v)` | `set_addr0(base + offset)` then write bytes to `$FFE4` |
| `xreg(device, channel, addr, &[u16])` | `xreg(dev, ch, addr, ...)` | Push to xstack, write `$FFEF = 0x01` |
| `xreg_vga_canvas(value)` | `xreg_vga_canvas(v)` | `self.xreg(1, 0, 0, &[value])` |
| `xreg_vga_mode(&[u16])` | `xreg_vga_mode(...)` | `self.xreg(1, 0, 1, values)` |
| `op_exit()` | `RIA.op = 0xFF` | Write `$FFEF = 0xFF` |
| `wait_frames(n)` | Cycle padding | Advance cycle by `n * 200_000` (no transactions) |

### Struct offset constants

Modules named after the C types from `cc65/include/rp6502.h`:

```rust
pub mod vga_mode3_config_t {
    pub const X_WRAP: u16 = 0;          // bool (u8)
    pub const Y_WRAP: u16 = 1;          // bool (u8)
    pub const X_POS_PX: u16 = 2;        // i16
    pub const Y_POS_PX: u16 = 4;        // i16
    pub const WIDTH_PX: u16 = 6;        // i16
    pub const HEIGHT_PX: u16 = 8;       // i16
    pub const XRAM_DATA_PTR: u16 = 10;  // u16
    pub const XRAM_PALETTE_PTR: u16 = 12; // u16
    // total: 14 bytes
}

pub mod vga_mode1_config_t {
    pub const X_WRAP: u16 = 0;            // bool (u8)
    pub const Y_WRAP: u16 = 1;            // bool (u8)
    pub const X_POS_PX: u16 = 2;          // i16
    pub const Y_POS_PX: u16 = 4;          // i16
    pub const WIDTH_CHARS: u16 = 6;       // i16
    pub const HEIGHT_CHARS: u16 = 8;      // i16
    pub const XRAM_DATA_PTR: u16 = 10;    // u16
    pub const XRAM_PALETTE_PTR: u16 = 12; // u16
    pub const XRAM_FONT_PTR: u16 = 14;    // u16
    // total: 16 bytes
}
```

### Refactor existing test modes

Rewrite `generate_test_trace` and `generate_mode1_test_trace` to use `TraceBuilder`. The test pattern data generation (`pattern_byte`, character data loops) stays unchanged. Existing tests remain unchanged and validate that the refactor produces identical output.

### Example: refactored Mode 3 test

```rust
pub fn generate_test_trace(mode: TestMode) -> Vec<BusTransaction> {
    let mut tb = TraceBuilder::new();
    let config_ptr: u16 = 0x0000;
    let data_ptr: u16 = 0x0100;
    let (bmp_w, bmp_h) = mode.bitmap_size();

    // Write config via per-field struct_set (mirrors xram0_struct_set in C)
    use ria_api::vga_mode3_config_t::*;
    tb.xram0_struct_set(config_ptr, X_WRAP, &[0]);
    tb.xram0_struct_set(config_ptr, Y_WRAP, &[0]);
    tb.xram0_struct_set(config_ptr, X_POS_PX, &0i16.to_le_bytes());
    tb.xram0_struct_set(config_ptr, Y_POS_PX, &0i16.to_le_bytes());
    tb.xram0_struct_set(config_ptr, WIDTH_PX, &bmp_w.to_le_bytes());
    tb.xram0_struct_set(config_ptr, HEIGHT_PX, &bmp_h.to_le_bytes());
    tb.xram0_struct_set(config_ptr, XRAM_DATA_PTR, &data_ptr.to_le_bytes());
    tb.xram0_struct_set(config_ptr, XRAM_PALETTE_PTR, &0u16.to_le_bytes());

    // Write pixel data
    let pixel_data = generate_pixel_data(mode);
    tb.xram0_write(data_ptr, &pixel_data);

    // Configure VGA
    tb.xreg_vga_canvas(mode.canvas_reg());
    tb.xreg_vga_mode(&[3, mode.attr(), config_ptr, 0, 0, 0]);

    tb.wait_frames(1);
    tb.op_exit();
    tb.trace
}
```
