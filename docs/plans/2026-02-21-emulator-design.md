# RP6502 Emulator Design

## Overview

A Rust-based "digital twin" emulator for the RP6502 Picocomputer, starting with the RIA and VGA components. Uses egui to display the framebuffer. Designed for both development (running real 6502 binaries) and education (observing data flow through the system).

A 6502 CPU emulator will be added later. For the MVP, bus traces drive the system.

## Architecture

Message-passing digital twin: RIA and VGA run as independent threads communicating via typed PIX messages over crossbeam channels, mirroring the real hardware topology.

```
[Bus Trace] -> [RIA Thread] -> PIX Channel -> [VGA Thread] -> [Framebuffer]
                                                                    |
                                                              [egui Renderer]
                    Backchannel Channel <---------------------------|
```

This naturally leads to event-sourcing later -- the channel messages are already the event stream.

## Core Data Types

### BusTransaction

The input format. What a 6502 (or test harness) produces:

```rust
struct BusTransaction {
    cycle: u64,       // clock cycle number
    addr: u16,        // full 16-bit address (only $FFE0-$FFFF hits RIA)
    data: u8,         // data byte
    rw: bool,         // true = read, false = write
}
```

Bus trace file format: binary with a header (magic bytes + version), then packed BusTransaction records. Also support a human-readable text format for hand-crafted test scenarios.

### PIX Messages

Two message types matching the real hardware's two VGA PIO state machines:

```rust
/// XRAM broadcast path (high bandwidth, device 0 channel 0)
struct XramWrite {
    addr: u16,
    data: u8,
}

/// Register/control path (device 1+, any channel)
struct PixRegWrite {
    channel: u8,
    register: u8,
    value: u16,
}

/// Combined PIX event sent over the channel
enum PixEvent {
    Xram(XramWrite),
    Reg(PixRegWrite),
    FrameSync,  // RIA signals a frame boundary
}
```

All messages can be packed/unpacked to the real 32-bit hardware format:
`0x10000000 | (dev << 29) | (ch << 24) | (reg << 16) | value`

### Backchannel (VGA -> RIA)

```rust
enum Backchannel {
    Vsync(u8),  // 0x80 | (frame_no & 0xF)
    Ack,        // 0x90
    Nak,        // 0xA0
}
```

## RIA Module

### State

```rust
struct Ria {
    regs: [u8; 32],          // $FFE0-$FFFF register file
    xram: [u8; 65536],       // 64KB extended RAM
    xstack: [u8; 513],       // 512 bytes + 1 zero byte for cstring safety
    xstack_ptr: usize,       // starts at 0x200 (empty), decrements on push
    irq_enabled: u8,         // bit 0 enables VSYNC interrupts
    irq_pin: bool,           // true = high (inactive), false = low (asserted)
    api_active_op: u8,       // latched OS operation in progress
    api_errno_opt: u8,       // 0=null, 1=cc65, 2=llvm-mos
    cycle_count: u64,        // current PHI2 cycle
    pix_tx: Sender<PixEvent>,
    backchannel_rx: Receiver<Backchannel>,
}
```

### Register Behavior

Derived directly from `ria.c` act_loop (lines 245-407).

**Continuous behavior:** Every iteration, RW0 and RW1 are refreshed from XRAM:
- `regs[0x04] = xram[addr0]`
- `regs[0x08] = xram[addr1]`

**On bus write:**

| Address | Register | Behavior |
|---------|----------|----------|
| $FFE1 | TX | Write byte to UART TX (if writable). Update $FFE0 bit 7. |
| $FFE4 | RW0 | `xram[addr0] = data`, emit `XramWrite{addr0, data}`, queue if same page. Fallthrough to read case. |
| $FFE5 | STEP0 | Set step0 as signed i8. |
| $FFE6-7 | ADDR0 | Set low/high byte of addr0. |
| $FFE8 | RW1 | `xram[addr1] = data`, emit `XramWrite{addr1, data}`, queue if same page. Fallthrough to read case. |
| $FFE9 | STEP1 | Set step1 as signed i8. |
| $FFEA-B | ADDR1 | Set low/high byte of addr1. |
| $FFEC | XSTACK | `if xstack_ptr > 0 { xstack_ptr -= 1; xstack[xstack_ptr] = data; }` then `regs[0x0C] = xstack[xstack_ptr]`. |
| $FFEF | OP | Trigger OS operation. 0x00 = zxstack, 0xFF = exit, others dispatched to api_task. |
| $FFF0 | IRQ | Store data to irq_enabled. Fallthrough: set IRQB pin HIGH (clear interrupt). |
| $FFF4 | A | Set OS call register A. |
| $FFF6 | X | Set OS call register X. |
| $FFF8-9 | SREG | Set 32-bit extension register low/high. |

**On bus read:**

| Address | Register | Behavior |
|---------|----------|----------|
| $FFE0 | READY | Update: check RX queue, set bit 6 if data ready. Check TX, set bit 7 if writable. Return value. |
| $FFE2 | RX | If com_rx_char >= 0: return it, set $FFE0 bit 6, clear queue. Else: clear bit 6, return 0. |
| $FFE4 | RW0 | `addr0 = addr0.wrapping_add(step0 as u16)` (auto-increment after read). |
| $FFE8 | RW1 | `addr1 = addr1.wrapping_add(step1 as u16)` (auto-increment after read). |
| $FFEC | XSTACK | `if xstack_ptr < 0x200 { xstack_ptr += 1; }` then `regs[0x0C] = xstack[xstack_ptr]`. |
| $FFF0 | IRQ | Set IRQB pin HIGH (acknowledge/clear interrupt). Return current value. |
| $FFF2 | BUSY | Return value (bit 7 = busy flag). |
| $FFF0-7 | Code area | Return self-modifying code bytes (set by api return mechanism). |
| $FFFA-FF | Vectors | Return 6502 vectors from register file. |

Note: RW0/RW1 writes also trigger auto-increment (the write case falls through to the read case in the firmware).

### Reset Defaults (from `api.c` api_run, lines 99-111)

- Registers $FFE0-$FFEE zeroed, **except $FFE3 (VSYNC) which is preserved**
- STEP0 = STEP1 = 1 (signed +1)
- RW0 = RW1 = xram[0]
- xstack_ptr = 0x200 (empty)
- api_errno_opt = 0 (null/unset)

### OS Operations (MVP)

- 0x00: zxstack -- zero the stack, reset pointer to 0x200
- 0x01: xreg -- send extended register to PIX device (forwards accumulated xregs as PixRegWrite)
- 0xFF: exit -- stop CPU
- All others: return ENOSYS

### API Return Mechanism (from `api.h` lines 138-202)

The return mechanism manipulates registers $FFF0-$FFF9 to form executable 6502 code:
- Blocked state: `$FFF0: NOP, $FFF1-2: BRA -2 (spin), $FFF3-4: LDA #$FF, $FFF5-6: LDX #$FF, $FFF7: RTS`
- Released state: BRA offset changed to 0x00 (fall through to LDA/LDX/RTS)
- Return value set via A ($FFF4) and X ($FFF6) registers
- 32-bit returns also set SREG ($FFF8-9)

## VGA Module

### State

```rust
struct Vga {
    xram: [u8; 65536],                // local XRAM replica
    planes: [Option<PlaneConfig>; 3], // up to 3 fill planes
    canvas_width: u16,                // 640 for MVP
    canvas_height: u16,               // 480 for MVP
    xregs: [u16; 8],                  // accumulated xreg parameters
    framebuffer: Vec<u32>,            // RGBA output, shared with egui
    pix_rx: Receiver<PixEvent>,
    backchannel_tx: Sender<Backchannel>,
}
```

### PIX Message Handling (from `vga/sys/pix.c` lines 177-192)

Two paths matching real hardware:

1. **XRAM path**: `XramWrite { addr, data }` -> `xram[addr] = data`
2. **Register path**: `PixRegWrite { channel, register, value }`
   - Channel 0, register 0 (CANVAS): configure canvas size, reset all planes, send Ack/Nak
   - Channel 0, register 1 (MODE): program a graphics mode into a plane using accumulated xregs, send Ack/Nak
   - Channel 0, registers 2-7: accumulate into xregs buffer
   - Channel 15: display config, code page, backchannel control
3. **FrameSync**: render current state to framebuffer, send Vsync backchannel

After CANVAS or MODE commands, xregs buffer is cleared to zero.

### Mode 3 Bitmap Rendering (from `vga/modes/mode3.c`)

Configuration via `mode3_config_t` stored in XRAM:

```rust
struct Mode3Config {     // read from xram at config_ptr
    x_wrap: bool,        // horizontal wrapping
    y_wrap: bool,        // vertical wrapping
    x_pos_px: i16,       // horizontal scroll offset
    y_pos_px: i16,       // vertical scroll offset
    width_px: i16,       // bitmap width in pixels
    height_px: i16,      // bitmap height in pixels
    xram_data_ptr: u16,  // XRAM address of pixel data
    xram_palette_ptr: u16, // XRAM address of palette
}
```

Programming sequence (via xregs accumulated before MODE command):
- xregs[2]: attributes (color depth + bit order)
- xregs[3]: config_ptr (XRAM address of mode3_config_t)
- xregs[4]: plane index (0-2)
- xregs[5]: scanline_begin
- xregs[6]: scanline_end (0 = canvas height)

Color formats (attributes field):
- 0: 1bpp MSB-first, 8: 1bpp LSB-first
- 1: 2bpp MSB-first, 9: 2bpp LSB-first
- 2: 4bpp MSB-first, 10: 4bpp LSB-first
- 3: 8bpp (256-color indexed)
- 4: 16bpp (direct RGB565)

Per-scanline rendering:
1. Map scanline to bitmap row (y_pos_px offset, optional y-wrap)
2. For each column range: extract pixel bits, palette lookup, write RGBA to framebuffer
3. Out-of-bounds pixels render as black/transparent

### Palettes

Built-in palettes matching firmware (`color.c`):
- **2-color** (1bpp default): index 0 = black/transparent, index 1 = light grey (192,192,192)
- **256-color** (ANSI): indices 0-15 standard + bright ANSI, 16-231 6x6x6 RGB cube, 232-255 24-step greyscale

Custom palettes: array of uint16 (RGB565) values stored in XRAM at xram_palette_ptr.

## Threading & Communication

```
Main Thread (egui)
  |
  | reads framebuffer via Arc<Mutex<Vec<u32>>>
  |
  +-- RIA Thread
  |     - Replays bus trace by cycle count
  |     - Processes register reads/writes
  |     - Emits PixEvent over crossbeam channel
  |     - Checks frame boundaries (every phi2_freq/60 cycles), sends FrameSync
  |     - Receives Backchannel messages, updates VSYNC counter
  |
  +-- VGA Thread
        - Receives PixEvent from crossbeam channel
        - Updates local XRAM replica
        - On FrameSync: renders framebuffer, sends Vsync backchannel
        - On register writes: configures canvas/modes, sends Ack/Nak
```

Framebuffer sharing: `Arc<Mutex<Vec<u32>>>`. VGA locks to write, egui locks to upload as texture.

### Timing

RIA thread is the master clock. It paces itself by cycle count from the bus trace. Every `phi2_freq / 60` cycles it sends a `FrameSync` to VGA. This is a simplification -- on real hardware VGA has its own independent video timing -- but preserves correct unidirectional data flow and is sufficient for the MVP.

## Project Structure

```
rp6502-emu/
+-- Cargo.toml
+-- src/
    +-- main.rs           # egui app, framebuffer texture, GUI loop
    +-- bus.rs             # BusTransaction, trace file parsing/generation
    +-- pix.rs             # PixMessage, PixEvent, Backchannel types
    +-- ria.rs             # RIA state machine
    +-- vga/
    |   +-- mod.rs         # VGA thread: PIX receiver, XRAM replica, dispatch
    |   +-- mode3.rs       # Mode 3 bitmap renderer
    |   +-- palette.rs     # Built-in palettes
    +-- test_harness.rs    # Programmatic bus trace generation
```

### Dependencies

- `eframe` / `egui` -- windowed app with texture rendering
- `crossbeam-channel` -- mpsc channels for PIX bus and backchannel
- `bytemuck` -- zero-copy casting for framebuffer data

### MVP End-to-End Flow

1. Test harness generates a bus trace: writes pixel data to XRAM via RW0, sends xreg commands to configure Mode 3 bitmap at 640x480
2. RIA thread replays the trace, updating registers and XRAM, emitting PIX messages
3. VGA thread receives PIX messages, updates XRAM replica, renders Mode 3 into RGBA framebuffer
4. egui displays the framebuffer as a texture at 60fps
