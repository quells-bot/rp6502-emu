# RIA API Test Harness Helpers — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Create C-mirror helper functions for the test harness that match the `rp6502.h` API, then refactor existing test modes to use them.

**Architecture:** New module `emu/src/ria_api.rs` with a `TraceBuilder` struct and methods mirroring the C SDK. Struct field offsets as constants in submodules named after C types. Existing `test_harness.rs` refactored to use the helpers.

**Tech Stack:** Rust, no new dependencies.

---

### Task 1: Create `ria_api.rs` with TraceBuilder and primitive helpers

**Files:**
- Create: `emu/src/ria_api.rs`
- Modify: `emu/src/main.rs:1` (add `mod ria_api;`)

**Step 1: Write failing test for TraceBuilder::write**

Add to `emu/src/ria_api.rs`:

```rust
use crate::bus::BusTransaction;

pub struct TraceBuilder {
    pub trace: Vec<BusTransaction>,
    pub cycle: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_appends_transaction() {
        let mut tb = TraceBuilder { trace: Vec::new(), cycle: 0 };
        tb.write(0xFFE4, 0x42);
        assert_eq!(tb.trace.len(), 1);
        assert_eq!(tb.trace[0], BusTransaction::write(0, 0xFFE4, 0x42));
        assert_eq!(tb.cycle, 1);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p rp6502-emu test_write_appends_transaction -- --nocapture`
Expected: FAIL — `write` method not defined on `TraceBuilder`

**Step 3: Implement TraceBuilder::new and TraceBuilder::write**

```rust
impl TraceBuilder {
    pub fn new() -> Self {
        Self { trace: Vec::new(), cycle: 0 }
    }

    /// Single bus write — mirrors `RIA.reg = val`.
    pub fn write(&mut self, addr: u16, data: u8) {
        self.trace.push(BusTransaction::write(self.cycle, addr, data));
        self.cycle += 1;
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p rp6502-emu test_write_appends_transaction -- --nocapture`
Expected: PASS

**Step 5: Add `mod ria_api;` to main.rs**

In `emu/src/main.rs`, add after line 5 (`mod test_harness;`):

```rust
mod ria_api;
```

**Step 6: Run full test suite**

Run: `cargo test -p rp6502-emu`
Expected: All existing tests pass + new test passes.

**Step 7: Commit**

```bash
git add emu/src/ria_api.rs emu/src/main.rs
git commit -m "feat: add ria_api module with TraceBuilder::write"
```

---

### Task 2: Add register helpers (set_addr0, set_step0, etc.)

**Files:**
- Modify: `emu/src/ria_api.rs`

**Step 1: Write failing tests**

```rust
#[test]
fn test_set_addr0() {
    let mut tb = TraceBuilder::new();
    tb.set_addr0(0x1234);
    assert_eq!(tb.trace.len(), 2);
    assert_eq!(tb.trace[0], BusTransaction::write(0, 0xFFE6, 0x34)); // lo
    assert_eq!(tb.trace[1], BusTransaction::write(1, 0xFFE7, 0x12)); // hi
}

#[test]
fn test_set_step0() {
    let mut tb = TraceBuilder::new();
    tb.set_step0(-1);
    assert_eq!(tb.trace.len(), 1);
    assert_eq!(tb.trace[0], BusTransaction::write(0, 0xFFE5, 0xFF)); // -1 as u8
}

#[test]
fn test_set_addr1() {
    let mut tb = TraceBuilder::new();
    tb.set_addr1(0xABCD);
    assert_eq!(tb.trace.len(), 2);
    assert_eq!(tb.trace[0], BusTransaction::write(0, 0xFFEA, 0xCD));
    assert_eq!(tb.trace[1], BusTransaction::write(1, 0xFFEB, 0xAB));
}

#[test]
fn test_set_step1() {
    let mut tb = TraceBuilder::new();
    tb.set_step1(2);
    assert_eq!(tb.trace.len(), 1);
    assert_eq!(tb.trace[0], BusTransaction::write(0, 0xFFE9, 2));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p rp6502-emu test_set_addr -- --nocapture`
Expected: FAIL — methods not defined

**Step 3: Implement register helpers**

```rust
/// Set XRAM portal 0 address — mirrors `RIA.addr0 = addr`.
pub fn set_addr0(&mut self, addr: u16) {
    self.write(0xFFE6, (addr & 0xFF) as u8);
    self.write(0xFFE7, (addr >> 8) as u8);
}

/// Set XRAM portal 0 step — mirrors `RIA.step0 = step`.
pub fn set_step0(&mut self, step: i8) {
    self.write(0xFFE5, step as u8);
}

/// Set XRAM portal 1 address — mirrors `RIA.addr1 = addr`.
pub fn set_addr1(&mut self, addr: u16) {
    self.write(0xFFEA, (addr & 0xFF) as u8);
    self.write(0xFFEB, (addr >> 8) as u8);
}

/// Set XRAM portal 1 step — mirrors `RIA.step1 = step`.
pub fn set_step1(&mut self, step: i8) {
    self.write(0xFFE9, step as u8);
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p rp6502-emu test_set_ -- --nocapture`
Expected: All 4 new tests PASS

**Step 5: Commit**

```bash
git add emu/src/ria_api.rs
git commit -m "feat: add TraceBuilder register helpers (addr0/1, step0/1)"
```

---

### Task 3: Add xram0_write, xram0_struct_set, and op helpers

**Files:**
- Modify: `emu/src/ria_api.rs`

**Step 1: Write failing tests**

```rust
#[test]
fn test_xram0_write() {
    let mut tb = TraceBuilder::new();
    tb.xram0_write(0x0100, &[0xAA, 0xBB, 0xCC]);
    // 2 addr writes + 3 data writes = 5
    assert_eq!(tb.trace.len(), 5);
    assert_eq!(tb.trace[0], BusTransaction::write(0, 0xFFE6, 0x00)); // addr lo
    assert_eq!(tb.trace[1], BusTransaction::write(1, 0xFFE7, 0x01)); // addr hi
    assert_eq!(tb.trace[2], BusTransaction::write(2, 0xFFE4, 0xAA));
    assert_eq!(tb.trace[3], BusTransaction::write(3, 0xFFE4, 0xBB));
    assert_eq!(tb.trace[4], BusTransaction::write(4, 0xFFE4, 0xCC));
}

#[test]
fn test_xram0_struct_set() {
    let mut tb = TraceBuilder::new();
    tb.xram0_struct_set(0xFF00, 6, &42i16.to_le_bytes());
    // addr0 = 0xFF00 + 6 = 0xFF06, then 2 bytes through RW0
    assert_eq!(tb.trace.len(), 4);
    assert_eq!(tb.trace[0], BusTransaction::write(0, 0xFFE6, 0x06)); // lo of 0xFF06
    assert_eq!(tb.trace[1], BusTransaction::write(1, 0xFFE7, 0xFF)); // hi of 0xFF06
    assert_eq!(tb.trace[2], BusTransaction::write(2, 0xFFE4, 42));   // lo byte of 42
    assert_eq!(tb.trace[3], BusTransaction::write(3, 0xFFE4, 0));    // hi byte of 42
}

#[test]
fn test_op_exit() {
    let mut tb = TraceBuilder::new();
    tb.op_exit();
    assert_eq!(tb.trace.len(), 1);
    assert_eq!(tb.trace[0], BusTransaction::write(0, 0xFFEF, 0xFF));
}

#[test]
fn test_wait_frames() {
    let mut tb = TraceBuilder::new();
    tb.write(0xFFE4, 0x00); // cycle 0 -> 1
    tb.wait_frames(2);
    assert_eq!(tb.cycle, 1 + 400_000);
    tb.write(0xFFE4, 0x01);
    assert_eq!(tb.trace.last().unwrap().cycle, 1 + 400_000);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p rp6502-emu test_xram0_ test_op_exit test_wait_frames -- --nocapture`
Expected: FAIL

**Step 3: Implement**

```rust
/// Write bytes to XRAM via portal 0 — mirrors sequential `RIA.rw0` writes.
/// Sets addr0 first, then streams data bytes.
pub fn xram0_write(&mut self, addr: u16, data: &[u8]) {
    self.set_addr0(addr);
    for &b in data {
        self.write(0xFFE4, b);
    }
}

/// Write a struct field to XRAM via portal 0 — mirrors `xram0_struct_set(base, T, field, val)`.
/// Sets addr0 to base + offset, then writes value bytes.
pub fn xram0_struct_set(&mut self, base: u16, offset: u16, val: &[u8]) {
    self.set_addr0(base.wrapping_add(offset));
    for &b in val {
        self.write(0xFFE4, b);
    }
}

/// Trigger exit — mirrors `RIA.op = 0xFF`.
pub fn op_exit(&mut self) {
    self.write(0xFFEF, 0xFF);
}

/// Advance cycle counter without emitting transactions.
/// Useful for waiting N frames (~200,000 cycles each at 8MHz/60fps).
pub fn wait_frames(&mut self, n: u32) {
    self.cycle += n as u64 * 200_000;
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p rp6502-emu test_xram0_ test_op_exit test_wait_frames -- --nocapture`
Expected: All PASS

**Step 5: Commit**

```bash
git add emu/src/ria_api.rs
git commit -m "feat: add xram0_write, xram0_struct_set, op_exit, wait_frames"
```

---

### Task 4: Add xreg and convenience wrappers

**Files:**
- Modify: `emu/src/ria_api.rs`

**Step 1: Write failing tests**

```rust
#[test]
fn test_xreg_single_value() {
    let mut tb = TraceBuilder::new();
    tb.xreg(1, 0, 0, &[3]); // xreg(1, 0, 0, 3) — set CANVAS to 3
    // xstack pushes: device(1) + channel(1) + addr(1) + 1 value * 2 bytes = 5
    // + trigger write = 6 total
    assert_eq!(tb.trace.len(), 6);
    assert_eq!(tb.trace[0], BusTransaction::write(0, 0xFFEC, 1));    // device
    assert_eq!(tb.trace[1], BusTransaction::write(1, 0xFFEC, 0));    // channel
    assert_eq!(tb.trace[2], BusTransaction::write(2, 0xFFEC, 0));    // start_addr
    assert_eq!(tb.trace[3], BusTransaction::write(3, 0xFFEC, 0));    // hi byte of 3
    assert_eq!(tb.trace[4], BusTransaction::write(4, 0xFFEC, 3));    // lo byte of 3
    assert_eq!(tb.trace[5], BusTransaction::write(5, 0xFFEF, 0x01)); // trigger
}

#[test]
fn test_xreg_multiple_values() {
    let mut tb = TraceBuilder::new();
    tb.xreg(1, 0, 1, &[1, 3, 0xFF00]);
    // 3 header + 3 values * 2 bytes + 1 trigger = 10
    assert_eq!(tb.trace.len(), 10);
    // Check the value bytes: each is hi then lo
    assert_eq!(tb.trace[3], BusTransaction::write(3, 0xFFEC, 0));      // hi of 1
    assert_eq!(tb.trace[4], BusTransaction::write(4, 0xFFEC, 1));      // lo of 1
    assert_eq!(tb.trace[5], BusTransaction::write(5, 0xFFEC, 0));      // hi of 3
    assert_eq!(tb.trace[6], BusTransaction::write(6, 0xFFEC, 3));      // lo of 3
    assert_eq!(tb.trace[7], BusTransaction::write(7, 0xFFEC, 0xFF));   // hi of 0xFF00
    assert_eq!(tb.trace[8], BusTransaction::write(8, 0xFFEC, 0x00));   // lo of 0xFF00
    assert_eq!(tb.trace[9], BusTransaction::write(9, 0xFFEF, 0x01));   // trigger
}

#[test]
fn test_xreg_vga_canvas() {
    let mut tb = TraceBuilder::new();
    tb.xreg_vga_canvas(3);
    assert_eq!(tb.trace.len(), 6);
    assert_eq!(tb.trace[0].data, 1);  // device = VGA
    assert_eq!(tb.trace[1].data, 0);  // channel = 0
    assert_eq!(tb.trace[2].data, 0);  // start_addr = 0 (CANVAS)
}

#[test]
fn test_xreg_vga_mode() {
    let mut tb = TraceBuilder::new();
    tb.xreg_vga_mode(&[3, 0, 0x0000, 0, 0, 0]);
    assert_eq!(tb.trace.len(), 16); // 3 header + 6*2 values + 1 trigger
    assert_eq!(tb.trace[2].data, 1);  // start_addr = 1 (MODE)
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p rp6502-emu test_xreg -- --nocapture`
Expected: FAIL

**Step 3: Implement**

```rust
/// Send xreg operation — mirrors `xreg(device, channel, addr, ...)`.
/// Pushes header and values to xstack, then triggers OP_XREG (0x01).
pub fn xreg(&mut self, device: u8, channel: u8, addr: u8, values: &[u16]) {
    self.write(0xFFEC, device);
    self.write(0xFFEC, channel);
    self.write(0xFFEC, addr);
    for &val in values {
        self.write(0xFFEC, (val >> 8) as u8);  // hi byte first
        self.write(0xFFEC, (val & 0xFF) as u8); // lo byte second
    }
    self.write(0xFFEF, 0x01); // OP_XREG
}

/// Set VGA canvas — mirrors `xreg_vga_canvas(value)`.
pub fn xreg_vga_canvas(&mut self, value: u16) {
    self.xreg(1, 0, 0, &[value]);
}

/// Set VGA mode registers — mirrors `xreg_vga_mode(...)`.
pub fn xreg_vga_mode(&mut self, values: &[u16]) {
    self.xreg(1, 0, 1, values);
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p rp6502-emu test_xreg -- --nocapture`
Expected: All PASS

**Step 5: Commit**

```bash
git add emu/src/ria_api.rs
git commit -m "feat: add xreg, xreg_vga_canvas, xreg_vga_mode helpers"
```

---

### Task 5: Add struct offset constants

**Files:**
- Modify: `emu/src/ria_api.rs`

**Step 1: Add offset constant modules**

No tests needed — these are just constants. Add at top of `ria_api.rs` (after imports, before `TraceBuilder`):

```rust
/// Field offsets for `vga_mode3_config_t` from `cc65/include/rp6502.h`.
/// Mirrors the C struct layout for use with `xram0_struct_set`.
#[allow(non_upper_case_globals)]
pub mod vga_mode3_config_t {
    pub const X_WRAP: u16 = 0;            // bool (u8)
    pub const Y_WRAP: u16 = 1;            // bool (u8)
    pub const X_POS_PX: u16 = 2;          // i16
    pub const Y_POS_PX: u16 = 4;          // i16
    pub const WIDTH_PX: u16 = 6;          // i16
    pub const HEIGHT_PX: u16 = 8;         // i16
    pub const XRAM_DATA_PTR: u16 = 10;    // u16
    pub const XRAM_PALETTE_PTR: u16 = 12; // u16
}

/// Field offsets for `vga_mode1_config_t` from `cc65/include/rp6502.h`.
#[allow(non_upper_case_globals)]
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
}
```

**Step 2: Run full test suite**

Run: `cargo test -p rp6502-emu`
Expected: All tests pass (constants are just definitions, nothing breaks)

**Step 3: Commit**

```bash
git add emu/src/ria_api.rs
git commit -m "feat: add vga_mode3_config_t and vga_mode1_config_t offset constants"
```

---

### Task 6: Refactor Mode 3 test generation to use TraceBuilder

**Files:**
- Modify: `emu/src/test_harness.rs`

This is the key refactor. The existing `generate_test_trace` for Mode 3 modes currently builds bus transactions inline. Rewrite it to use `TraceBuilder` and the offset constants.

**Important:** The refactored code uses `xram0_struct_set` per-field instead of sequential blob writes. This changes the *trace shape* (more `$FFE6`/`$FFE7` writes interspersed) but the XRAM *content* is identical because each field write seeks to the correct address. The existing tests check `$FFE4` write counts and exit opcodes — `$FFE4` count stays the same (14 config bytes + pixel data bytes).

**Step 1: Refactor `generate_test_trace` for Mode 3**

Replace the Mode 3 path in `generate_test_trace` (keep Mode 1 dispatch at top, keep `TestMode` enum/impls, keep `pattern_byte`):

```rust
use crate::ria_api::{self, TraceBuilder};

pub fn generate_test_trace(mode: TestMode) -> Vec<BusTransaction> {
    match mode {
        TestMode::Text1bpp320x240 | TestMode::Text8bpp320x240 => {
            return generate_mode1_test_trace(mode);
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
```

**Step 2: Run existing tests**

Run: `cargo test -p rp6502-emu`
Expected: All tests pass. Key tests to watch:
- `test_mono320x240_pixel_count` — 14 + 9600 RW0 writes (unchanged)
- `test_mono640x480_pixel_count` — 14 + 38400 RW0 writes
- `test_color16bpp_partial_height` — 14 + 102*640 RW0 writes
- `test_trace_ends_with_exit` — last transaction is `$FFEF = 0xFF`

**Note:** The pixel count tests count `$FFE4` writes. With per-field `xram0_struct_set`, each config field still writes its bytes through `$FFE4`, so the total is still 14 bytes (2 one-byte bools + 6 two-byte values). The difference is more `$FFE6`/`$FFE7` writes, which these tests don't count.

**Step 3: Commit**

```bash
git add emu/src/test_harness.rs
git commit -m "refactor: Mode 3 test generation uses TraceBuilder helpers"
```

---

### Task 7: Refactor Mode 1 test generation to use TraceBuilder

**Files:**
- Modify: `emu/src/test_harness.rs`

**Step 1: Refactor `generate_mode1_test_trace`**

```rust
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
```

**Note:** Character data uses raw `tb.write(0xFFE4, ...)` calls after `tb.set_addr0(data_ptr)` instead of `xram0_write` because the data is generated per-cell in a loop, not from a pre-built buffer. This mirrors the C pattern of sequential `RIA.rw0` writes after setting `RIA.addr0`.

**Step 2: Run existing tests**

Run: `cargo test -p rp6502-emu`
Expected: All tests pass.

**Step 3: Commit**

```bash
git add emu/src/test_harness.rs
git commit -m "refactor: Mode 1 test generation uses TraceBuilder helpers"
```

---

### Task 8: Clean up — remove dead cycle tracking from test_harness.rs

**Files:**
- Modify: `emu/src/test_harness.rs`

**Step 1: Verify no manual `BusTransaction::write` calls remain**

Grep `emu/src/test_harness.rs` for `BusTransaction::write`. There should be zero remaining calls (all replaced by `TraceBuilder` methods). If the `use crate::bus::BusTransaction` import is only used by tests, verify that and keep it for test assertions.

**Step 2: Run full test suite**

Run: `cargo test -p rp6502-emu`
Expected: All tests pass, no warnings.

**Step 3: Commit**

```bash
git add emu/src/test_harness.rs
git commit -m "refactor: clean up test_harness after TraceBuilder migration"
```
