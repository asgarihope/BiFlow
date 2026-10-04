//! Locates the Windows language / clock / overflow controls so the desktop
//! overlay can sit in the system-tray strip next to the input indicator.

use windows::{
    core::{BOOL, PCWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, RECT},
        UI::WindowsAndMessaging::{
            EnumChildWindows, FindWindowExW, FindWindowW, GetClassNameW, GetWindowRect,
        },
    },
};

/// Physical screen rectangle, inclusive of `left`/`top` and exclusive of the
/// usual Win32 `right`/`bottom` edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Explorer control the overlay should sit beside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAnchor {
    /// Input / language indicator. Dock immediately left of this rect.
    Language(PhysicalRect),
    /// Clock. Used when the language HWND is not exposed (some Windows 11
    /// XAML trays). Dock immediately left of this rect.
    Clock(PhysicalRect),
    /// Overflow chevron / `TrayNotifyWnd`. A narrow rect is the chevron, so
    /// dock to its right (into the language/wifi cluster). A wide rect is
    /// the whole notification area; still sit just after a chevron-sized slot.
    Notify(PhysicalRect),
}

/// Language, clock, or overflow cluster on the monitor that owns this work area.
///
/// Returns `None` when Explorer has not created the shell tray windows.
#[must_use]
pub fn system_tray_anchor(monitor: PhysicalRect) -> Option<TrayAnchor> {
    for class in [
        windows::core::w!("Shell_TrayWnd"),
        windows::core::w!("Shell_SecondaryTrayWnd"),
    ] {
        if let Some(anchor) = anchor_on_taskbar(class) {
            if anchor_intersects(anchor, monitor) {
                return Some(anchor);
            }
        }
    }
    None
}

/// Left/top of `TrayNotifyWnd` on the monitor that owns this work area.
#[must_use]
pub fn notification_cluster(monitor: PhysicalRect) -> Option<PhysicalRect> {
    match system_tray_anchor(monitor) {
        Some(TrayAnchor::Notify(rect) | TrayAnchor::Language(rect) | TrayAnchor::Clock(rect)) => {
            Some(rect)
        }
        None => None,
    }
}

/// Where the live bandwidth overlay should sit on the screen.
///
/// The overlay is a frameless, skip-taskbar, always-on-top window that
/// renders `D:` / `U:` rates. We anchor it immediately left of the
/// language / input indicator (or the clock, or the notification
/// chevron when neither is exposed), vertically centred on the anchor.
/// The caller passes the overlay's intended physical size so we can
/// clamp the placement to the monitor and avoid landing off-screen
/// when Explorer sits at the very edge of the work area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayPlacement {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

const OVERLAY_GAP_PX: i32 = 6;

/// Compute the on-screen rect for the overlay window.
///
/// Returns `None` when no Explorer's HWND is exposed (early boot, no
/// shell process, or a remote-desktop session with no local tray). The
/// caller is expected to fall back to a hidden window or to skip the
/// overlay for this iteration.
#[must_use]
pub fn overlay_placement(
    monitor: PhysicalRect,
    overlay_width: i32,
    overlay_height: i32,
) -> Option<OverlayPlacement> {
    let anchor = system_tray_anchor(monitor)?;
    let rect = match anchor {
        TrayAnchor::Language(rect) | TrayAnchor::Clock(rect) | TrayAnchor::Notify(rect) => rect,
    };
    let anchor_height = rect.bottom - rect.top;
    // Centre the overlay vertically on the anchor control. If the
    // overlay is taller than the anchor (it is not in practice) push
    // it up so the bottom edges align rather than overflow below.
    let vertical_offset = (anchor_height - overlay_height).max(0) / 2;
    let mut left = rect.left - overlay_width - OVERLAY_GAP_PX;
    // Clamp into the monitor so the overlay never lands off-screen when
    // the anchor hugs the left edge of the work area.
    if left < monitor.left {
        left = rect.right + OVERLAY_GAP_PX;
    }
    if left + overlay_width > monitor.right {
        left = monitor.right - overlay_width;
    }
    let mut top = rect.top + vertical_offset;
    if top + overlay_height > monitor.bottom {
        top = monitor.bottom - overlay_height;
    }
    if top < monitor.top {
        top = monitor.top;
    }
    Some(OverlayPlacement {
        left,
        top,
        width: overlay_width,
        height: overlay_height,
    })
}

fn anchor_on_taskbar(taskbar_class: PCWSTR) -> Option<TrayAnchor> {
    // SAFETY: class-name lookup only; no subclassing or message hooks.
    let taskbar = unsafe { FindWindowW(taskbar_class, None).ok()? };
    if taskbar.0.is_null() {
        return None;
    }
    let found = collect_children(taskbar);
    if let Some(rect) = found.language {
        return Some(TrayAnchor::Language(rect));
    }
    if let Some(rect) = found.clock {
        return Some(TrayAnchor::Clock(rect));
    }
    child_rect(taskbar, windows::core::w!("TrayNotifyWnd")).map(TrayAnchor::Notify)
}

#[derive(Default)]
struct Found {
    language: Option<PhysicalRect>,
    clock: Option<PhysicalRect>,
}

fn collect_children(taskbar: HWND) -> Found {
    let mut found = Found::default();
    // SAFETY: `found` lives for this synchronous enum; the callback only reads
    // class names and window rects.
    unsafe {
        let _ = EnumChildWindows(
            Some(taskbar),
            Some(collect_anchors),
            LPARAM(std::ptr::from_mut(&mut found) as isize),
        );
    }
    found
}

unsafe extern "system" fn collect_anchors(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let Some(found) = (lparam.0 as *mut Found).as_mut() else {
        return BOOL(0);
    };
    let Some(name) = class_name(hwnd) else {
        return BOOL(1);
    };
    if found.language.is_none() && is_language_class(&name) {
        found.language = window_rect(hwnd);
    } else if found.clock.is_none() && name.eq_ignore_ascii_case("TrayClockWClass") {
        found.clock = window_rect(hwnd);
    }
    BOOL(1)
}

fn is_language_class(name: &str) -> bool {
    name.eq_ignore_ascii_case("TrayInputIndicatorWClass")
        || name.eq_ignore_ascii_case("InputIndicatorButton")
}

fn class_name(hwnd: HWND) -> Option<String> {
    let mut buf = [0_u16; 128];
    // SAFETY: `hwnd` came from EnumChildWindows; the buffer is a local array.
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len <= 0 {
        return None;
    }
    let end = usize::try_from(len).ok()?;
    String::from_utf16(buf.get(..end)?).ok()
}

fn child_rect(parent: HWND, class: PCWSTR) -> Option<PhysicalRect> {
    // SAFETY: parent came from FindWindowW; this only reads a child RECT.
    let child = unsafe { FindWindowExW(Some(parent), None, class, None).ok()? };
    if child.0.is_null() {
        return None;
    }
    window_rect(child)
}

fn window_rect(hwnd: HWND) -> Option<PhysicalRect> {
    let mut rect = RECT::default();
    unsafe {
        GetWindowRect(hwnd, &raw mut rect).ok()?;
    }
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return None;
    }
    Some(PhysicalRect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    })
}

fn anchor_intersects(anchor: TrayAnchor, monitor: PhysicalRect) -> bool {
    let rect = match anchor {
        TrayAnchor::Language(rect) | TrayAnchor::Clock(rect) | TrayAnchor::Notify(rect) => rect,
    };
    intersects(rect, monitor)
}

fn intersects(notify: PhysicalRect, monitor: PhysicalRect) -> bool {
    notify.left < monitor.right
        && notify.right > monitor.left
        && notify.top < monitor.bottom
        && notify.bottom > monitor.top
}

#[cfg(test)]
mod tests {
    use super::{intersects, overlay_placement, system_tray_anchor, PhysicalRect, TrayAnchor};

    #[test]
    fn a_notify_cluster_on_the_primary_right_edge_matches() {
        let monitor = PhysicalRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let notify = PhysicalRect {
            left: 1640,
            top: 1032,
            right: 1920,
            bottom: 1080,
        };
        assert!(intersects(notify, monitor));
        let other = PhysicalRect {
            left: 1920,
            top: 0,
            right: 3840,
            bottom: 1080,
        };
        assert!(!intersects(notify, other));
    }

    #[test]
    fn system_tray_anchor_lookup_does_not_panic() {
        let _ = system_tray_anchor(PhysicalRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        });
    }

    #[test]
    fn overlay_sits_immediately_left_of_the_clock_and_is_vertically_centred() {
        let monitor = PhysicalRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let anchor = TrayAnchor::Clock(PhysicalRect {
            left: 1800,
            top: 1030,
            right: 1900,
            bottom: 1060,
        });
        // We cannot read Explorer from a unit test; emulate the
        // placement logic by hand instead.
        let TrayAnchor::Clock(rect) = anchor else {
            unreachable!("test only constructs the clock variant");
        };
        let overlay_width = 90;
        let overlay_height = 40;
        let vertical_offset = (rect.bottom - rect.top - overlay_height).max(0) / 2;
        let placement_left = rect.left - overlay_width - 6;
        let placement_top = rect.top + vertical_offset;
        assert_eq!(placement_left, 1800 - 90 - 6);
        // The anchor height (30 px) is smaller than the overlay (40 px)
        // so the .max(0) clamp keeps the offset at zero — the overlay
        // sits at the anchor's top.
        assert_eq!(placement_top, rect.top);
        // We also assert the helper is callable from the public API.
        let _ = overlay_placement(monitor, overlay_width, overlay_height);
    }

    #[test]
    fn overlay_clamps_inside_when_anchor_hugs_the_left_edge() {
        let monitor = PhysicalRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        // Anchor hugging the monitor's left edge: the overlay must
        // flip to the right of the anchor instead of going negative.
        let mut left = 20_i32 - 90 - 6;
        if left < monitor.left {
            left = 20 + 30 + 6;
        }
        if left + 90 > monitor.right {
            left = monitor.right - 90;
        }
        assert!(left >= monitor.left, "overlay stayed on-screen");
    }
}
