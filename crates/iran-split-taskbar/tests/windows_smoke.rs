#![cfg(windows)]

use iran_split_taskbar::{Display, Status};
use std::time::{Duration, Instant};
use windows::{
    core::w,
    Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::{
            BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
            GetPixel, ReleaseDC, SelectObject, CAPTUREBLT, SRCCOPY,
        },
        UI::WindowsAndMessaging::{
            FindWindowExW, FindWindowW, GetWindowRect, GetWindowThreadProcessId,
        },
    },
};

fn owned_child() -> Option<HWND> {
    // SAFETY: read-only discovery; test never writes to a cross-thread HWND.
    unsafe {
        let parent = FindWindowW(w!("Shell_TrayWnd"), None).ok()?;
        let mut after = None;
        while let Ok(child) = FindWindowExW(Some(parent), after, w!("BiFlowTaskbarSpeed"), None) {
            let mut pid = 0;
            GetWindowThreadProcessId(child, Some(&raw mut pid));
            if pid == std::process::id() {
                return Some(child);
            }
            after = Some(child);
        }
        None
    }
}

fn child_present() -> bool {
    owned_child().is_some()
}

fn visible_ink_pixels(hwnd: HWND) -> usize {
    // Read the composed desktop, not the child DC: a valid visible HWND with
    // successful GDI calls can still sit behind Explorer's XAML surface.
    // SAFETY: bounded read of only our verified taskbar rectangle; DC released.
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &raw mut rect).expect("child bounds");
        let dc = GetDC(None);
        assert!(!dc.is_invalid());
        let memory = CreateCompatibleDC(Some(dc));
        assert!(!memory.is_invalid());
        let bitmap = CreateCompatibleBitmap(dc, rect.right - rect.left, rect.bottom - rect.top);
        assert!(!bitmap.is_invalid());
        let previous = SelectObject(memory, bitmap.into());
        assert!(!previous.is_invalid());
        let copied = BitBlt(
            memory,
            0,
            0,
            rect.right - rect.left,
            rect.bottom - rect.top,
            Some(dc),
            rect.left,
            rect.top,
            SRCCOPY | CAPTUREBLT,
        );
        let background = GetPixel(memory, 2, 2).0;
        let mut ink = 0;
        for y in 0..(rect.bottom - rect.top) {
            for x in 0..(rect.right - rect.left) {
                let pixel = GetPixel(memory, x, y).0;
                if pixel != u32::MAX
                    && [0, 8, 16].into_iter().any(|shift| {
                        ((pixel >> shift) & 255).abs_diff((background >> shift) & 255) >= 96
                    })
                {
                    ink += 1;
                }
            }
        }
        assert!(!SelectObject(memory, previous).is_invalid());
        assert!(DeleteObject(bitmap.into()).as_bool());
        assert!(DeleteDC(memory).as_bool());
        assert_eq!(ReleaseDC(None, dc), 1);
        copied.expect("composed desktop copy");
        ink
    }
}

fn wait_until(display: &Display, predicate: impl Fn(Status) -> bool) -> Status {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let status = display.status();
        if predicate(status) {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "native worker did not settle: {status:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "creates a native taskbar child; run explicitly on an interactive Windows 11 desktop"]
fn native_enable_update_disable_reenable_and_shutdown() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    let display = Display::start(|| {}).expect("native thread");
    display.set_enabled(true);
    let status = wait_until(&display, |status| {
        status == Status::Attached || status == Status::Failed
    });
    assert_ne!(status, Status::Failed);
    println!("Windows taskbar embedding status: {status:?}");
    assert!(child_present());
    display.update(Some(350.0 * 1024.0), Some(2.4 * 1024.0 * 1024.0));
    std::thread::sleep(Duration::from_millis(700));
    let deadline = Instant::now() + Duration::from_secs(5);
    while visible_ink_pixels(owned_child().expect("owned child")) < 20 && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(
        visible_ink_pixels(owned_child().expect("owned child")) >= 20,
        "HWND attached but speed glyphs are absent from the composed desktop"
    );
    for _ in 0..24 {
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(
            display.status(),
            Status::Attached,
            "periodic discovery lost attachment"
        );
        assert!(
            visible_ink_pixels(owned_child().expect("owned child")) >= 20,
            "periodic Explorer repaint obscured speed glyphs"
        );
    }
    if status == Status::Attached {
        assert_eq!(
            wait_until(&display, |state| state == Status::Attached),
            Status::Attached
        );
    }
    display.set_enabled(false);
    assert_eq!(
        wait_until(&display, |status| status == Status::Disabled),
        Status::Disabled
    );
    assert!(!child_present(), "disable left an owned child behind");
    display.set_enabled(true);
    assert_eq!(
        wait_until(&display, |status| status == Status::Attached
            || status == Status::Failed),
        Status::Attached
    );
    display.set_enabled(false);
    wait_until(&display, |status| status == Status::Disabled);
    let shutdown = Instant::now();
    drop(display);
    assert!(!child_present(), "shutdown left an owned child behind");
    assert!(
        shutdown.elapsed() < Duration::from_secs(5),
        "worker shutdown blocked"
    );
}
