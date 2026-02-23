use std::fs;
use std::io::BufWriter;
use std::path::Path;

/// Encode an RGBA framebuffer as a PNG file.
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

    #[test]
    fn test_save_png_creates_valid_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("rp6502_test_output.png");

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
