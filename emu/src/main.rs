mod bus;
mod pix;
mod ria;
mod test_harness;
mod vga;

use std::sync::{Arc, Mutex};
use std::thread;
use eframe::egui;
use crate::vga::Vga;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([660.0, 500.0])
            .with_title("RP6502 Emulator"),
        ..Default::default()
    };

    // Shared framebuffer (RGBA bytes, 640x480)
    let framebuffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(vec![0u8; 640 * 480 * 4]));

    // PIX channel (RIA -> VGA) and backchannel (VGA -> RIA)
    let (pix_tx, pix_rx) = crossbeam_channel::unbounded();
    let (back_tx, back_rx) = crossbeam_channel::unbounded();

    // Spawn VGA thread
    let fb_vga = framebuffer.clone();
    thread::spawn(move || {
        let mut vga = Vga::new(pix_rx, back_tx, fb_vga);
        vga.run();
    });

    // Spawn RIA thread: replay test harness trace
    thread::spawn(move || {
        let mut ria = ria::Ria::new(pix_tx, back_rx);
        let trace = test_harness::generate_gradient_trace();
        for txn in &trace {
            if !ria.running {
                break;
            }
            ria.process(txn);
        }
    });

    // Run egui on the main thread
    eframe::run_native(
        "rp6502-emu",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(EmulatorApp {
                framebuffer,
                texture: None,
            }))
        }),
    )
}

struct EmulatorApp {
    framebuffer: Arc<Mutex<Vec<u8>>>,
    texture: Option<egui::TextureHandle>,
}

impl eframe::App for EmulatorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("RP6502 Emulator");

            // Snapshot the framebuffer
            let pixels = if let Ok(fb) = self.framebuffer.lock() {
                fb.clone()
            } else {
                vec![0u8; 640 * 480 * 4]
            };

            let image = egui::ColorImage::from_rgba_unmultiplied([640, 480], &pixels);

            match &mut self.texture {
                Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
                None => {
                    self.texture = Some(ctx.load_texture(
                        "screen",
                        image,
                        egui::TextureOptions::NEAREST,
                    ));
                }
            }

            if let Some(tex) = &self.texture {
                ui.add(
                    egui::Image::from_texture(tex)
                        .fit_to_exact_size(egui::vec2(640.0, 480.0)),
                );
            }
        });

        // Request repaint to keep updating the display
        ctx.request_repaint();
    }
}
