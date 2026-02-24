#!/usr/bin/env python3
"""Extract CP437 font data from firmware font.c and generate Rust source."""

import re
import sys
from pathlib import Path

def extract_hex_array(source: str, name: str) -> list[int]:
    """Extract a C hex array by name, return list of byte values."""
    # Match: static const ... uint8_t NAME[] = { ... };
    pattern = rf'uint8_t\s+{name}\s*\[\s*\]\s*=\s*\{{([^}}]+)\}}'
    m = re.search(pattern, source, re.DOTALL)
    if not m:
        raise ValueError(f"Array {name} not found")
    hex_str = m.group(1)
    # Parse hex values like 0x00, 0x7e, etc.
    return [int(x, 16) for x in re.findall(r'0x[0-9a-fA-F]+', hex_str)]

def interleave_font(ascii_data: list[int], cp_data: list[int], rows: int) -> list[int]:
    """Combine ASCII (glyphs 0-127) and code page (glyphs 128-255) into wide format.

    Input: each array is rows * 128 bytes (128 bytes per row, one byte per glyph).
    Output: rows * 256 bytes (256 bytes per row, full glyph set).

    This matches firmware font_init():
      font[row*256 + 0..127]   = ASCII[row*128 + 0..127]
      font[row*256 + 128..255] = CP[row*128 + 0..127]
    """
    assert len(ascii_data) == rows * 128, f"ASCII: expected {rows*128}, got {len(ascii_data)}"
    assert len(cp_data) == rows * 128, f"CP: expected {rows*128}, got {len(cp_data)}"

    result = []
    for row in range(rows):
        result.extend(ascii_data[row*128 : (row+1)*128])
        result.extend(cp_data[row*128 : (row+1)*128])
    return result

def format_rust_array(name: str, data: list[int], line_width: int = 16) -> str:
    """Format as a Rust const array."""
    lines = [f"pub const {name}: [u8; {len(data)}] = ["]
    for i in range(0, len(data), line_width):
        chunk = data[i:i+line_width]
        hex_vals = ", ".join(f"0x{b:02X}" for b in chunk)
        lines.append(f"    {hex_vals},")
    lines.append("];")
    return "\n".join(lines)

def main():
    font_c = Path(__file__).parent.parent.parent / "firmware" / "src" / "vga" / "term" / "font.c"
    if not font_c.exists():
        print(f"Error: {font_c} not found", file=sys.stderr)
        sys.exit(1)

    source = font_c.read_text()

    font8_ascii = extract_hex_array(source, "FONT8_ASCII")
    font8_cp437 = extract_hex_array(source, "FONT8_CP437")
    font16_ascii = extract_hex_array(source, "FONT16_ASCII")
    font16_cp437 = extract_hex_array(source, "FONT16_CP437")

    font8 = interleave_font(font8_ascii, font8_cp437, 8)
    font16 = interleave_font(font16_ascii, font16_cp437, 16)

    assert len(font8) == 2048
    assert len(font16) == 4096

    out = [
        "/// Built-in CP437 (American English) fonts, IBM VGA typeface.",
        "///",
        "/// \"Wide\" format: `font[row * 256 + glyph_code]` gives one byte",
        "/// (8 pixels, MSB = leftmost) for that glyph at that row.",
        "///",
        "/// Generated from firmware/src/vga/term/font.c by scripts/gen_font.py.",
        "/// SPDX-License-Identifier: BSD-3-Clause",
        "",
        f"/// 8x8 font: 256 glyphs, 8 rows per glyph, 1 byte per row.",
        format_rust_array("FONT8", font8),
        "",
        f"/// 8x16 font: 256 glyphs, 16 rows per glyph, 1 byte per row.",
        format_rust_array("FONT16", font16),
        "",
        "#[cfg(test)]",
        "mod tests {",
        "    use super::*;",
        "",
        "    #[test]",
        "    fn test_font8_size() {",
        "        assert_eq!(FONT8.len(), 2048);",
        "    }",
        "",
        "    #[test]",
        "    fn test_font16_size() {",
        "        assert_eq!(FONT16.len(), 4096);",
        "    }",
        "",
        "    #[test]",
        "    fn test_font8_space_is_blank() {",
        "        // Space (glyph 0x20) should be all zeros in 8x8 font",
        "        for row in 0..8 {",
        "            assert_eq!(FONT8[row * 256 + 0x20], 0, \"row {row}\");",
        "        }",
        "    }",
        "",
        "    #[test]",
        "    fn test_font16_space_is_blank() {",
        "        for row in 0..16 {",
        "            assert_eq!(FONT16[row * 256 + 0x20], 0, \"row {row}\");",
        "        }",
        "    }",
        "",
        "    #[test]",
        "    fn test_font8_A_has_content() {",
        "        // 'A' (glyph 0x41) should have non-zero rows",
        "        let mut has_content = false;",
        "        for row in 0..8 {",
        "            if FONT8[row * 256 + 0x41] != 0 {",
        "                has_content = true;",
        "            }",
        "        }",
        "        assert!(has_content, \"glyph 'A' should have visible pixels\");",
        "    }",
        "",
        "    #[test]",
        "    fn test_font16_A_has_content() {",
        "        let mut has_content = false;",
        "        for row in 0..16 {",
        "            if FONT16[row * 256 + 0x41] != 0 {",
        "                has_content = true;",
        "            }",
        "        }",
        "        assert!(has_content, \"glyph 'A' should have visible pixels\");",
        "    }",
        "",
        "    #[test]",
        "    fn test_font8_high_glyph_has_content() {",
        "        // CP437 glyph 0xDB (full block) should be all 0xFF",
        "        for row in 0..8 {",
        "            assert_eq!(FONT8[row * 256 + 0xDB], 0xFF, \"row {row}\");",
        "        }",
        "    }",
        "}",
        "",
    ]

    out_path = Path(__file__).parent.parent / "src" / "vga" / "font.rs"
    out_path.write_text("\n".join(out))
    print(f"Generated {out_path} ({len(font8) + len(font16)} bytes of font data)")

if __name__ == "__main__":
    main()
