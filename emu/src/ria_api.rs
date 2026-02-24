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
