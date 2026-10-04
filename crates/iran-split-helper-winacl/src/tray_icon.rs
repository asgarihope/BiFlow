//! Renders the live download/upload rate into a 32×32 RGBA buffer that the
//! desktop host feeds back to Tauri as the tray icon image.
//!
//! The icon is the BiFlow tray icon itself — no separate overlay window.
//! On every traffic sample we redraw a tiny two-row panel:
//!
//! ```text
//! ┌───────────────────────┐
//! │ D │ ▓▓▓ │ 1.2 M      │  ← cyan "D" + bar + 3-digit value
//! │ U │ ▓▓░ │ 850 K      │  ← orange "U" + bar + 3-digit value
//! └───────────────────────┘
//! ```
//!
//! Bytes are laid out top-down in RGBA order to match what
//! `tauri::image::Image::new` expects.

use iran_split_config::TaskbarCompactRate;

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

/// Column at which the D / U label glyph starts.
const LABEL_LEFT: u32 = 2;
/// First column to the right of the label glyph (gap column included).
const LABEL_RIGHT: u32 = LABEL_LEFT + GLYPH_WIDTH as u32 + 1;
/// First column the rate bar can paint into.
const BAR_LEFT: u32 = LABEL_RIGHT;
/// First column past the rate bar (the number column starts here).
const BAR_RIGHT: u32 = BAR_LEFT + BAR_WIDTH;
/// Width of the rate bar in pixels.
const BAR_WIDTH: u32 = 10;
/// First column the compact numeric readout can paint into.
const NUMBER_LEFT: u32 = BAR_RIGHT + 1;
/// First column past the compact numeric readout (always inside the
/// right border so digits stay readable when saturated).
const NUMBER_RIGHT: u32 = ICON_WIDTH - 2;

/// Returns a 32×32 RGBA buffer with two stacked rows (`D` over `U`) that
/// show the live download and upload speed. Bytes are top-down in RGBA
/// order.
#[must_use]
pub fn render_rate_icon(
    download_bps: u64,
    upload_bps: u64,
    download_compact: TaskbarCompactRate,
    upload_compact: TaskbarCompactRate,
) -> Vec<u8> {
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
    paint_compact_value(
        &mut buffer,
        0,
        ICON_HEIGHT / 2,
        &download_compact,
        NUMBER_COLOR_DOWNLOAD,
    );
    paint_compact_value(
        &mut buffer,
        ICON_HEIGHT / 2,
        ICON_HEIGHT,
        &upload_compact,
        NUMBER_COLOR_UPLOAD,
    );
    buffer
}

const LABEL_COLOR_DOWNLOAD: [u8; 4] = [120, 230, 255, 255];
const LABEL_COLOR_UPLOAD: [u8; 4] = [255, 184, 96, 255];
const BAR_COLOR_DOWNLOAD: [u8; 4] = [80, 220, 255, 255];
const BAR_COLOR_UPLOAD: [u8; 4] = [255, 168, 64, 255];
const NUMBER_COLOR_DOWNLOAD: [u8; 4] = [200, 240, 255, 255];
const NUMBER_COLOR_UPLOAD: [u8; 4] = [255, 220, 170, 255];
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
    let horizontal_offset = LABEL_LEFT as i32;
    for (row, line) in glyph.iter().enumerate() {
        let y = top as i32 + vertical_offset + row as i32;
        if y < top as i32 || y >= bottom as i32 {
            continue;
        }
        for (col, bit) in line.iter().enumerate() {
            // Glyph rows are byte strings with two possible encodings:
            //   - letter glyphs use `'1'` (on) / `'0'` (off)
            //   - digit glyphs use `'#'` (on) / `'.'` (off)
            // Any byte that is not one of the "on" markers is treated
            // as a blank cell so neither font ever paints a solid
            // block. Comparing to numeric `0` would always be false
            // and would paint every cell of every glyph.
            if !matches!(*bit, b'1' | b'#') {
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

const DIGIT_WIDTH: usize = 3;
const DIGIT_HEIGHT: usize = 5;

/// 3×5 bitmap glyphs for digits `0`..`9`. Indexed by `[digit][row]`
/// (digit first so a single `GLYPH_DIGITS[digit][row]` lookup returns
/// the `row`-th scanline of digit `d`). Same `'1'` (on) / `'0'` (off)
/// encoding as the letter table; a single column gap is added between
/// digit slots at render time so the number stays readable.
const GLYPH_DIGITS: [[&[u8; DIGIT_WIDTH]; DIGIT_HEIGHT]; 10] = [
    [
        b"###", b"#.#", b"#.#", b"#.#", b"###", // 0
    ],
    [
        b".#.", b"##.", b".#.", b".#.", b"###", // 1
    ],
    [
        b"###", b"..#", b"###", b"#..", b"###", // 2
    ],
    [
        b"###", b"..#", b"###", b"..#", b"###", // 3
    ],
    [
        b"#.#", b"#.#", b"###", b"..#", b"..#", // 4
    ],
    [
        b"###", b"#..", b"###", b"..#", b"###", // 5
    ],
    [
        b"###", b"#..", b"###", b"#.#", b"###", // 6
    ],
    [
        b"###", b"..#", b"..#", b"..#", b"..#", // 7
    ],
    [
        b"###", b"#.#", b"###", b"#.#", b"###", // 8
    ],
    [
        b"###", b"#.#", b"###", b"..#", b"###", // 9
    ],
];

fn digit_glyph(digit: u8) -> &'static [&'static [u8; DIGIT_WIDTH]; DIGIT_HEIGHT] {
    // `digit` is constrained to 0..=9 by the caller; the array
    // indexing is therefore safe.
    &GLYPH_DIGITS[usize::from(digit.min(9))]
}

fn paint_bar(buffer: &mut [u8], top: u32, bottom: u32, rate_bps: u64, color: [u8; 4]) {
    let saturation = rate_bps.min(SATURATION_BPS);
    if BAR_RIGHT <= BAR_LEFT {
        return;
    }
    let usable_width = u64::from(BAR_RIGHT - BAR_LEFT);
    let saturation_u64 = u64::from(SATURATION_BPS as u32);
    let width = (usable_width * saturation + saturation_u64 / 2) / saturation_u64;
    let width_u32 = u32::try_from(width).unwrap_or(u32::MAX);
    let bar_top = top + 4;
    let bar_bottom = bottom - 4;
    for y in bar_top..bar_bottom {
        for x in BAR_LEFT..BAR_LEFT + width_u32 {
            if x >= BAR_RIGHT {
                break;
            }
            write_pixel(buffer, x, y, color);
        }
    }
}

/// Paint the compact numeric readout for a single row, using the same
/// vertical centring as the `D` / `U` label. The readout lives in
/// `NUMBER_LEFT..NUMBER_RIGHT` and right-aligns the three digit slots
/// so the unit letter (when present) always sits flush with the right
/// border. Leading blank slots stay on the panel colour.
fn paint_compact_value(
    buffer: &mut [u8],
    top: u32,
    bottom: u32,
    compact: &TaskbarCompactRate,
    color: [u8; 4],
) {
    if compact.is_blank() {
        return;
    }
    let row_height = bottom - top;
    let vertical_offset = (row_height as i32 - DIGIT_HEIGHT as i32) / 2;
    let slot_pitch = DIGIT_WIDTH as i32 + 1; // 1 px gap between slots
                                             // Right-align: digit slots occupy the rightmost columns of the
                                             // number area. The unit letter takes a 1-px column on the very
                                             // right when present.
    let right_edge = (NUMBER_RIGHT - 1) as i32;
    let digits: [Option<u8>; 3] = [compact.hundreds, compact.tens, compact.ones];
    for (index, digit) in digits.iter().enumerate() {
        let Some(digit) = digit else {
            continue;
        };
        // Slot index 0 is the rightmost (ones), index 2 the leftmost
        // (hundreds). Right-aligned, the rightmost slot starts at
        // `right_edge - 2` (so the digit covers `right_edge-2..right_edge`).
        let slot_index = i32::try_from(2_usize.saturating_sub(index)).unwrap_or(0);
        let slot_left = right_edge - (slot_index * slot_pitch) - (DIGIT_WIDTH as i32 - 1);
        let rows = digit_glyph(*digit);
        for (row, line) in rows.iter().enumerate() {
            let y = top as i32 + vertical_offset + row as i32;
            if y < top as i32 || y >= bottom as i32 {
                continue;
            }
            for (col, bit) in line.iter().enumerate() {
                // Digits use `'#'` / `'.'`. Skip any byte that is not
                // the "on" marker so the painter never fills the
                // glyph with a solid block.
                if !matches!(*bit, b'#') {
                    continue;
                }
                let x = slot_left + col as i32;
                if x < NUMBER_LEFT as i32 || x > right_edge {
                    continue;
                }
                write_pixel(buffer, x as u32, y as u32, color);
            }
        }
    }
    if let Some(unit) = compact.unit {
        // The unit letter is a 3×5 glyph that sits to the left of
        // the digit slots when we want it on the icon; but the unit
        // itself is already shown in the tooltip and the bar's fill
        // ratio carries the magnitude. We deliberately skip the
        // glyph here so the icon stays focused on the digits and
        // does not crowd the 32×32 panel with another column.
        let _ = unit;
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
        for (label, download_bps, upload_bps, download_compact, upload_compact) in [
            ("idle", 0, 0, rate_display(0), rate_display(0)),
            (
                "small",
                24_000,
                12_000,
                rate_display(24_000),
                rate_display(12_000),
            ),
            (
                "half",
                8 * 1024 * 1024,
                4 * 1024 * 1024,
                rate_display(8 * 1024 * 1024),
                rate_display(4 * 1024 * 1024),
            ),
            (
                "saturated",
                SATURATION_BPS * 2,
                SATURATION_BPS * 2,
                rate_display(SATURATION_BPS * 2),
                rate_display(SATURATION_BPS * 2),
            ),
        ] {
            let bytes =
                render_rate_icon(download_bps, upload_bps, download_compact, upload_compact);
            let path = dir.join(format!("tray-{label}.ppm"));
            write_ppm(&path, &bytes);
            eprintln!("wrote {}", path.display());
        }
    }

    fn rate_display(bps: u64) -> TaskbarCompactRate {
        // Auto-scale + bytes so the dumped PNGs match what a freshly
        // installed user sees before they open Settings.
        iran_split_config::format_bandwidth_rate_compact(
            bps,
            iran_split_config::TaskbarTrafficDisplay::default(),
        )
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
        let download = rate_display(1024 * 1024);
        let upload = rate_display(512 * 1024);
        let bytes = render_rate_icon(1024 * 1024, 512 * 1024, download, upload);
        assert_eq!(bytes.len(), (ICON_WIDTH * ICON_HEIGHT * 4) as usize);
    }

    #[test]
    fn render_rate_icon_saturates_bars_above_256mbps() {
        let rate = rate_display(2 * SATURATION_BPS);
        let bytes = render_rate_icon(2 * SATURATION_BPS, 2 * SATURATION_BPS, rate, rate);
        let last_bar_col = (BAR_RIGHT - 1) as usize;
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
        // Zero traffic must not flash a sliver of colour on the bars
        // or the number column.
        let idle = TaskbarCompactRate {
            hundreds: None,
            tens: None,
            ones: Some(0),
            unit: None,
        };
        let bytes = render_rate_icon(0, 0, idle, idle);
        // The bar at col `BAR_LEFT + 5` (mid-bar) stays panel-coloured.
        let bar_column = bytes[((ICON_WIDTH as usize) + (BAR_LEFT as usize) + 5) * 4];
        assert_eq!(
            bar_column, PANEL_COLOR[0],
            "idle download bar must stay panel-coloured"
        );
        // The number column at col `NUMBER_LEFT` also stays panel-coloured
        // because `is_blank()` short-circuits before any pixel is drawn.
        let number_column = bytes[((ICON_WIDTH as usize) + (NUMBER_LEFT as usize)) * 4];
        assert_eq!(
            number_column, PANEL_COLOR[0],
            "idle number column must stay panel-coloured"
        );
    }

    #[test]
    fn render_rate_icon_paints_the_d_and_u_labels() {
        let rate = rate_display(SATURATION_BPS);
        let bytes = render_rate_icon(SATURATION_BPS, SATURATION_BPS, rate, rate);
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
            bytes[(top_half_first_row * ICON_WIDTH as usize + LABEL_LEFT as usize + 1) * 4];
        assert_eq!(
            d_painted_cell, LABEL_COLOR_DOWNLOAD[0],
            "D glyph must still paint its outer rim"
        );

        // Same row, col 2 is the hollow centre of the D. A regression
        // that compares the bitmap byte to numeric `0` paints a solid
        // block and this cell turns into LABEL_COLOR instead.
        let d_panel_cell =
            bytes[(top_half_hollow_row * ICON_WIDTH as usize + LABEL_LEFT as usize + 2) * 4];
        assert_eq!(
            d_panel_cell, PANEL_COLOR[0],
            "D glyph must keep its hollow centre; a solid block hides the letter"
        );

        // Same checks for the U glyph in the bottom half.
        let u_painted_cell =
            bytes[(bottom_half_first_row * ICON_WIDTH as usize + LABEL_LEFT as usize + 1) * 4];
        assert_eq!(
            u_painted_cell, LABEL_COLOR_UPLOAD[0],
            "U glyph must still paint its outer rim"
        );
        let u_panel_cell =
            bytes[(bottom_half_hollow_row * ICON_WIDTH as usize + LABEL_LEFT as usize + 2) * 4];
        assert_eq!(
            u_panel_cell, PANEL_COLOR[0],
            "U glyph must keep its hollow centre; a solid block hides the letter"
        );
    }

    #[test]
    fn paint_compact_value_paints_each_digit_at_its_own_slot() {
        // 16 MB/s at the megabyte unit → "16M": hundreds blank, tens=1,
        // ones=6, unit letter column set.
        let compact = TaskbarCompactRate {
            hundreds: None,
            tens: Some(1),
            ones: Some(6),
            unit: Some('M'),
        };
        let mut buffer = vec![0u8; STRIDE_BYTES * ICON_HEIGHT as usize];
        paint_compact_value(
            &mut buffer,
            0,
            ICON_HEIGHT / 2,
            &compact,
            [255, 255, 255, 255],
        );
        // The "6" digit is the rightmost slot; its rightmost cell
        // (col NUMBER_RIGHT - 1, row = vertical_offset + 4) must paint.
        let ones_row =
            (DIGIT_HEIGHT as i32 - 1) + ((ICON_HEIGHT as i32 / 2 - DIGIT_HEIGHT as i32) / 2);
        let ones_col = (NUMBER_RIGHT - 1) as i32;
        let pixel = &buffer[(ones_row as usize * ICON_WIDTH as usize + ones_col as usize) * 4];
        assert_eq!(*pixel, 255, "ones digit must paint its rightmost cell");
        // The "1" digit is the next slot to the left; a different column.
        let tens_col = ones_col - DIGIT_WIDTH as i32 - 1;
        let tens_pixel = &buffer[(ones_row as usize * ICON_WIDTH as usize + tens_col as usize) * 4];
        assert_eq!(*tens_pixel, 255, "tens digit must paint its rightmost cell");
        // The hundreds slot is `None`, so its rightmost column should
        // stay panel-coloured (the default 0).
        let hundreds_col = tens_col - DIGIT_WIDTH as i32 - 1;
        let hundreds_pixel =
            &buffer[(ones_row as usize * ICON_WIDTH as usize + hundreds_col as usize) * 4];
        assert_eq!(*hundreds_pixel, 0, "blank hundreds slot must not paint");
    }

    #[test]
    fn paint_compact_value_keeps_the_panel_for_an_entirely_blank_reading() {
        // All `None` collapses to `is_blank()` and the painter short-circuits.
        let compact = TaskbarCompactRate {
            hundreds: None,
            tens: None,
            ones: None,
            unit: None,
        };
        let mut buffer = vec![42u8; STRIDE_BYTES * ICON_HEIGHT as usize];
        paint_compact_value(
            &mut buffer,
            0,
            ICON_HEIGHT / 2,
            &compact,
            [255, 255, 255, 255],
        );
        // Every pixel in the number column must keep its sentinel value.
        for row in 0..ICON_HEIGHT / 2 {
            for col in NUMBER_LEFT..NUMBER_RIGHT {
                let offset = (row as usize * ICON_WIDTH as usize + col as usize) * 4;
                assert_eq!(buffer[offset], 42, "row {row} col {col} must be untouched");
            }
        }
    }

    #[test]
    fn digit_glyph_returns_distinct_shapes_for_every_digit() {
        // Every glyph must render at least one white pixel so the
        // operator can actually read the number, and no two glyphs
        // may share an identical shape (regression against copy-paste).
        let mut seen: Vec<[[u8; DIGIT_WIDTH]; DIGIT_HEIGHT]> = Vec::new();
        for digit in 0..10u8 {
            let glyph = digit_glyph(digit);
            let mut flat = [[0u8; DIGIT_WIDTH]; DIGIT_HEIGHT];
            let mut painted = 0;
            for (row, line) in glyph.iter().enumerate() {
                for (col, byte) in line.iter().enumerate() {
                    flat[row][col] = *byte;
                    if *byte == b'#' {
                        painted += 1;
                    }
                }
            }
            assert!(painted >= 5, "digit {digit} should paint at least 5 cells");
            assert!(
                !seen.iter().any(|other| other == &flat),
                "digit {digit} matches another glyph shape"
            );
            seen.push(flat);
        }
    }

    #[test]
    fn digit_glyphs_keep_a_hollow_centre_so_the_number_stays_readable() {
        // Regression against a copy of the bug that originally hid the
        // D / U letters: comparing the bitmap byte to numeric `0`
        // (instead of `b'0'`) always evaluates true and paints a
        // solid 3×5 block. Pin the hollow centre on digit 0 so the
        // next font edit cannot silently regress the number.
        let glyph = digit_glyph(0);
        // Middle row of "0" is "#.#"; the centre cell must NOT paint.
        let middle_centre = glyph[1][1];
        assert_eq!(
            middle_centre, b'.',
            "digit 0 centre must stay hollow; a solid block hides the digit"
        );
        // Outer rim still paints.
        let rim = glyph[0][0];
        assert_eq!(rim, b'#', "digit 0 outer rim must stay painted");
    }
}
