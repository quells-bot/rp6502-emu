/// XRAM broadcast (device 0, channel 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XramWrite {
    pub addr: u16,
    pub data: u8,
}

/// Register write to a PIX device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixRegWrite {
    pub channel: u8,
    pub register: u8,
    pub value: u16,
}

/// Events sent from RIA to VGA over the PIX channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixEvent {
    Xram(XramWrite),
    Reg(PixRegWrite),
    FrameSync,
}

/// Backchannel messages from VGA to RIA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backchannel {
    Vsync(u8),
    Ack,
    Nak,
}

/// Pack a PIX message into the 32-bit hardware format.
/// Format: [31:29]=device, [28]=1 (framing), [27:24]=channel, [23:16]=register, [15:0]=value
pub fn pix_pack(device: u8, channel: u8, register: u8, value: u16) -> u32 {
    debug_assert!(device < 8, "PIX device must be 0-7");
    debug_assert!(channel < 16, "PIX channel must be 0-15");
    0x1000_0000
        | ((device as u32) << 29)
        | ((channel as u32) << 24)
        | ((register as u32) << 16)
        | (value as u32)
}

/// Unpack a 32-bit PIX message. Returns None if framing bit is not set.
pub fn pix_unpack(raw: u32) -> Option<(u8, u8, u8, u16)> {
    if raw & 0x1000_0000 == 0 {
        return None;
    }
    let device = ((raw >> 29) & 0x7) as u8;
    let channel = ((raw >> 24) & 0xF) as u8;
    let register = ((raw >> 16) & 0xFF) as u8;
    let value = (raw & 0xFFFF) as u16;
    Some((device, channel, register, value))
}

/// Pack an XRAM write into PIX format.
/// Matches firmware: PIX_SEND_XRAM(addr, data) = PIX_MESSAGE(0, 0, data, addr)
/// Note: data goes in the register field (bits 23:16), addr in value field (bits 15:0).
pub fn pix_pack_xram(addr: u16, data: u8) -> u32 {
    pix_pack(0, 0, data, addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pix_pack_roundtrip() {
        let raw = pix_pack(1, 0, 0x42, 0x1234);
        let (dev, ch, reg, val) = pix_unpack(raw).unwrap();
        assert_eq!(dev, 1);
        assert_eq!(ch, 0);
        assert_eq!(reg, 0x42);
        assert_eq!(val, 0x1234);
    }

    #[test]
    fn test_pix_pack_vga_canvas() {
        // VGA canvas 640x480 = device 1, channel 0, register 0, value 3
        let raw = pix_pack(1, 0, 0, 3);
        assert_eq!(raw, 0x3000_0003);
    }

    #[test]
    fn test_pix_pack_xram() {
        // XRAM write: addr=0x1234, data=0xAB
        // Matches firmware: PIX_MESSAGE(0, 0, 0xAB, 0x1234)
        let raw = pix_pack_xram(0x1234, 0xAB);
        assert_eq!(raw, 0x10AB_1234);
    }

    #[test]
    fn test_pix_unpack_invalid_framing() {
        assert_eq!(pix_unpack(0x0000_0000), None);
    }

    #[test]
    fn test_pix_pack_idle() {
        // Device 7 idle frame
        let raw = pix_pack(7, 0, 0, 0);
        assert_eq!(raw, 0xF000_0000);
    }
}
