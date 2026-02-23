mod bus;
mod pix;
mod ria;
mod screenshot;
mod test_harness;
mod vga;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use clap::{Parser, Subcommand};
use eframe::egui;
use crate::vga::Vga;

#[derive(Parser)]
#[command(name = "rp6502-emu", about = "RP6502 Picocomputer Emulator")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Render a test pattern to a PNG file (headless, no window)
    Screenshot {
        /// Test mode name (e.g. mono320x240, color8bpp320x180)
        #[arg(long)]
        mode: test_harness::TestMode,
        /// Output PNG file path
        #[arg(short, long)]
        output: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Screenshot { mode, output }) => {
            run_screenshot(mode, &output);
        }
        None => {
            run_gui();
        }
    }
}

fn run_screenshot(mode: test_harness::TestMode, output: &std::path::Path) {
    let framebuffer: Arc<Mutex<Vec<u8>>> =
        Arc::new(Mutex::new(vec![0u8; 640 * 480 * 4]));

    let (pix_tx, pix_rx) = crossbeam_channel::unbounded();
    let (back_tx, back_rx) = crossbeam_channel::unbounded();

    let fb_vga = framebuffer.clone();
    thread::spawn(move || {
        let mut vga = Vga::new(pix_rx, back_tx, fb_vga);
        vga.run();
    });

    // Run RIA on a joinable thread
    let ria_handle = thread::spawn(move || {
        let mut ria_state = ria::Ria::new(pix_tx, back_rx);
        let trace = test_harness::generate_test_trace(mode);
        for txn in &trace {
            if !ria_state.running {
                break;
            }
            ria_state.process(txn);
        }
        // pix_tx is dropped here, which causes VGA thread to exit
    });

    ria_handle.join().expect("RIA thread panicked");

    // Small delay to let VGA thread finish processing final events
    thread::sleep(std::time::Duration::from_millis(100));

    let fb = framebuffer.lock().expect("framebuffer lock poisoned");
    screenshot::save_png(output, &fb, 640, 480)
        .expect("failed to write PNG");

    println!("Screenshot saved to {}", output.display());
}

fn run_gui() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([660.0, 500.0])
            .with_title("RP6502 Emulator"),
        ..Default::default()
    };

    let framebuffer: Arc<Mutex<Vec<u8>>> =
        Arc::new(Mutex::new(vec![0u8; 640 * 480 * 4]));

    let (pix_tx, pix_rx) = crossbeam_channel::unbounded();
    let (back_tx, back_rx) = crossbeam_channel::unbounded();

    let fb_vga = framebuffer.clone();
    thread::spawn(move || {
        let mut vga = Vga::new(pix_rx, back_tx, fb_vga);
        vga.run();
    });

    thread::spawn(move || {
        let mut ria_state = ria::Ria::new(pix_tx, back_rx);
        let trace = test_harness::generate_test_trace(test_harness::TestMode::Mono320x240);
        for txn in &trace {
            if !ria_state.running {
                break;
            }
            ria_state.process(txn);
        }
    });

    eframe::run_native(
        "rp6502-emu",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(EmulatorApp {
                framebuffer,
                texture: None,
            }))
        }),
    ).expect("eframe failed");
}

struct EmulatorApp {
    framebuffer: Arc<Mutex<Vec<u8>>>,
    texture: Option<egui::TextureHandle>,
}

impl eframe::App for EmulatorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("RP6502 Emulator");

            let pixels = if let Ok(fb) = self.framebuffer.lock() {
                fb.clone()
            } else {
                vec![0u8; 640 * 480 * 4]
            };

            let image = egui::ColorImage::from_rgba_unmultiplied(
                [640, 480],
                &pixels,
            );

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

        ctx.request_repaint();
    }
}
