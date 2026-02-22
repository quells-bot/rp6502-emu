/// A single 6502 bus transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusTransaction {
    pub cycle: u64,
    pub addr: u16,
    pub data: u8,
    /// true = read (6502 reading from bus), false = write (6502 writing to bus)
    pub rw: bool,
}

impl BusTransaction {
    pub fn write(cycle: u64, addr: u16, data: u8) -> Self {
        Self { cycle, addr, data, rw: false }
    }

    pub fn read(cycle: u64, addr: u16, data: u8) -> Self {
        Self { cycle, addr, data, rw: true }
    }

    /// Returns true if this transaction targets the RIA register space ($FFE0-$FFFF).
    pub fn hits_ria(&self) -> bool {
        self.addr >= 0xFFE0
    }

    /// Returns the RIA register index (0-31) for this address.
    pub fn ria_reg(&self) -> u8 {
        (self.addr & 0x1F) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hits_ria() {
        assert!(BusTransaction::write(0, 0xFFE4, 0x42).hits_ria());
        assert!(!BusTransaction::write(0, 0x1000, 0x42).hits_ria());
    }

    #[test]
    fn test_ria_reg() {
        assert_eq!(BusTransaction::write(0, 0xFFE4, 0).ria_reg(), 0x04);
        assert_eq!(BusTransaction::write(0, 0xFFFF, 0).ria_reg(), 0x1F);
        assert_eq!(BusTransaction::write(0, 0xFFE0, 0).ria_reg(), 0x00);
    }
}
