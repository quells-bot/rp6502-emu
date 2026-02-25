# Headless Screenshot Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `screenshot` CLI subcommand that renders a test pattern framebuffer as a PNG without launching the egui window.

**Architecture:** Add `clap` for CLI parsing (default = egui GUI, `screenshot` subcommand = headless). Reuse the existing threaded RIA+VGA pipeline. Add `png` crate for encoding the 640x480 RGBA framebuffer.

**Tech Stack:** clap (derive), png crate, existing RIA/VGA threading

---

### Task 1: Add dependencies

**Files:**
- Modify: `emu/Cargo.toml`

**Step 1: Add clap and png to Cargo.toml**

Add to `[dependencies]`:

```toml
clap = { version = "4", features = ["derive"] }
png = "0.17"
```

**Step 2: Verify it compiles**

Run: `cargo check` from `emu/`
Expected: compiles with no errors (warnings OK)

**Step 3: Commit**

```bash
git add emu/Cargo.toml emu/Cargo.lock
git commit -m "chore: add clap and png dependencies"
```

---

### Task 2: Add FromStr for TestMode

**Files:**
- Modify: `emu/src/test_harness.rs`
- Test: existing tests still pass, plus new parsing test

**Step 1: Write the failing test**

Add to the `tests` module in `test_harness.rs`:

```rust
#[test]
fn test_mode_from_str() {
    assert!(matches!("mono640x480".parse::<TestMode>(), Ok(TestMode::Mono640x480)));
    assert!(matches!("color8bpp320x180".parse::<TestMode>(), Ok(TestMode::Color8bpp320x180)));
    assert!(matches!("color16bpp320".parse::<TestMode>(), Ok(TestMode::Color16bpp320)));
    assert!("invalid".parse::<TestMode>().is_err());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p rp6502-emu test_mode_from_str`
Expected: FAIL — `FromStr` not implemented

**Step 3: Implement FromStr**

Add above the `impl TestMode` block:

```rust
impl std::fmt::Display for TestMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            TestMode::Mono640x480 => "mono640x480",
            TestMode::Mono640x360 => "mono640x360",
            TestMode::Mono320x240 => "mono320x240",
            TestMode::Mono320x180 => "mono320x180",
            TestMode::Color2bpp640x360 => "color2bpp640x360",
            TestMode::Color2bpp320x240 => "color2bpp320x240",
            TestMode::Color2bpp320x180 => "color2bpp320x180",
            TestMode::Color4bpp320x240 => "color4bpp320x240",
            TestMode::Color4bpp320x180 => "color4bpp320x180",
            TestMode::Color8bpp320x180 => "color8bpp320x180",
            TestMode::Color16bpp320 => "color16bpp320",
        };
        write!(f, "{}", name)
    }
}

impl std::str::FromStr for TestMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "mono640x480" => Ok(TestMode::Mono640x480),
            "mono640x360" => Ok(TestMode::Mono640x360),
            "mono320x240" => Ok(TestMode::Mono320x240),
            "mono320x180" => Ok(TestMode::Mono320x180),
            "color2bpp640x360" => Ok(TestMode::Color2bpp640x360),
            "color2bpp320x240" => Ok(TestMode::Color2bpp320x240),
            "color2bpp320x180" => Ok(TestMode::Color2bpp320x180),
            "color4bpp320x240" => Ok(TestMode::Color4bpp320x240),
            "color4bpp320x180" => Ok(TestMode::Color4bpp320x180),
            "color8bpp320x180" => Ok(TestMode::Color8bpp320x180),
            "color16bpp320" => Ok(TestMode::Color16bpp320),
            _ => Err(format!(
                "unknown mode '{}'. Valid modes: {}",
                s,
                TestMode::all().iter().map(|m| m.to_string()).collect::<Vec<_>>().join(", ")
            )),
        }
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p rp6502-emu test_mode_from_str`
Expected: PASS

Run: `cargo test -p rp6502-emu` (all tests)
Expected: all pass

**Step 5: Commit**

```bash
git add emu/src/test_harness.rs
git commit -m "feat: add Display and FromStr for TestMode"
```

---

### Task 3: Add screenshot module (PNG encoding)

**Files:**
- Create: `emu/src/screenshot.rs`

**Step 1: Write the failing test**

Create `emu/src/screenshot.rs` with:

```rust
use std::fs;
use std::io::BufWriter;
use std::path::Path;

/// Encode a 640x480 RGBA framebuffer as a PNG file.
pub fn save_png(path: &Path, rgba_data: &[u8], width: u32, height: u32) -> Result<(), Box<dyn std::error::Error>> {
    let file = fs::File::create(path)?;
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgba_data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_save_png_creates_valid_file() {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let path = dir.join("test_output.png");

        // 2x2 RGBA image: red, green, blue, white
        let data: [u8; 16] = [
            255, 0, 0, 255,     // red
            0, 255, 0, 255,     // green
            0, 0, 255, 255,     // blue
            255, 255, 255, 255, // white
        ];

        save_png(&path, &data, 2, 2).expect("should write PNG");
        assert!(path.exists());

        // Verify it's a valid PNG by checking the magic bytes
        let bytes = fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], &[0x89, 0x50, 0x4E, 0x47]); // PNG magic

        fs::remove_file(&path).ok();
    }
}
```

Also add `mod screenshot;` to `src/main.rs` (after the existing mod declarations).

**Step 2: Run test to verify it passes**

Run: `cargo test -p rp6502-emu test_save_png`
Expected: PASS (this is implementation + test together since the function is simple)

**Step 3: Commit**

```bash
git add emu/src/screenshot.rs emu/src/main.rs
git commit -m "feat: add screenshot module with PNG encoding"
```

---

### Task 4: Wire up clap CLI and screenshot subcommand

**Files:**
- Modify: `emu/src/main.rs`

**Step 1: Rewrite main.rs with clap CLI**

Replace the contents of `main.rs`. Key changes:
- Add `clap` derive structs: `Cli` with an optional `Command` enum
- Default (no subcommand) runs egui as before
- `screenshot` subcommand: spawns RIA+VGA threads, joins RIA thread, grabs framebuffer, calls `save_png`

```rust
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
```

**Step 2: Verify it compiles**

Run: `cargo check` from `emu/`
Expected: compiles with no errors

**Step 3: Run all tests**

Run: `cargo test -p rp6502-emu`
Expected: all existing tests pass

**Step 4: Manual smoke test**

Run: `cargo run -p rp6502-emu -- screenshot --mode mono320x240 -o /tmp/test_screenshot.png`
Expected: prints "Screenshot saved to /tmp/test_screenshot.png", file exists and is a valid PNG

Run: `cargo run -p rp6502-emu -- screenshot --mode color8bpp320x180 -o /tmp/test_8bpp.png`
Expected: prints success, second PNG created

Verify help text:
Run: `cargo run -p rp6502-emu -- --help`
Expected: shows both default and screenshot subcommand

Run: `cargo run -p rp6502-emu -- screenshot --help`
Expected: shows --mode and --output flags

**Step 5: Commit**

```bash
git add emu/src/main.rs
git commit -m "feat: add screenshot subcommand with clap CLI"
```

---

### Task 5: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md` (project root)

**Step 1: Add screenshot usage to CLAUDE.md**

In the "Emulator Source" section, add after the module layout table:

```markdown
### CLI usage

```
cargo run                                              # launch egui window (default)
cargo run -- screenshot --mode mono320x240 -o out.png  # headless screenshot
```

Valid `--mode` values: `mono640x480`, `mono640x360`, `mono320x240`, `mono320x180`, `color2bpp640x360`, `color2bpp320x240`, `color2bpp320x180`, `color4bpp320x240`, `color4bpp320x180`, `color8bpp320x180`, `color16bpp320`.
```

Also add `screenshot.rs` to the module layout table:

```markdown
| `src/screenshot.rs` | PNG encoding for headless framebuffer export |
```

**Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add screenshot CLI usage to CLAUDE.md"
```
