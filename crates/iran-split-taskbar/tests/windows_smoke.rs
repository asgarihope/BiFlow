#![cfg(windows)]

use iran_split_taskbar::{Display, Status};
use std::time::{Duration, Instant};
use windows::{
    core::w,
    Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW},
};

fn child_present() -> bool {
    // SAFETY: read-only discovery; test never writes to a cross-thread HWND.
    unsafe {
        FindWindowW(w!("Shell_TrayWnd"), None).is_ok_and(|parent| {
            FindWindowExW(Some(parent), None, w!("BiFlowTaskbarSpeed"), None).is_ok()
        })
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
    for _ in 0..24 {
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(
            display.status(),
            Status::Attached,
            "periodic discovery lost attachment"
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
