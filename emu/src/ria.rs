use crossbeam_channel::{Receiver, Sender, TryRecvError};
use crate::bus::BusTransaction;
use crate::pix::{Backchannel, PixEvent, PixRegWrite, XramWrite};

const XSTACK_SIZE: usize = 0x200;

pub struct Ria {
    /// Register file: $FFE0-$FFFF mapped to indices 0-31.
    pub regs: [u8; 32],
    /// 64KB extended RAM.
    pub xram: Box<[u8; 65536]>,
    /// 512-byte stack + 1 zero byte for cstring safety.
    pub xstack: [u8; XSTACK_SIZE + 1],
    /// Stack pointer. Starts at XSTACK_SIZE (empty), decrements on push.
    pub xstack_ptr: usize,
    /// IRQ enable register (bit 0 enables VSYNC IRQ).
    pub irq_enabled: u8,
    /// IRQ pin state. true = high (inactive), false = low (asserted).
    pub irq_pin: bool,
    /// Current PHI2 cycle count.
    pub cycle_count: u64,
    /// PHI2 frequency in Hz (default 8 MHz).
    #[allow(dead_code)]
    pub phi2_freq: u64,
    /// Cycles per frame (phi2_freq / 60).
    cycles_per_frame: u64,
    /// Cycle count of next frame boundary.
    next_frame_cycle: u64,
    /// PIX transmit channel (RIA -> VGA).
    pix_tx: Sender<PixEvent>,
    /// Backchannel receive (VGA -> RIA).
    backchannel_rx: Receiver<Backchannel>,
    /// Whether the emulator is running.
    pub running: bool,
}

impl Ria {
    pub fn new(
        pix_tx: Sender<PixEvent>,
        backchannel_rx: Receiver<Backchannel>,
    ) -> Self {
        let phi2_freq = 8_000_000;
        let cycles_per_frame = phi2_freq / 60;
        let mut ria = Self {
            regs: [0; 32],
            xram: Box::new([0; 65536]),
            xstack: [0; XSTACK_SIZE + 1],
            xstack_ptr: XSTACK_SIZE,
            irq_enabled: 0,
            irq_pin: true,
            cycle_count: 0,
            phi2_freq,
            cycles_per_frame,
            next_frame_cycle: cycles_per_frame,
            pix_tx,
            backchannel_rx,
            running: true,
        };
        ria.reset();
        ria
    }

    /// Reset registers to power-on defaults.
    /// Matches api_run() in firmware/src/ria/api/api.c lines 99-111.
    pub fn reset(&mut self) {
        // Zero registers 0-15 ($FFE0-$FFEF), skip register 3 ($FFE3 VSYNC)
        for i in 0..16 {
            if i != 3 {
                self.regs[i] = 0;
            }
        }
        // STEP0 = 1 (signed +1)
        self.regs[0x05] = 1;
        // RW0 = xram[0]
        self.regs[0x04] = self.xram[0];
        // STEP1 = 1 (signed +1)
        self.regs[0x09] = 1;
        // RW1 = xram[0]
        self.regs[0x08] = self.xram[0];
        // Reset xstack
        self.xstack_ptr = XSTACK_SIZE;
        self.irq_enabled = 0;
        self.irq_pin = true;
        self.running = true;
    }

    // --- Register accessors matching firmware macros ---

    fn addr0(&self) -> u16 {
        u16::from_le_bytes([self.regs[0x06], self.regs[0x07]])
    }

    fn set_addr0(&mut self, val: u16) {
        let bytes = val.to_le_bytes();
        self.regs[0x06] = bytes[0];
        self.regs[0x07] = bytes[1];
    }

    fn step0(&self) -> i8 {
        self.regs[0x05] as i8
    }

    fn addr1(&self) -> u16 {
        u16::from_le_bytes([self.regs[0x0A], self.regs[0x0B]])
    }

    fn set_addr1(&mut self, val: u16) {
        let bytes = val.to_le_bytes();
        self.regs[0x0A] = bytes[0];
        self.regs[0x0B] = bytes[1];
    }

    fn step1(&self) -> i8 {
        self.regs[0x09] as i8
    }

    /// Refresh RW0 and RW1 from XRAM.
    /// Matches act_loop lines 249-250: RIA_RW0 = xram[RIA_ADDR0]; RIA_RW1 = xram[RIA_ADDR1];
    fn refresh_rw(&mut self) {
        let addr0 = self.addr0();
        self.regs[0x04] = self.xram[addr0 as usize];
        let addr1 = self.addr1();
        self.regs[0x08] = self.xram[addr1 as usize];
    }

    /// Process a single bus transaction.
    /// Returns the data byte for reads (value placed on data bus).
    pub fn process(&mut self, txn: &BusTransaction) -> u8 {
        self.cycle_count = txn.cycle;

        // Check for frame boundary
        if self.cycle_count >= self.next_frame_cycle {
            self.next_frame_cycle += self.cycles_per_frame;
            let _ = self.pix_tx.send(PixEvent::FrameSync);
            // Process backchannel
            self.poll_backchannel();
        }

        // Refresh RW0/RW1 before processing (matches act_loop continuous refresh)
        self.refresh_rw();

        if !txn.hits_ria() {
            return txn.data;
        }

        if txn.rw {
            self.handle_read(txn)
        } else {
            self.handle_write(txn);
            txn.data
        }
    }

    /// Poll backchannel for VGA responses.
    pub fn poll_backchannel(&mut self) {
        loop {
            match self.backchannel_rx.try_recv() {
                Ok(Backchannel::Vsync(frame)) => {
                    self.regs[0x03] = frame;
                    if self.irq_enabled & 0x01 != 0 {
                        self.irq_pin = false;
                    }
                }
                Ok(Backchannel::Ack) | Ok(Backchannel::Nak) => {
                    // For MVP, we don't track ack/nak state
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.running = false;
                    break;
                }
            }
        }
    }

    /// Handle a 6502 write to RIA register space.
    /// Matches act_loop switch cases for CASE_WRITE.
    fn handle_write(&mut self, txn: &BusTransaction) {
        let data = txn.data;
        let reg = txn.ria_reg();

        match reg {
            // $FFE1: UART TX
            0x01 => {
                // MVP: just store the byte, no real UART
                self.regs[0x00] |= 0b1000_0000; // TX always ready
            }

            // $FFE4: Write XRAM via portal 0
            0x04 => {
                let addr = self.addr0();
                self.xram[addr as usize] = data;
                let _ = self.pix_tx.send(PixEvent::Xram(XramWrite { addr, data }));
                // Fallthrough: auto-increment addr0
                let new_addr = addr.wrapping_add(self.step0() as i16 as u16);
                self.set_addr0(new_addr);
            }

            // $FFE5: STEP0
            0x05 => {
                self.regs[0x05] = data;
            }

            // $FFE6: ADDR0 low
            0x06 => {
                self.regs[0x06] = data;
            }

            // $FFE7: ADDR0 high
            0x07 => {
                self.regs[0x07] = data;
            }

            // $FFE8: Write XRAM via portal 1
            0x08 => {
                let addr = self.addr1();
                self.xram[addr as usize] = data;
                let _ = self.pix_tx.send(PixEvent::Xram(XramWrite { addr, data }));
                // Fallthrough: auto-increment addr1
                let new_addr = addr.wrapping_add(self.step1() as i16 as u16);
                self.set_addr1(new_addr);
            }

            // $FFE9: STEP1
            0x09 => {
                self.regs[0x09] = data;
            }

            // $FFEA: ADDR1 low
            0x0A => {
                self.regs[0x0A] = data;
            }

            // $FFEB: ADDR1 high
            0x0B => {
                self.regs[0x0B] = data;
            }

            // $FFEC: XSTACK push
            0x0C => {
                if self.xstack_ptr > 0 {
                    self.xstack_ptr -= 1;
                    self.xstack[self.xstack_ptr] = data;
                }
                self.regs[0x0C] = self.xstack[self.xstack_ptr];
            }

            // $FFED: ERRNO low
            0x0D => {
                self.regs[0x0D] = data;
            }

            // $FFEE: ERRNO high
            0x0E => {
                self.regs[0x0E] = data;
            }

            // $FFEF: OP - trigger OS operation
            0x0F => {
                self.regs[0x0F] = data;
                self.handle_op(data);
            }

            // $FFF0: IRQ enable/disable + clear
            0x10 => {
                self.irq_enabled = data;
                self.irq_pin = true; // clear interrupt
            }

            // $FFF4: A register
            0x14 => { self.regs[0x14] = data; }

            // $FFF6: X register
            0x16 => { self.regs[0x16] = data; }

            // $FFF8: SREG low
            0x18 => { self.regs[0x18] = data; }

            // $FFF9: SREG high
            0x19 => { self.regs[0x19] = data; }

            // All other writes: store in register file
            _ => {
                self.regs[reg as usize] = data;
            }
        }
    }

    /// Handle a 6502 read from RIA register space.
    /// Matches act_loop switch cases for CASE_READ.
    fn handle_read(&mut self, txn: &BusTransaction) -> u8 {
        let reg = txn.ria_reg();

        match reg {
            // $FFE0: UART flow control
            0x00 => {
                // MVP: TX always ready, no RX
                self.regs[0x00] |= 0b1000_0000; // TX ready
                self.regs[0x00] &= !0b0100_0000; // no RX data
                self.regs[0x00]
            }

            // $FFE2: UART RX
            0x02 => {
                // MVP: no UART data
                self.regs[0x00] &= !0b0100_0000;
                self.regs[0x02] = 0;
                0
            }

            // $FFE4: Read XRAM via portal 0 (auto-increment after)
            0x04 => {
                let val = self.regs[0x04]; // already refreshed
                let addr = self.addr0();
                let new_addr = addr.wrapping_add(self.step0() as i16 as u16);
                self.set_addr0(new_addr);
                val
            }

            // $FFE8: Read XRAM via portal 1 (auto-increment after)
            0x08 => {
                let val = self.regs[0x08]; // already refreshed
                let addr = self.addr1();
                let new_addr = addr.wrapping_add(self.step1() as i16 as u16);
                self.set_addr1(new_addr);
                val
            }

            // $FFEC: XSTACK pop
            // The 6502 sees the current register value (set by previous push/pop).
            // We return that, then update pointer and register for next access.
            0x0C => {
                let val = self.regs[0x0C];
                if self.xstack_ptr < XSTACK_SIZE {
                    self.xstack_ptr += 1;
                }
                self.regs[0x0C] = self.xstack[self.xstack_ptr];
                val
            }

            // $FFF0: IRQ acknowledge
            0x10 => {
                self.irq_pin = true; // clear interrupt
                self.regs[0x10]
            }

            // All other reads: return current register value
            _ => self.regs[reg as usize],
        }
    }

    /// Handle OS operation triggered by writing to $FFEF.
    fn handle_op(&mut self, op: u8) {
        match op {
            // 0x00: zxstack - zero stack and reset pointer
            0x00 => {
                self.regs[0x0C] = 0; // API_STACK = 0
                self.xstack_ptr = XSTACK_SIZE;
                self.api_return_ax(0);
            }

            // 0x01: xreg - send extended register to PIX device
            0x01 => {
                self.handle_xreg();
            }

            // 0xFF: exit - stop CPU
            0xFF => {
                self.running = false;
            }

            // All others: return ENOSYS (not implemented)
            _ => {
                self.api_return_ax(0xFFFF); // -1
            }
        }
    }

    /// Handle xreg OS operation.
    /// Sends accumulated xstack data as PIX register writes.
    /// Reference: firmware/src/ria/sys/pix.c pix_api_xreg() lines 76-184.
    ///
    /// Xstack layout (pushed by 6502, top-down):
    ///   [XSTACK_SIZE-1] = device
    ///   [XSTACK_SIZE-2] = channel
    ///   [XSTACK_SIZE-3] = start_addr
    ///   [XSTACK_SIZE-4..xstack_ptr] = uint16 data values (pairs of bytes)
    ///
    /// Data mapping: first-pushed (highest offset) -> lowest register, last-pushed (lowest offset) -> highest register.
    fn handle_xreg(&mut self) {
        if self.xstack_ptr >= XSTACK_SIZE - 3 {
            self.api_return_ax(0xFFFF);
            return;
        }

        let device = self.xstack[XSTACK_SIZE - 1];
        let channel = self.xstack[XSTACK_SIZE - 2];
        let start_addr = self.xstack[XSTACK_SIZE - 3];
        let data_bytes = XSTACK_SIZE - self.xstack_ptr - 3;

        if data_bytes < 2 || !data_bytes.is_multiple_of(2) || device > 7 || channel > 15 {
            self.api_return_ax(0xFFFF);
            return;
        }

        let count = data_bytes / 2;

        // Send in order: first-pushed data (at highest offset) -> lowest register (start_addr+0),
        // last-pushed data (at xstack_ptr) -> highest register (start_addr + count - 1).
        // This matches firmware: api_pop_uint16 pops from xstack_ptr upward (last-pushed first),
        // and pix_send uses addr + --pix_send_count (counting from highest down to 0).
        // Send in descending register order (highest first), matching firmware pix_send():
        // pix_send_addr starts at start_addr+count and decrements each cycle.
        // This ensures CANVAS (reg 0) arrives LAST (resetting xregs after MODE reads them),
        // and MODE (reg 1) arrives after regs 2-6 have been accumulated.
        for i in (0..count).rev() {
            let offset = self.xstack_ptr + (count - 1 - i) * 2;
            let value = u16::from_le_bytes([
                self.xstack[offset],
                self.xstack[offset + 1],
            ]);
            let register = start_addr + i as u8;
            let _ = self.pix_tx.send(PixEvent::Reg(PixRegWrite {
                channel,
                register,
                value,
            }));
        }

        self.xstack_ptr = XSTACK_SIZE;
        self.api_return_ax(0);
    }

    /// Set return registers to unblocked state with AX return value.
    /// Matches api_return_ax() in firmware/src/ria/api/api.h.
    fn api_return_ax(&mut self, val: u16) {
        // Released state: NOP, BRA +0, LDA #lo, LDX #hi, RTS
        self.regs[0x10] = 0xEA; // NOP
        self.regs[0x11] = 0x80; // BRA
        self.regs[0x12] = 0x00; // offset 0 (fall through)
        self.regs[0x13] = 0xA9; // LDA #imm
        self.regs[0x14] = (val & 0xFF) as u8; // A
        self.regs[0x15] = 0xA2; // LDX #imm
        self.regs[0x16] = (val >> 8) as u8; // X
        self.regs[0x17] = 0x60; // RTS
        // Update XSTACK register
        self.regs[0x0C] = self.xstack[self.xstack_ptr];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use crate::bus::BusTransaction;

    fn make_ria() -> (Ria, crossbeam_channel::Receiver<PixEvent>, crossbeam_channel::Sender<Backchannel>) {
        let (pix_tx, pix_rx) = unbounded();
        let (back_tx, back_rx) = unbounded();
        let ria = Ria::new(pix_tx, back_rx);
        (ria, pix_rx, back_tx)
    }

    #[test]
    fn test_reset_defaults() {
        let (ria, _, _) = make_ria();
        assert_eq!(ria.regs[0x05], 1); // STEP0 = 1
        assert_eq!(ria.regs[0x09], 1); // STEP1 = 1
        assert_eq!(ria.xstack_ptr, XSTACK_SIZE);
    }

    #[test]
    fn test_xram_write_portal0() {
        let (mut ria, pix_rx, _) = make_ria();
        // Set ADDR0 to 0x0100
        ria.process(&BusTransaction::write(1, 0xFFE6, 0x00)); // low byte
        ria.process(&BusTransaction::write(2, 0xFFE7, 0x01)); // high byte
        // Write 0x42 to XRAM via RW0
        ria.process(&BusTransaction::write(3, 0xFFE4, 0x42));

        assert_eq!(ria.xram[0x0100], 0x42);
        // ADDR0 should have auto-incremented to 0x0101
        assert_eq!(ria.addr0(), 0x0101);

        // Check PIX event was emitted
        let evt = pix_rx.try_recv().unwrap();
        assert_eq!(evt, PixEvent::Xram(XramWrite { addr: 0x0100, data: 0x42 }));
    }

    #[test]
    fn test_xram_read_portal0_auto_increment() {
        let (mut ria, _, _) = make_ria();
        ria.xram[0x0050] = 0xAB;
        // Set ADDR0 to 0x0050
        ria.process(&BusTransaction::write(1, 0xFFE6, 0x50));
        ria.process(&BusTransaction::write(2, 0xFFE7, 0x00));
        // Read from RW0
        let val = ria.process(&BusTransaction::read(3, 0xFFE4, 0));
        assert_eq!(val, 0xAB);
        // ADDR0 auto-incremented to 0x0051
        assert_eq!(ria.addr0(), 0x0051);
    }

    #[test]
    fn test_xram_step_negative() {
        let (mut ria, _, _) = make_ria();
        // Set STEP0 to -1 (0xFF as i8)
        ria.process(&BusTransaction::write(1, 0xFFE5, 0xFF));
        // Set ADDR0 to 0x0010
        ria.process(&BusTransaction::write(2, 0xFFE6, 0x10));
        ria.process(&BusTransaction::write(3, 0xFFE7, 0x00));
        // Write then check auto-decrement
        ria.process(&BusTransaction::write(4, 0xFFE4, 0x01));
        assert_eq!(ria.addr0(), 0x000F);
    }

    #[test]
    fn test_xstack_push_pop() {
        let (mut ria, _, _) = make_ria();
        // Push 0x42
        ria.process(&BusTransaction::write(1, 0xFFEC, 0x42));
        assert_eq!(ria.xstack_ptr, XSTACK_SIZE - 1);
        assert_eq!(ria.regs[0x0C], 0x42);

        // Push 0x43
        ria.process(&BusTransaction::write(2, 0xFFEC, 0x43));
        assert_eq!(ria.xstack_ptr, XSTACK_SIZE - 2);
        assert_eq!(ria.regs[0x0C], 0x43);

        // Pop (read) — 6502 sees current register value (0x43, set by last push)
        let val = ria.process(&BusTransaction::read(3, 0xFFEC, 0));
        assert_eq!(val, 0x43); // TOS was 0x43
        assert_eq!(ria.xstack_ptr, XSTACK_SIZE - 1);
        // After pop, register now reflects new TOS
        assert_eq!(ria.regs[0x0C], 0x42);

        // Pop again — 6502 sees 0x42 (set by previous pop's update)
        let val2 = ria.process(&BusTransaction::read(4, 0xFFEC, 0));
        assert_eq!(val2, 0x42);
        assert_eq!(ria.xstack_ptr, XSTACK_SIZE);
        // Stack empty — reads zero sentinel
        assert_eq!(ria.regs[0x0C], 0);
    }

    #[test]
    fn test_op_zxstack() {
        let (mut ria, _, _) = make_ria();
        // Push some data
        ria.process(&BusTransaction::write(1, 0xFFEC, 0x42));
        ria.process(&BusTransaction::write(2, 0xFFEC, 0x43));
        // Trigger zxstack (OP 0x00)
        ria.process(&BusTransaction::write(3, 0xFFEF, 0x00));
        assert_eq!(ria.xstack_ptr, XSTACK_SIZE);
        assert_eq!(ria.regs[0x0C], 0); // API_STACK = 0
    }

    #[test]
    fn test_op_exit() {
        let (mut ria, _, _) = make_ria();
        ria.process(&BusTransaction::write(1, 0xFFEF, 0xFF));
        assert!(!ria.running);
    }

    #[test]
    fn test_irq_enable_and_ack() {
        let (mut ria, _, back_tx) = make_ria();
        // Enable IRQ
        ria.process(&BusTransaction::write(1, 0xFFF0, 0x01));
        assert_eq!(ria.irq_enabled, 0x01);
        assert!(ria.irq_pin); // cleared by write

        // Simulate VSYNC backchannel
        back_tx.send(Backchannel::Vsync(0x81)).unwrap();
        ria.poll_backchannel();
        assert!(!ria.irq_pin); // IRQ asserted

        // Acknowledge by reading $FFF0
        ria.process(&BusTransaction::read(2, 0xFFF0, 0));
        assert!(ria.irq_pin); // cleared
    }

    #[test]
    fn test_vsync_preserved_across_reset() {
        let (mut ria, _, _) = make_ria();
        ria.regs[0x03] = 0x42; // set VSYNC counter
        ria.reset();
        assert_eq!(ria.regs[0x03], 0x42); // preserved
    }
}
