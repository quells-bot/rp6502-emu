use crate::bus::BusTransaction;

/// Generate a bus trace that sets up a 320x200 8bpp gradient bitmap in Mode 3.
///
/// The trace:
/// 1. Writes a Mode3Config struct to XRAM at address 0x0000
/// 2. Writes 320x200 gradient pixels to XRAM starting at address 0x0100
/// 3. Configures VGA via xreg: CANVAS=640x480, MODE=3, attr=8bpp, config_ptr=0
/// 4. Exits after 1 frame worth of cycles
///
/// The canvas stays 640x480 (set by CANVAS register = 3); the bitmap covers only
/// the top-left 320x200 area. 320*200 = 64,000 bytes which fits in XRAM (64KB).
pub fn generate_gradient_trace() -> Vec<BusTransaction> {
    let mut trace = Vec::new();
    let mut cycle: u64 = 0;

    let config_ptr: u16 = 0x0000;
    let data_ptr: u16 = 0x0100;
    let bitmap_width: i16 = 320;
    let bitmap_height: i16 = 200;

    // --- Step 1: Write Mode3Config to XRAM at config_ptr via ADDR0/RW0 ---
    // Set ADDR0 to config_ptr (low byte $FFE6, high byte $FFE7)
    trace.push(BusTransaction::write(cycle, 0xFFE6, (config_ptr & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFE7, (config_ptr >> 8) as u8));
    cycle += 1;

    // Write 14 bytes of Mode3Config via RW0 ($FFE4), auto-increment handles address
    let config_bytes: Vec<u8> = vec![
        0, 0,                                                          // x_wrap=false, y_wrap=false
        0, 0,                                                          // x_pos_px = 0
        0, 0,                                                          // y_pos_px = 0
        (bitmap_width & 0xFF) as u8, (bitmap_width >> 8) as u8,       // width_px
        (bitmap_height & 0xFF) as u8, (bitmap_height >> 8) as u8,     // height_px
        (data_ptr & 0xFF) as u8, (data_ptr >> 8) as u8,               // xram_data_ptr
        0, 0,                                                          // xram_palette_ptr = 0 (default)
    ];
    for &b in &config_bytes {
        trace.push(BusTransaction::write(cycle, 0xFFE4, b));
        cycle += 1;
    }

    // --- Step 2: Write gradient pixel data to XRAM at data_ptr ---
    // Set ADDR0 to data_ptr
    trace.push(BusTransaction::write(cycle, 0xFFE6, (data_ptr & 0xFF) as u8));
    cycle += 1;
    trace.push(BusTransaction::write(cycle, 0xFFE7, (data_ptr >> 8) as u8));
    cycle += 1;

    // 320x200 8bpp gradient: pixel at (x,y) = (x + y) % 256
    for y in 0..bitmap_height as u16 {
        for x in 0..bitmap_width as u16 {
            let color = ((x + y) % 256) as u8;
            trace.push(BusTransaction::write(cycle, 0xFFE4, color));
            cycle += 1;
        }
    }

    // --- Step 3: Configure VGA via xreg ---
    // Push to xstack ($FFEC): device, channel, start_addr, then uint16 values in register order.
    // Stack grows downward: first push -> XSTACK_SIZE-1 (highest addr).
    // handle_xreg maps first-pushed data -> lowest register (start_addr+0).

    // Push device = 1 (VGA)
    trace.push(BusTransaction::write(cycle, 0xFFEC, 1));
    cycle += 1;
    // Push channel = 0
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0));
    cycle += 1;
    // Push start_addr = 0 (start at register 0)
    trace.push(BusTransaction::write(cycle, 0xFFEC, 0));
    cycle += 1;

    // Push uint16 register values in order (reg 0 first = canvas, reg 6 last = scanline_end).
    // The xstack grows downward: each push decrements the ptr and stores at the new ptr.
    // handle_xreg reads u16 values as from_le_bytes([xstack[lower_addr], xstack[lower_addr+1]]).
    // Pushing hi byte first puts hi at the lower address; pushing lo byte second puts lo at
    // lower_addr+1. That gives [lower, lower+1] = [hi, lo] which is still wrong — we need lo
    // at the lower address. So we must push hi first so it lands at the higher address, then lo
    // so it lands at the lower address: [lower, lower+1] = [lo, hi] = correct little-endian.
    let reg_values: &[u16] = &[
        3,           // reg 0: CANVAS = 640x480
        3,           // reg 1: MODE = Mode 3
        3,           // reg 2: attributes = 8bpp
        config_ptr,  // reg 3: config_ptr
        0,           // reg 4: plane = 0
        0,           // reg 5: scanline_begin = 0
        0,           // reg 6: scanline_end = 0 (= canvas height)
    ];
    for &val in reg_values {
        trace.push(BusTransaction::write(cycle, 0xFFEC, (val >> 8) as u8));    // hi first
        cycle += 1;
        trace.push(BusTransaction::write(cycle, 0xFFEC, (val & 0xFF) as u8));  // lo second
        cycle += 1;
    }

    // Trigger xreg operation: OP = 0x01
    trace.push(BusTransaction::write(cycle, 0xFFEF, 0x01));
    cycle += 1;

    // Wait long enough for at least 1 frame (phi2_freq=8MHz, 60fps -> ~133333 cycles/frame)
    // Then exit.
    trace.push(BusTransaction::write(cycle + 200_000, 0xFFEF, 0xFF));

    trace
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gradient_trace_not_empty() {
        let trace = generate_gradient_trace();
        assert!(trace.len() > 100);
        // First transaction writes to ADDR0 low byte
        assert_eq!(trace[0].addr, 0xFFE6);
    }

    #[test]
    fn test_gradient_trace_has_exit() {
        let trace = generate_gradient_trace();
        let last = trace.last().unwrap();
        assert_eq!(last.addr, 0xFFEF);
        assert_eq!(last.data, 0xFF); // exit opcode
    }

    #[test]
    fn test_gradient_trace_pixel_count() {
        // Should have 320*200 pixel writes plus config overhead
        let trace = generate_gradient_trace();
        assert!(trace.len() >= 320 * 200);
    }
}
