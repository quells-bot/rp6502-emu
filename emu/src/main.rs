mod bus;
mod pix;
mod ria;
mod vga;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([660.0, 500.0])
            .with_title("RP6502 Emulator"),
        ..Default::default()
    };
    eframe::run_native(
        "rp6502-emu",
        options,
        Box::new(|_cc| Ok(Box::new(EmulatorApp::default()))),
    )
}

#[derive(Default)]
struct EmulatorApp {
    texture: Option<egui::TextureHandle>,
}

impl eframe::App for EmulatorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("RP6502 Emulator");
            // Blue 640x480 test pattern
            let width = 640;
            let height = 480;
            let mut pixels = vec![0u8; width * height * 4];
            for y in 0..height {
                for x in 0..width {
                    let i = (y * width + x) * 4;
                    pixels[i] = (x % 256) as u8;
                    pixels[i + 1] = (y % 256) as u8;
                    pixels[i + 2] = 128;
                    pixels[i + 3] = 255;
                }
            }
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [width, height],
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
                        .fit_to_exact_size(egui::vec2(640.0, 480.0))
                );
            }
        });
    }
}
