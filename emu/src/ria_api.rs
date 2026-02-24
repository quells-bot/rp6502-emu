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
}
