pub mod font;
pub mod mode1;
pub mod mode2;
pub mod mode3;
pub mod palette;

use std::sync::{Arc, Mutex};
use crossbeam_channel::{Receiver, Sender};
use crate::pix::{Backchannel, PixEvent, PixRegWrite};
use mode1::{Mode1Config, Mode1Format, Mode1Plane, render_mode1};
use mode3::{ColorFormat, Mode3Config, Mode3Plane, render_mode3};
use mode2::{Mode2Config, Mode2Format, Mode2Plane, render_mode2};

/// Display output is always 640x480.
const DISPLAY_WIDTH: usize = 640;
const DISPLAY_HEIGHT: usize = 480;

/// Upscale canvas buffer to the 640x480 display buffer.
///
/// Integer scale factors are derived from canvas dimensions:
/// - 320-wide canvases: 2x horizontal and vertical
/// - 640-wide canvases: 1x (direct copy)
/// - 16:9 canvases (height 180 or 360): top-aligned, black fills remaining scanlines
///
/// The u32 pixel format is R in bits 31:24, G in 23:16, B in 15:8, A in 7:0.
/// Manual shift-and-mask is used (NOT bytemuck::cast_slice, which would give
/// wrong byte order on little-endian targets due to this u32 packing).
fn upscale_canvas(canvas: &[u32], canvas_w: u16, canvas_h: u16, display: &mut [u8]) {
    let cw = canvas_w as usize;
    let ch = canvas_h as usize;
    let sx = DISPLAY_WIDTH / cw;
    let sy = DISPLAY_HEIGHT / ch.max(1);

    // Clear entire display to black (handles letterbox regions for 16:9)
    display.fill(0);

    for cy in 0..ch {
        for cx in 0..cw {
            let pixel = canvas[cy * cw + cx];
            let r = (pixel >> 24) as u8;
            let g = (pixel >> 16) as u8;
            let b = (pixel >> 8) as u8;
            let a = (pixel & 0xFF) as u8;

            for dy in 0..sy {
                let display_y = cy * sy + dy;
                if display_y >= DISPLAY_HEIGHT {
                    break;
                }
                for dx in 0..sx {
                    let display_x = cx * sx + dx;
                    if display_x >= DISPLAY_WIDTH {
                        break;
                    }
                    let idx = (display_y * DISPLAY_WIDTH + display_x) * 4;
                    display[idx]     = r;
                    display[idx + 1] = g;
                    display[idx + 2] = b;
                    display[idx + 3] = a;
                }
            }
        }
    }
}

/// A programmed display plane, which may be Mode 1 or Mode 3.
#[derive(Debug, Clone)]
pub enum Plane {
    Mode1(Mode1Plane),
    Mode2(Mode2Plane),
    Mode3(Mode3Plane),
}

/// VGA state machine.
pub struct Vga {
    pub xram: Box<[u8; 65536]>,
    pub planes: [Option<Plane>; 3],
    pub canvas_width: u16,
    pub canvas_height: u16,
    xregs: [u16; 8],
    pix_rx: Receiver<PixEvent>,
    backchannel_tx: Sender<Backchannel>,
    framebuffer: Arc<Mutex<Vec<u8>>>,
    frame_count: u8,
    canvas_buf: Vec<u32>,
}

impl Vga {
    pub fn new(
        pix_rx: Receiver<PixEvent>,
        backchannel_tx: Sender<Backchannel>,
        framebuffer: Arc<Mutex<Vec<u8>>>,
    ) -> Self {
        let canvas_width = 640;
        let canvas_height = 480;
        Self {
            xram: Box::new([0; 65536]),
            planes: [None, None, None],
            canvas_width,
            canvas_height,
            xregs: [0; 8],
            pix_rx,
            backchannel_tx,
            framebuffer,
            frame_count: 0,
            canvas_buf: vec![0u32; DISPLAY_WIDTH * DISPLAY_HEIGHT],
        }
    }

    /// Run the VGA event loop. Call from a dedicated thread.
    pub fn run(&mut self) {
        while let Ok(event) = self.pix_rx.recv() {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: PixEvent) {
        match event {
            PixEvent::Xram(write) => {
                self.xram[write.addr as usize] = write.data;
            }
            PixEvent::Reg(reg) => {
                self.handle_reg(reg);
            }
            PixEvent::FrameSync => {
                self.render_frame();
                self.frame_count = self.frame_count.wrapping_add(1);
                let _ = self.backchannel_tx.send(
                    Backchannel::Vsync(0x80 | (self.frame_count & 0x0F))
                );
            }
        }
    }

    /// Handle a PIX register write.
    /// Matches firmware vga/sys/pix.c pix_ch0_xreg().
    fn handle_reg(&mut self, reg: PixRegWrite) {
        if reg.channel == 0 {
            // Accumulate xregs for registers 2-7
            if (reg.register as usize) < self.xregs.len() {
                self.xregs[reg.register as usize] = reg.value;
            }

            match reg.register {
                0 => {
                    // CANVAS - configure canvas size
                    match reg.value {
                        1 => { self.canvas_width = 320; self.canvas_height = 240; }
                        2 => { self.canvas_width = 320; self.canvas_height = 180; }
                        3 => { self.canvas_width = 640; self.canvas_height = 480; }
                        4 => { self.canvas_width = 640; self.canvas_height = 360; }
                        _ => { self.canvas_width = 640; self.canvas_height = 480; }
                    }
                    // Reset all planes
                    self.planes = [None, None, None];
                    self.xregs = [0; 8];
                    let _ = self.backchannel_tx.send(Backchannel::Ack);
                }
                1 => {
                    // MODE - program a graphics mode
                    let mode = reg.value;
                    match mode {
                        1 => {
                            self.program_mode1();
                            let _ = self.backchannel_tx.send(Backchannel::Ack);
                        }
                        2 => {
                            self.program_mode2();
                            let _ = self.backchannel_tx.send(Backchannel::Ack);
                        }
                        3 => {
                            self.program_mode3();
                            let _ = self.backchannel_tx.send(Backchannel::Ack);
                        }
                        _ => {
                            let _ = self.backchannel_tx.send(Backchannel::Nak);
                        }
                    }
                    self.xregs = [0; 8];
                }
                _ => {
                    // Registers 2-7: accumulate into xregs, no ack needed
                }
            }
        }
        // Channel 15: display config, code page, backchannel control - ignored in MVP
    }


    /// Program Mode 2 from accumulated xregs.
    /// xregs layout for MODE command:
    ///   xregs[2] = attributes (tile format)
    ///   xregs[3] = config_ptr (XRAM address of Mode2Config)
    ///   xregs[4] = plane index (0-2)
    ///   xregs[5] = scanline_begin
    ///   xregs[6] = scanline_end (0 = canvas height)
    fn program_mode2(&mut self) {
        let attr = self.xregs[2];
        let config_ptr = self.xregs[3];
        let plane_idx = self.xregs[4] as usize;
        let scanline_begin = self.xregs[5];
        let scanline_end = self.xregs[6];

        if plane_idx >= 3 || config_ptr & 1 != 0 {
            return;
        }

        if config_ptr as usize + 16 > 0x10000 {
            return;
        }

        let format = match Mode2Format::from_attr(attr) {
            Some(f) => f,
            None => return,
        };

        let config = Mode2Config::from_xram(&self.xram, config_ptr);

        self.planes[plane_idx] = Some(Plane::Mode2(Mode2Plane {
            config,
            format,
            scanline_begin,
            scanline_end,
            config_ptr,
        }));
    }

    /// Program Mode 3 from accumulated xregs.
    /// xregs layout for MODE command:
    ///   xregs[2] = attributes (color format)
    ///   xregs[3] = config_ptr (XRAM address of Mode3Config)
    ///   xregs[4] = plane index (0-2)
    ///   xregs[5] = scanline_begin
    ///   xregs[6] = scanline_end (0 = canvas height)
    fn program_mode3(&mut self) {
        let attr = self.xregs[2];
        let config_ptr = self.xregs[3];
        let plane_idx = self.xregs[4] as usize;
        let scanline_begin = self.xregs[5];
        let scanline_end = self.xregs[6];

        if plane_idx >= 3 || config_ptr & 1 != 0 {
            return;
        }

        let format = match ColorFormat::from_attr(attr) {
            Some(f) => f,
            None => return,
        };

        let config = Mode3Config::from_xram(&self.xram, config_ptr);

        self.planes[plane_idx] = Some(Plane::Mode3(Mode3Plane {
            config,
            format,
            scanline_begin,
            scanline_end,
            config_ptr,
        }));
    }

    /// Program Mode 1 from accumulated xregs.
    /// Same xregs layout as Mode 3:
    ///   xregs[2] = attributes (format: font size + color depth)
    ///   xregs[3] = config_ptr (XRAM address of Mode1Config)
    ///   xregs[4] = plane index (0-2)
    ///   xregs[5] = scanline_begin
    ///   xregs[6] = scanline_end (0 = canvas height)
    fn program_mode1(&mut self) {
        let attr = self.xregs[2];
        let config_ptr = self.xregs[3];
        let plane_idx = self.xregs[4] as usize;
        let scanline_begin = self.xregs[5];
        let scanline_end = self.xregs[6];

        if plane_idx >= 3 || config_ptr & 1 != 0 {
            return;
        }

        // Additional firmware check: config_ptr must leave room for the 16-byte struct
        if config_ptr as usize + 16 > 0x10000 {
            return;
        }

        let format = match Mode1Format::from_attr(attr) {
            Some(f) => f,
            None => return,
        };

        let config = Mode1Config::from_xram(&self.xram, config_ptr);

        self.planes[plane_idx] = Some(Plane::Mode1(Mode1Plane {
            config,
            format,
            scanline_begin,
            scanline_end,
            config_ptr,
        }));
    }

    /// Render all planes to the framebuffer.
    fn render_frame(&mut self) {
        let w = self.canvas_width;
        let h = self.canvas_height;
        let pixel_count = w as usize * h as usize;

        // Clear canvas buffer (only the used portion)
        self.canvas_buf[..pixel_count].fill(0);

        // Render each plane into canvas buffer
        for plane in self.planes.iter().flatten() {
            match plane {
                Plane::Mode1(p) => {
                    let fresh_config = Mode1Config::from_xram(&self.xram, p.config_ptr);
                    let current_plane = Mode1Plane { config: fresh_config, ..p.clone() };
                    render_mode1(&current_plane, &self.xram, &mut self.canvas_buf[..pixel_count], w, h);
                }
                Plane::Mode2(p) => {
                    let fresh_config = Mode2Config::from_xram(&self.xram, p.config_ptr);
                    let current_plane = Mode2Plane { config: fresh_config, ..p.clone() };
                    render_mode2(&current_plane, &self.xram, &mut self.canvas_buf[..pixel_count], w, h);
                }
                Plane::Mode3(p) => {
                    let fresh_config = Mode3Config::from_xram(&self.xram, p.config_ptr);
                    let current_plane = Mode3Plane { config: fresh_config, ..p.clone() };
                    render_mode3(&current_plane, &self.xram, &mut self.canvas_buf[..pixel_count], w, h);
                }
            }
        }

        // Upscale canvas to 640x480 display buffer
        let mut display = vec![0u8; DISPLAY_WIDTH * DISPLAY_HEIGHT * 4];
        upscale_canvas(&self.canvas_buf[..pixel_count], w, h, &mut display);

        if let Ok(mut fb) = self.framebuffer.lock() {
            *fb = display;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upscale_1x() {
        // 640x480 canvas -> 1x scale, direct copy
        let mut canvas = vec![0u32; 640 * 480];
        canvas[0] = 0xFF0000FF; // R=FF, G=00, B=00, A=FF at (0,0)
        canvas[639 + 479 * 640] = 0x00FF00FF; // green at (639,479)

        let mut display = vec![0u8; 640 * 480 * 4];
        upscale_canvas(&canvas, 640, 480, &mut display);

        assert_eq!(display[0], 0xFF); // R
        assert_eq!(display[1], 0x00); // G
        assert_eq!(display[2], 0x00); // B
        assert_eq!(display[3], 0xFF); // A

        let idx = (639 + 479 * 640) * 4;
        assert_eq!(display[idx], 0x00);
        assert_eq!(display[idx + 1], 0xFF);
        assert_eq!(display[idx + 2], 0x00);
        assert_eq!(display[idx + 3], 0xFF);
    }

    #[test]
    fn test_upscale_2x() {
        // 320x240 canvas -> 2x scale
        let mut canvas = vec![0u32; 320 * 240];
        canvas[0] = 0x0000FFFF; // blue at (0,0)
        canvas[1] = 0xFF0000FF; // red at (1,0)

        let mut display = vec![0u8; 640 * 480 * 4];
        upscale_canvas(&canvas, 320, 240, &mut display);

        // (0,0) in canvas -> 2x2 block at (0,0),(1,0),(0,1),(1,1) in display
        for (dx, dy) in [(0usize, 0usize), (1, 0), (0, 1), (1, 1)] {
            let idx = (dx + dy * 640) * 4;
            assert_eq!(display[idx], 0x00, "R at ({dx},{dy})");
            assert_eq!(display[idx + 1], 0x00, "G at ({dx},{dy})");
            assert_eq!(display[idx + 2], 0xFF, "B at ({dx},{dy})");
            assert_eq!(display[idx + 3], 0xFF, "A at ({dx},{dy})");
        }

        // (1,0) in canvas -> 2x2 block at (2,0),(3,0),(2,1),(3,1) in display
        for (dx, dy) in [(2usize, 0usize), (3, 0), (2, 1), (3, 1)] {
            let idx = (dx + dy * 640) * 4;
            assert_eq!(display[idx], 0xFF, "R at ({dx},{dy})");
            assert_eq!(display[idx + 1], 0x00, "G at ({dx},{dy})");
            assert_eq!(display[idx + 2], 0x00, "B at ({dx},{dy})");
            assert_eq!(display[idx + 3], 0xFF, "A at ({dx},{dy})");
        }
    }

    #[test]
    fn test_upscale_2x_16_9_black_below() {
        // 320x180 canvas -> 2x scale, content fills 640x360, black below
        let mut canvas = vec![0u32; 320 * 180];
        canvas[0] = 0xFF0000FF; // red at (0,0)

        let mut display = vec![0u8; 640 * 480 * 4];
        upscale_canvas(&canvas, 320, 180, &mut display);

        // (0,0) should be red
        assert_eq!(display[0], 0xFF);
        assert_eq!(display[3], 0xFF);

        // Scanline 360 (first line below content) should be black
        let idx = 360 * 640 * 4;
        assert_eq!(display[idx], 0x00);
        assert_eq!(display[idx + 1], 0x00);
        assert_eq!(display[idx + 2], 0x00);
        assert_eq!(display[idx + 3], 0x00);
    }
}
