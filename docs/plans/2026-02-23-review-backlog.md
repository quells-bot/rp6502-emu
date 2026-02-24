# Emulator Review Backlog

Issues identified during the firmware/docs vs. emulator review on 2026-02-23. These are not addressed by the fixes in `2026-02-23-review-fixes.md` and are tracked here for future work.

---

## 1. Combined Canvas+Mode xreg Special Case

**Severity:** Behavioral — silent failure with real SDK code

**Location:** `emu/src/ria.rs` `handle_xreg()`

**Problem:** The firmware (`firmware/src/ria/sys/pix.c:170-182`) has a special case: when `xreg(1, 0, 0, canvas, mode, attr, ...)` sends both CANVAS (reg 0) and MODE (reg 1) in a single call, it sends CANVAS first (blocking until ACK), then sends the remaining registers in descending order.

The emulator sends all registers in simple reverse order (highest first, CANVAS last). In a combined call, MODE arrives before CANVAS — it programs a plane, then CANVAS arrives and wipes all planes.

The test harness sidesteps this by making two separate xreg calls, but real SDK usage (`xreg(1, 0, 0, canvas, mode, attr, config_ptr, plane, begin, end)`) would silently produce a blank screen.

**Fix approach:** Detect the combined case (device=1, channel=0, start_addr=0, count>1), extract and send CANVAS first, wait for ACK, then adjust start_addr to 1 and send the rest.

---

## 2. xreg Ack/Nak State Machine

**Severity:** Simplification — no impact on happy path

**Location:** `emu/src/ria.rs` `handle_xreg()`, `emu/src/vga/mod.rs` `handle_reg()`

**Problem:** The firmware's `pix_api_xreg()` is an async state machine that:
- Sends one register per iteration (not all at once)
- Waits for VGA ACK/NAK after CANVAS or MODE writes
- Has a 2ms timeout returning EIO on no response
- Returns EINVAL on NAK

The emulator fires all registers synchronously and ignores ACK/NAK (`ria.rs:164`). This means:
- No timeout-based error detection
- No NAK → EINVAL error path
- Programs that poll BUSY or check errno after xreg would see different behavior

**Fix approach:** After sending CANVAS or MODE, block on backchannel_rx with a timeout. Return appropriate errno on NAK or timeout.

---

## 3. palette_ptr=0 Treated as Built-in (Documented Divergence)

**Severity:** Minor — only affects unusual palette configurations

**Location:** `emu/src/vga/mode3.rs` `resolve_palette()` lines 120-121, 144-145

**Problem:** The emulator adds `palette_ptr > 0` to use built-in palettes when the pointer is zero. The firmware has no such check — `ptr=0` passes the alignment test (`!(0 & 1)` is true) and would read XRAM at offset 0 as a custom palette.

In practice XRAM[0] holds the Mode3Config struct, so reading it as a palette would produce garbage. Programs typically use `0xFFFF` (odd, fails alignment check) to explicitly request built-in palettes.

The divergence is documented in comments at `mode3.rs:116-118` and `mode3.rs:140-142`.

**Fix approach (if desired):** Remove the `palette_ptr > 0` check to match firmware. This would only matter if a program intentionally placed a palette at XRAM address 0.

---

## 4. Built-in Palette via 0xFFFF Sentinel Not Explicitly Handled

**Severity:** Minor — works correctly by accident

**Location:** `emu/src/vga/mode3.rs` `resolve_palette()`

**Problem:** The pico-docs say built-in palettes are accessible via special XRAM pointer `$FFFF`. In the firmware, `ptr=0xFFFF` fails the alignment check (`0xFFFF & 1 != 0`), causing fallthrough to the built-in palette. The emulator's alignment check (`palette_ptr & 1 == 0`) also rejects `0xFFFF`, so it works — but for the right reasons by accident rather than by explicit sentinel handling.

**Fix approach:** No code change needed, but a comment noting that `0xFFFF` is the conventional sentinel for built-in palettes (and that it falls through correctly due to odd alignment) would improve clarity.

---

## 5. Firmware `2 ^ bpp` XOR Bug Corrected (Intentional)

**Severity:** None — intentional improvement over firmware

**Location:** `emu/src/vga/mode3.rs` `resolve_palette()` line 136

**Problem:** `firmware/src/vga/modes/mode3.c:54` uses `2 ^ bpp` where `^` is C's bitwise XOR, not exponentiation. This gives wrong palette sizes (e.g., `2 ^ 8 = 10` instead of 256). The emulator correctly uses `1 << bpp`.

This is documented at `mode3.rs:137-139`. No action needed — this is a firmware bug the emulator deliberately does not replicate.

---

## 6. Canvas+Depth Constraint Validation Missing

**Severity:** Low — test harness only generates valid combinations

**Location:** `emu/src/vga/mod.rs` `handle_reg()` CANVAS case

**Problem:** The pico-docs specify maximum color depths per canvas:
- 640x480: 1bpp only
- 640x360: (not explicitly limited for Mode 3)
- 320x240: 4bpp max
- 320x180: 8bpp max

The emulator does not enforce these limits — any bpp can be used with any canvas. The firmware likely enforces this in `vga_xreg_canvas()` or `mode3_prog()` (returning NAK for invalid combinations).

**Fix approach:** Add validation in `program_mode3()` that checks the canvas/bpp combination and returns without programming the plane (sending NAK) for invalid combinations.
