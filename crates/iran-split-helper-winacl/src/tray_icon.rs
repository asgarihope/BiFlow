//! Renders the live download/upload rate into a 32×32 RGBA buffer that the
//! desktop host feeds back to Tauri as the tray icon image.
//!
//! The icon is the BiFlow tray icon itself — no separate overlay window.
//! On every traffic sample we redraw a tiny two-row panel:
//!
//! ```text
//! ┌──────────────────┐
//! │ D │ ▓▓▓▓▓░░░░░░░░ │  ← cyan "D" + download bar
//! │ U │ ▓▓▓░░░░░░░░░░ │  ← orange "U" + upload bar
//! └──────────────────┘
//! ```
//!
//! Bytes are laid out top-down in RGBA order to match what
//! `tauri::image::Image::new` expects.

/// Final tray icon dimensions and RGBA stride.
pub const ICON_WIDTH: u32 = 32;
pub const ICON_HEIGHT: u32 = 32;
pub const STRIDE_BYTES: usize = (ICON_WIDTH as usize) * 4;

/// How much of the icon width the rate bar reaches at the saturation point
/// (≈16 MB/s). Below this rate the bar grows linearly; above it stays
/// full. The 16 MB/s ceiling matches a typical Iranian home link so
/// 1 MB/s → ~12 % fill, 4 MB/s → ~25 % fill, and a saturated 8 MB/s
/// line fills the bar without an unrealistic 256 MB/s peak.
pub const SATURATION_BPS: u64 = 16 * 1024 * 1024;

/// Returns a 32×32 RGBA buffer with two stacked rows (`D` over `U`) that
/// show the live download and upload speed. Bytes are top-down in RGBA
/// order.
#[must_use]
pub fn render_rate_icon(download_bps: u64, upload_bps: u64) -> Vec<u8> {
    let mut buffer = vec![0u8; STRIDE_BYTES * ICON_HEIGHT as usize];
    paint_background(&mut buffer);
    paint_border(&mut buffer);
    paint_label(&mut buffer, 'D', 0, ICON_HEIGHT / 2, LABEL_COLOR_DOWNLOAD);
    paint_label(
        &mut buffer,
        'U',
        ICON_HEIGHT / 2,
        ICON_HEIGHT,
        LABEL_COLOR_UPLOAD,
    );
    paint_bar(
        &mut buffer,
        0,
        ICON_HEIGHT / 2,
        download_bps,
        BAR_COLOR_DOWNLOAD,
    );
    paint_bar(
        &mut buffer,
        ICON_HEIGHT / 2,
        ICON_HEIGHT,
        upload_bps,
        BAR_COLOR_UPLOAD,
    );
    buffer
}

const LABEL_COLOR_DOWNLOAD: [u8; 4] = [120, 230, 255, 255];
const LABEL_COLOR_UPLOAD: [u8; 4] = [255, 184, 96, 255];
const BAR_COLOR_DOWNLOAD: [u8; 4] = [80, 220, 255, 255];
const BAR_COLOR_UPLOAD: [u8; 4] = [255, 168, 64, 255];
const PANEL_COLOR: [u8; 4] = [18, 26, 46, 235];
const BORDER_COLOR: [u8; 4] = [6, 10, 20, 255];

fn paint_background(buffer: &mut [u8]) {
    for chunk in buffer.chunks_exact_mut(4) {
        chunk.copy_from_slice(&PANEL_COLOR);
    }
}

fn paint_border(buffer: &mut [u8]) {
    for x in 0..ICON_WIDTH {
        write_pixel(buffer, x, 0, BORDER_COLOR);
        write_pixel(buffer, x, ICON_HEIGHT - 1, BORDER_COLOR);
    }
    for y in 0..ICON_HEIGHT {
        write_pixel(buffer, 0, y, BORDER_COLOR);
        write_pixel(buffer, ICON_WIDTH - 1, y, BORDER_COLOR);
    }
}

/// Draw a 7×9 bitmap glyph for the given letter into the panel row
/// spanning `top..bottom`. The glyph sits in the leftmost 8 pixels so
/// the right-hand side stays free for the rate bar.
fn paint_label(buffer: &mut [u8], letter: char, top: u32, bottom: u32, color: [u8; 4]) {
    let row_height = bottom - top;
    let glyph = match letter {
        'D' => GLYPH_D,
        'U' => GLYPH_U,
        _ => return,
    };
    let vertical_offset = (row_height as i32 - GLYPH_HEIGHT as i32) / 2;
    let horizontal_offset = 2_i32;
    for (row, line) in glyph.iter().enumerate() {
        let y = top as i32 + vertical_offset + row as i32;
        if y < top as i32 || y >= bottom as i32 {
            continue;
        }
        for (col, bit) in line.iter().enumerate() {
            // Glyph rows are byte strings: `'1'` (0x31) = pixel on,
            // `'0'` (0x30) = pixel off. Comparing to `0` would always
            // be false and would paint a solid 7×9 block; the byte
            // literal `b'0'` is the actual gate.
            if *bit == b'0' {
                continue;
            }
            let x = horizontal_offset + col as i32;
            if x < 0 || x >= ICON_WIDTH as i32 {
                continue;
            }
            write_pixel(buffer, x as u32, y as u32, color);
        }
    }
}

const GLYPH_WIDTH: usize = 7;
const GLYPH_HEIGHT: usize = 9;

/// 7×9 bitmap glyph for the letter "D".
const GLYPH_D: [&[u8; GLYPH_WIDTH]; GLYPH_HEIGHT] = [
    b"1111110", b"1111110", b"1100110", b"1100110", b"1100110", b"1100110", b"1100110", b"1111110",
    b"1111110",
];

/// 7×9 bitmap glyph for the letter "U".
const GLYPH_U: [&[u8; GLYPH_WIDTH]; GLYPH_HEIGHT] = [
    b"1100110", b"1100110", b"1100110", b"1100110", b"1100110", b"1100110", b"1100110", b"1111110",
    b"0111100",
];

fn paint_bar(buffer: &mut [u8], top: u32, bottom: u32, rate_bps: u64, color: [u8; 4]) {
    let saturation = rate_bps.min(SATURATION_BPS);
    let label_left = 2_u32;
    let label_right = label_left + GLYPH_WIDTH as u32 + 1;
    let bar_right = ICON_WIDTH - 2;
    if bar_right <= label_right {
        return;
    }
    let usable_width = u64::from(bar_right - label_right);
    let saturation_u64 = u64::from(SATURATION_BPS as u32);
    let width = (usable_width * saturation + saturation_u64 / 2) / saturation_u64;
    let width_u32 = u32::try_from(width).unwrap_or(u32::MAX);
    let bar_top = top + 4;
    let bar_bottom = bottom - 4;
    for y in bar_top..bar_bottom {
        for x in label_right..=label_right + width_u32 {
            if x >= bar_right {
                break;
            }
            write_pixel(buffer, x, y, color);
        }
    }
}

fn write_pixel(buffer: &mut [u8], x: u32, y: u32, color: [u8; 4]) {
    if x >= ICON_WIDTH || y >= ICON_HEIGHT {
        return;
    }
    let offset = (y as usize * ICON_WIDTH as usize + x as usize) * 4;
    let pixel = &mut buffer[offset..offset + 4];
    pixel.copy_from_slice(&color);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dump a few rendered icons as PPM files (any image viewer can
    /// open them) for visual review. Always runs in `cargo test` so
    /// the developer can pipe the four artefacts through image tools
    /// without remembering an opt-in flag.
    #[test]
    fn dump_render_rate_icons_for_review() {
        let dir = std::env::temp_dir().join("biflow-tray-icons");
        std::fs::create_dir_all(&dir).expect("mkdir icons");
        for (label, download_bps, upload_bps) in [
            ("idle", 0, 0),
            ("small", 24_000, 12_000),
            ("half", 8 * 1024 * 1024, 4 * 1024 * 1024),
            ("saturated", SATURATION_BPS * 2, SATURATION_BPS * 2),
        ] {
            let bytes = render_rate_icon(download_bps, upload_bps);
            let path = dir.join(format!("tray-{label}.ppm"));
            write_ppm(&path, &bytes);
            eprintln!("wrote {}", path.display());
        }
    }

    /// PPM (Portable Pixmap, P6) writer — header + raw RGB bytes.
    /// Trivially readable by image tools, so no extra crate dependency.
    fn write_ppm(path: &std::path::Path, rgba: &[u8]) {
        use std::fmt::Write as _;
        let mut header = String::new();
        let _ = writeln!(header, "P6");
        let _ = writeln!(header, "{ICON_WIDTH} {ICON_HEIGHT}");
        let _ = writeln!(header, "255");
        let mut bytes = header.into_bytes();
        bytes.reserve(rgba.len() / 4 * 3);
        for pixel in rgba.chunks_exact(4) {
            bytes.push(pixel[0]);
            bytes.push(pixel[1]);
            bytes.push(pixel[2]);
        }
        std::fs::write(path, &bytes).expect("write ppm");
    }

    #[test]
    fn render_rate_icon_returns_a_32x32_buffer() {
        let bytes = render_rate_icon(1024 * 1024, 512 * 1024);
        assert_eq!(bytes.len(), (ICON_WIDTH * ICON_HEIGHT * 4) as usize);
    }

    #[test]
    fn render_rate_icon_saturates_bars_above_256mbps() {
        let bytes = render_rate_icon(2 * SATURATION_BPS, 2 * SATURATION_BPS);
        let bar_right = ICON_WIDTH as usize - 2;
        let label_right = 2 + GLYPH_WIDTH + 1;
        let last_bar_col = bar_right - 1;
        assert!(
            last_bar_col > label_right,
            "saturated bar pixel must be inside the bar area"
        );
        // The bar fills y = top+4 .. bottom-4 (vertical padding for the
        // glyph + border); pick a row that sits well inside both halves.
        let middle_top_row = (ICON_HEIGHT as usize / 2 / 2) * ICON_WIDTH as usize;
        let middle_bottom_row =
            ((ICON_HEIGHT as usize / 2) + (ICON_HEIGHT as usize / 2 / 2)) * ICON_WIDTH as usize;
        let top = bytes[(middle_top_row + last_bar_col) * 4];
        let bottom = bytes[(middle_bottom_row + last_bar_col) * 4];
        assert_eq!(top, BAR_COLOR_DOWNLOAD[0], "download bar must be saturated");
        assert_eq!(bottom, BAR_COLOR_UPLOAD[0], "upload bar must be saturated");
    }

    #[test]
    fn render_rate_icon_zero_bytes_leaves_the_panel() {
        // Zero traffic must not flash a sliver of colour on the bars.
        let bytes = render_rate_icon(0, 0);
        let label_right = 2 + GLYPH_WIDTH + 1;
        let bar_column = bytes[(ICON_WIDTH as usize + label_right + 5) * 4];
        assert_eq!(
            bar_column, PANEL_COLOR[0],
            "idle download bar must stay panel-coloured"
        );
    }

    #[test]
    fn render_rate_icon_paints_the_d_and_u_labels() {
        let bytes = render_rate_icon(SATURATION_BPS, SATURATION_BPS);
        let label_left = 2_u32;
        // The glyph sits centred vertically: 9 rows inside a 16-row
        // half → vertical_offset = 3, so the painted band lives at
        // rows 3..11 (top) / 19..27 (bottom).
        let top_half_first_row = 3_usize;
        let top_half_hollow_row = 5_usize;
        let bottom_half_first_row = ICON_HEIGHT as usize / 2 + 3;
        let bottom_half_hollow_row = ICON_HEIGHT as usize / 2 + 5;

        // Row 2 of the D bitmap is "1100110" — col 0 is set, col 1 is
        // set. Pinned to vertical_offset = 3 that lands at top row 5.
        // Use the col-1 cell to assert the painted rim survived.
        let d_painted_cell =
            bytes[(top_half_first_row * ICON_WIDTH as usize + label_left as usize + 1) * 4];
        assert_eq!(
            d_painted_cell, LABEL_COLOR_DOWNLOAD[0],
            "D glyph must still paint its outer rim"
        );

        // Same row, col 2 is the hollow centre of the D. A regression
        // that compares the bitmap byte to numeric `0` paints a solid
        // block and this cell turns into LABEL_COLOR instead.
        let d_panel_cell =
            bytes[(top_half_hollow_row * ICON_WIDTH as usize + label_left as usize + 2) * 4];
        assert_eq!(
            d_panel_cell, PANEL_COLOR[0],
            "D glyph must keep its hollow centre; a solid block hides the letter"
        );

        // Same checks for the U glyph in the bottom half.
        let u_painted_cell =
            bytes[(bottom_half_first_row * ICON_WIDTH as usize + label_left as usize + 1) * 4];
        assert_eq!(
            u_painted_cell, LABEL_COLOR_UPLOAD[0],
            "U glyph must still paint its outer rim"
        );
        let u_panel_cell =
            bytes[(bottom_half_hollow_row * ICON_WIDTH as usize + label_left as usize + 2) * 4];
        assert_eq!(
            u_panel_cell, PANEL_COLOR[0],
            "U glyph must keep its hollow centre; a solid block hides the letter"
        );
    }
}
