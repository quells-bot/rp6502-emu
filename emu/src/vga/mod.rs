pub mod mode3;
pub mod palette;

use std::sync::{Arc, Mutex};
use crossbeam_channel::{Receiver, Sender};
use crate::pix::{Backchannel, PixEvent, PixRegWrite};
use mode3::{ColorFormat, Mode3Config, Mode3Plane, render_mode3};

/// VGA state machine.
pub struct Vga {
    pub xram: Box<[u8; 65536]>,
    pub planes: [Option<Mode3Plane>; 3],
    pub canvas_width: u16,
    pub canvas_height: u16,
    xregs: [u16; 8],
    pix_rx: Receiver<PixEvent>,
    backchannel_tx: Sender<Backchannel>,
    framebuffer: Arc<Mutex<Vec<u8>>>,
    frame_count: u8,
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
                    if mode == 3 {
                        self.program_mode3();
                        let _ = self.backchannel_tx.send(Backchannel::Ack);
                    } else {
                        // Only Mode 3 supported in MVP
                        let _ = self.backchannel_tx.send(Backchannel::Nak);
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

        self.planes[plane_idx] = Some(Mode3Plane {
            config,
            format,
            scanline_begin,
            scanline_end,
            config_ptr,
        });
    }

    /// Render all planes to the framebuffer.
    fn render_frame(&mut self) {
        let w = self.canvas_width;
        let h = self.canvas_height;
        let pixel_count = w as usize * h as usize;

        let mut fb_rgba = vec![0u32; pixel_count];

        for plane in self.planes.iter().flatten() {
            let fresh_config = Mode3Config::from_xram(&self.xram, plane.config_ptr);
            let current_plane = Mode3Plane { config: fresh_config, ..plane.clone() };
            render_mode3(&current_plane, &self.xram, &mut fb_rgba, w, h);
        }

        // Convert and write to fixed 640x480 display buffer
        // For now: 1:1 copy into top-left, rest stays black
        let mut display = vec![0u8; 640 * 480 * 4];
        for y in 0..h as usize {
            for x in 0..w as usize {
                let src = y * w as usize + x;
                let dst = y * 640 + x;
                let pixel = fb_rgba[src];
                display[dst * 4]     = (pixel >> 24) as u8;
                display[dst * 4 + 1] = (pixel >> 16) as u8;
                display[dst * 4 + 2] = (pixel >> 8)  as u8;
                display[dst * 4 + 3] = (pixel & 0xFF) as u8;
            }
        }

        if let Ok(mut fb) = self.framebuffer.lock() {
            *fb = display;
        }
    }
}
