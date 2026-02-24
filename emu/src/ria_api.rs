use crate::bus::BusTransaction;

pub struct TraceBuilder {
    pub trace: Vec<BusTransaction>,
    pub cycle: u64,
}

impl TraceBuilder {
    pub fn new() -> Self {
        Self { trace: Vec::new(), cycle: 0 }
    }

    /// Single bus write — mirrors `RIA.reg = val`.
    pub fn write(&mut self, addr: u16, data: u8) {
        self.trace.push(BusTransaction::write(self.cycle, addr, data));
        self.cycle += 1;
    }

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
}
