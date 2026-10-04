//! Renders the live download/upload rate into a 32×32 RGBA buffer that the
//! desktop host feeds back to Tauri as the tray icon image. We deliberately
//! avoid GDI text: drawing proportional bars on a transparent canvas keeps the
//! icon readable on every Windows release without bundling a font file.
//!
//! The visual encodes two stacked horizontal bars on a rounded blue panel:
//! - top half: download (cyan), fills right as the rate climbs;
//! - bottom half: upload (orange), fills right as the rate climbs.
//!
//! Bytes are laid out top-down in RGBA order to match what
//! `tauri::image::Image::new` expects.

/// Final tray icon dimensions and RGBA stride.
pub const ICON_WIDTH: u32 = 32;
pub const ICON_HEIGHT: u32 = 32;
const STRIDE_BYTES: usize = (ICON_WIDTH as usize) * 4;

/// How much of the icon width the rate bar reaches at the saturation point
/// (≈256 MB/s). Below this rate the bar grows linearly; above it stays full.
const SATURATION_BPS: u64 = 256 * 1024 * 1024;

/// Returns a 32×32 RGBA buffer with two stacked horizontal bars that show
/// the live download and upload speed. Bytes are top-down in RGBA order.
#[must_use]
pub fn render_rate_icon(download_bps: u64, upload_bps: u64) -> Vec<u8> {
    let mut buffer = vec![0u8; STRIDE_BYTES * ICON_HEIGHT as usize];
    paint_background(&mut buffer);
    paint_bar(&mut buffer, 0, ICON_HEIGHT / 2, download_bps, DOWNLOAD_COLOR);
    paint_bar(
        &mut buffer,
        ICON_HEIGHT / 2,
        ICON_HEIGHT,
        upload_bps,
        UPLOAD_COLOR,
    );
    paint_border(&mut buffer);
    buffer
}

const DOWNLOAD_COLOR: [u8; 4] = [80, 220, 255, 255];
const UPLOAD_COLOR: [u8; 4] = [255, 168, 64, 255];
const PANEL_COLOR: [u8; 4] = [18, 26, 46, 235];

fn paint_background(buffer: &mut [u8]) {
    for chunk in buffer.chunks_exact_mut(4) {
        chunk.copy_from_slice(&PANEL_COLOR);
    }
}

fn paint_border(buffer: &mut [u8]) {
    // 1px dark border so the panel stays distinct on light/dark taskbars.
    let border = [6u8, 10, 20, 255];
    for x in 0..ICON_WIDTH {
        write_pixel(buffer, x, 0, border);
        write_pixel(buffer, x, ICON_HEIGHT - 1, border);
    }
    for y in 0..ICON_HEIGHT {
        write_pixel(buffer, 0, y, border);
        write_pixel(buffer, ICON_WIDTH - 1, y, border);
    }
}

fn paint_bar(buffer: &mut [u8], top: u32, bottom: u32, rate_bps: u64, color: [u8; 4]) {
    let saturation = rate_bps.min(SATURATION_BPS);
    let usable_width = u64::from(ICON_WIDTH - 2);
    let saturation_u64 = u64::from(SATURATION_BPS as u32);
    let width = (usable_width * saturation + saturation_u64 / 2) / saturation_u64;
    let width_u32 = u32::try_from(width).unwrap_or(u32::MAX);
    for y in top..bottom {
        for x in 1..=width_u32 {
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

    #[test]
    fn render_rate_icon_returns_a_32x32_buffer() {
        let bytes = render_rate_icon(1024 * 1024, 512 * 1024);
        assert_eq!(bytes.len(), (ICON_WIDTH * ICON_HEIGHT * 4) as usize);
    }

    #[test]
    fn render_rate_icon_saturates_bars_above_256mbps() {
        // Above SATURATION_BPS the bars must be drawn fully across the icon
        // (minus the 1px border) so the operator never sees a tiny sliver
        // when the link is saturated.
        let bytes = render_rate_icon(2 * SATURATION_BPS, 2 * SATURATION_BPS);
        let x = ICON_WIDTH as usize - 2; // last bar pixel column
        let top = bytes[(1 * ICON_WIDTH as usize + x) * 4];
        let bottom = bytes[((ICON_HEIGHT as usize / 2 + 1) * ICON_WIDTH as usize + x) * 4];
        assert_eq!(top, DOWNLOAD_COLOR[0], "download bar must be saturated");
        assert_eq!(bottom, UPLOAD_COLOR[0], "upload bar must be saturated");
    }

    #[test]
    fn render_rate_icon_zero_bytes_leaves_the_panel() {
        // Zero traffic must not flash a sliver of colour on the bars: the
        // RGBA byte at the top-left of each bar should still be the panel
        // background so the icon starts clean.
        let bytes = render_rate_icon(0, 0);
        let top_bar_x = (ICON_WIDTH as usize / 2) * 4;
        let top = bytes[(1 * ICON_WIDTH as usize) * 4 + top_bar_x];
        assert_eq!(
            top, PANEL_COLOR[0],
            "idle download bar must stay panel-coloured"
        );
    }
}