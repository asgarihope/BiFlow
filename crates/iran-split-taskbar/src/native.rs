//! HWND/GDI ownership stays on the message pump; UIA runs on a windowless MTA.
use crate::{find_gap, format_rate, rows};
use std::{
    cell::{Cell, RefCell},
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, Mutex, Weak,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows::{
    core::{w, Interface},
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
            GetClipBox, GetSysColor, InvalidateRect, SelectObject, SetBkMode, SetTextColor,
            UpdateWindow, CLIP_DEFAULT_PRECIS, COLOR_BTNFACE, COLOR_BTNTEXT, DEFAULT_CHARSET,
            DEFAULT_PITCH, DEFAULT_QUALITY, DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE,
            DT_VCENTER, HDC, HGDIOBJ, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TRANSPARENT,
        },
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED,
            },
            LibraryLoader::GetModuleHandleW,
            Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD},
            Threading::GetCurrentThreadId,
            Variant::VARIANT,
        },
        UI::{
            Accessibility::{
                AutomationElementMode_None, CUIAutomation8, IUIAutomation, IUIAutomation2,
                IUIAutomationCondition, IUIAutomationElement, IUIAutomationElementArray,
                SetWinEventHook, TreeScope, TreeScope_Children, TreeScope_Descendants,
                TreeScope_Element, UIA_AutomationIdPropertyId, UIA_BoundingRectanglePropertyId,
                UIA_ButtonControlTypeId, UIA_ControlTypePropertyId, UIA_EditControlTypeId,
                UIA_IsOffscreenPropertyId, UIA_ListItemControlTypeId, UIA_MenuItemControlTypeId,
                UIA_NativeWindowHandlePropertyId, UnhookWinEvent, HWINEVENTHOOK,
            },
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowExW,
                FindWindowW, GetAncestor, GetClientRect, GetMessageW, GetParent, GetWindowLongPtrW,
                GetWindowRect, GetWindowThreadProcessId, IsWindow, KillTimer, PostMessageW,
                RegisterClassW, RegisterWindowMessageW, SetLayeredWindowAttributes, SetTimer,
                SetWindowPos, ShowWindow, TranslateMessage, UnregisterClassW, EVENT_OBJECT_CREATE,
                EVENT_OBJECT_HIDE, EVENT_OBJECT_LOCATIONCHANGE, GA_ROOT, GWL_EXSTYLE, HTCLIENT,
                HWND_TOP, LWA_COLORKEY, MSG, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
                WINEVENT_OUTOFCONTEXT, WM_APP, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ERASEBKGND,
                WM_LBUTTONUP, WM_NCHITTEST, WM_PAINT, WM_POWERBROADCAST, WM_SETTINGCHANGE,
                WM_THEMECHANGED, WM_TIMER, WNDCLASSW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE,
                WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
};

/// Address-free lifecycle state for diagnostics and smoke verification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Disabled,
    Attached,
    Unavailable,
    Failed,
}

#[derive(Clone, Copy)]
struct Input {
    status: Status,
    enabled: bool,
    upload: Option<f64>,
    download: Option<f64>,
    sampled: Instant,
}

static DIAGNOSTICS: Mutex<Option<Weak<Mutex<Input>>>> = Mutex::new(None);
static RENDER_SURFACE: AtomicU8 = AtomicU8::new(0);
static THEME_FALLBACK_LOGGED: AtomicBool = AtomicBool::new(false);

#[must_use]
pub fn diagnostic_status() -> Status {
    DIAGNOSTICS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(Weak::upgrade)
        .map_or(Status::Disabled, |input| {
            input
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .status
        })
}

/// Address-free renderer state for environment snapshots.
#[must_use]
pub fn diagnostic_renderer() -> &'static str {
    match RENDER_SURFACE.load(Ordering::Relaxed) {
        1 => "layered_gdi",
        2 => "unlayered_gdi",
        3 => "clipped",
        _ => "awaiting_paint",
    }
}

/// Thread-safe mailbox. No native handle crosses this interface.
pub struct Display {
    input: Arc<Mutex<Input>>,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Display {
    /// Starts a dedicated worker; shell failures are diagnosed and retried.
    ///
    /// # Errors
    /// Returns a thread-spawn error.
    pub fn start(on_click: impl Fn() + Send + Sync + 'static) -> std::io::Result<Self> {
        let input = Arc::new(Mutex::new(Input {
            status: Status::Disabled,
            enabled: false,
            upload: None,
            download: None,
            sampled: Instant::now(),
        }));
        *DIAGNOSTICS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::downgrade(&input));
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_input = input.clone();
        let worker_stopped = stopped.clone();
        let click: Arc<dyn Fn() + Send + Sync> = Arc::new(on_click);
        let thread = std::thread::Builder::new()
            .name("biflow-taskbar".into())
            .spawn(move || {
                // SAFETY: all owned HWND lifetimes and callbacks are confined to this
                // worker. UIA interfaces remain on their separate MTA. Synchronous pointers refer to live stack values; window
                // class names are static UTF-16. No Explorer memory is modified.
                if unsafe { run(&worker_input, &worker_stopped, &click) }.is_err() {
                    worker_input
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .status = Status::Failed;
                    audit("taskbar.worker_failed", "win32_initialization_failed");
                }
            })?;
        Ok(Self {
            input,
            stopped,
            thread: Some(thread),
        })
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .enabled = enabled;
    }

    #[must_use]
    pub fn status(&self) -> Status {
        self.input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .status
    }

    pub fn update(&self, upload: Option<f64>, download: Option<f64>) {
        let mut input = self
            .input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        input.upload = upload;
        input.download = download;
        input.sampled = Instant::now();
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if self
            .thread
            .take()
            .is_some_and(|thread| thread.join().is_err())
        {
            audit("taskbar.worker_failed", "thread_panicked");
        }
    }
}

fn audit(event: &'static str, cause: &'static str) {
    tracing::warn!(
        event,
        section = "taskbar",
        initiator = "native_worker",
        cause,
        trace_route = "taskbar->win32",
        "taskbar display lifecycle"
    );
}

struct PaintState {
    input: Input,
    click: Arc<dyn Fn() + Send + Sync>,
}
thread_local! {
    static PAINT_DIAGNOSED: Cell<bool> = const { Cell::new(false) };
    static PAINT: RefCell<Option<PaintState>> = const { RefCell::new(None) };
    static WINDOWS: Cell<(HWND, HWND, HWND)> = const {
        Cell::new((HWND(std::ptr::null_mut()), HWND(std::ptr::null_mut()), HWND(std::ptr::null_mut())))
    };
}
// Only this background key is transparent; glyphs remain clickable.
const BACKGROUND_KEY: COLORREF = COLORREF(0x0003_0201);
const WM_LAYOUT: u32 = WM_APP + 1;

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match message {
        WM_PAINT => {
            PAINT.with(|state| {
                if let Some(state) = state.borrow().as_ref() {
                    paint(hwnd, state.input);
                }
            });
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_LBUTTONUP => {
            PAINT.with(|state| {
                if let Some(state) = state.borrow().as_ref() {
                    (state.click)();
                }
            });
            LRESULT(0)
        }
        WM_NCHITTEST => LRESULT(isize::try_from(HTCLIENT).unwrap_or(1)),
        _ => DefWindowProcW(hwnd, message, wp, lp),
    }
}

// Out-of-context hooks are dispatched by the registering worker's message pump.
unsafe extern "system" fn layout_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    WINDOWS.with(|state| {
        let (host, parent, display) = state.get();
        if hwnd == display || owns_window(hwnd) {
            return;
        }
        if !parent.is_invalid() && GetAncestor(hwnd, GA_ROOT) == parent {
            tracing::debug!(
                event = "taskbar.layout_invalidated",
                section = "taskbar",
                initiator = "shell_hook",
                cause = "shell_event",
                trace_route = "explorer->taskbar",
                shell_event = event,
                shell_object = object,
                parent_event = hwnd == parent,
                "taskbar layout invalidated"
            );
            if owns_window(display) {
                // ShowWindow's BOOL reports previous visibility, not failure.
                let _was_visible = ShowWindow(display, SW_HIDE);
            }
            if PostMessageW(Some(host), WM_LAYOUT, WPARAM(0), LPARAM(0)).is_err() {
                audit("taskbar.recovery_failed", "layout_message_failed");
            }
        }
    });
}

struct Worker {
    hooks: Vec<HWINEVENTHOOK>,
    display: HWND,
    parent: HWND,
    discovery: Discovery,
    pending: bool,
    epoch: u64,
    last_layout: Instant,
    last_failure: Option<&'static str>,
    retry_attempt: u8,
}

unsafe fn owns_window(hwnd: HWND) -> bool {
    // Explorer can destroy the child during a restart and Windows can recycle
    // its HWND. Never write to a recycled handle owned by another thread.
    !hwnd.is_invalid()
        && IsWindow(Some(hwnd)).as_bool()
        && GetWindowThreadProcessId(hwnd, None) == GetCurrentThreadId()
}

impl Worker {
    unsafe fn destroy(&mut self) {
        RENDER_SURFACE.store(0, Ordering::Relaxed);
        PAINT_DIAGNOSED.with(|state| state.set(false));
        for hook in self.hooks.drain(..) {
            if !UnhookWinEvent(hook).as_bool() {
                audit("taskbar.cleanup_failed", "unhook_failed");
            }
        }
        WINDOWS.with(|state| {
            let (host, _, _) = state.get();
            state.set((host, HWND::default(), HWND::default()));
        });
        if owns_window(self.display) && DestroyWindow(self.display).is_err() {
            audit("taskbar.cleanup_failed", "destroy_window_failed");
        }
        self.display = HWND::default();
        self.parent = HWND::default();
    }

    unsafe fn layout(&mut self, enabled: bool) {
        if !enabled {
            self.destroy();
            self.retry_attempt = 0;
            self.last_failure = None;
            return;
        }
        if !self.pending {
            self.pending = self
                .discovery
                .requests
                .send((self.epoch, self.display.0.addr()))
                .is_ok();
            if !self.pending {
                self.failed("accessibility_worker_unavailable");
            }
        }
    }

    unsafe fn finish_layout(&mut self, enabled: bool) {
        let (epoch, result) = match self.discovery.results.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                if self.pending {
                    self.failed("accessibility_worker_unavailable");
                }
                self.pending = false;
                return;
            }
        };
        self.pending = false;
        if !enabled || epoch != self.epoch {
            // Never apply geometry captured before a shell event or preference change.
            self.last_layout = Instant::now()
                .checked_sub(Duration::from_secs(30))
                .unwrap_or_else(Instant::now);
            return;
        }
        if let Err(cause) = result.and_then(|layout| self.attach(layout)) {
            self.failed(cause);
        } else {
            self.retry_attempt = 0;
            if self.last_failure.take().is_some() {
                tracing::info!(
                    event = "taskbar.recovered",
                    section = "taskbar",
                    initiator = "native_worker",
                    cause = "layout_available",
                    trace_route = "taskbar->win32",
                    "taskbar display recovered"
                );
            }
        }
    }

    unsafe fn failed(&mut self, cause: &'static str) {
        self.destroy();
        self.retry_attempt = self.retry_attempt.saturating_add(1);
        if self.last_failure != Some(cause) {
            audit("taskbar.unavailable", cause);
            self.last_failure = Some(cause);
        }
    }

    unsafe fn attach(&mut self, layout: Layout) -> Result<(), &'static str> {
        let parent = HWND(layout.parent as *mut _);
        let Layout {
            bar,
            dpi,
            x,
            requested,
            height,
            ..
        } = layout;
        let mut current = RECT::default();
        if FindWindowW(w!("Shell_TrayWnd"), None).ok() != Some(parent)
            || GetWindowRect(parent, &raw mut current).is_err()
            || current != bar
            || GetDpiForWindow(parent).max(96) != dpi
        {
            return Err("layout_changed_during_discovery");
        }
        if self.parent != parent
            || !owns_window(self.display)
            || !GetParent(self.display).is_ok_and(|owner| owner == parent)
        {
            self.destroy();
            self.display = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_LAYERED,
                w!("BiFlowTaskbarSpeed"),
                w!("BiFlow network speed"),
                WS_CHILD,
                x,
                0,
                requested,
                height,
                Some(parent),
                None,
                None,
                None,
            )
            .map_err(|_| "embedding_failed")?;
            // Explorer's XAML surface can cover an ordinary GDI child despite
            // IsWindowVisible reporting true. A color-keyed layered child gives
            // DWM a separate composition surface without creating an overlay.
            SetLayeredWindowAttributes(self.display, BACKGROUND_KEY, 0, LWA_COLORKEY)
                .map_err(|_| "layered_composition_failed")?;
            self.parent = parent;
            WINDOWS.with(|state| {
                let (host, _, _) = state.get();
                state.set((host, parent, self.display));
            });
            self.install_hooks()?;
            tracing::info!(
                event = "taskbar.attached",
                section = "taskbar",
                initiator = "native_worker",
                cause = "verified_gap",
                trace_route = "taskbar->win32",
                dpi,
                "native taskbar display attached"
            );
        }
        SetWindowPos(
            self.display,
            Some(HWND_TOP),
            x,
            0,
            requested,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
        .map_err(|_| "position_failed")?;
        Ok(())
    }

    unsafe fn install_hooks(&mut self) -> Result<(), &'static str> {
        let mut pid = 0;
        if GetWindowThreadProcessId(self.parent, Some(&raw mut pid)) == 0 {
            return Err("shell_process_unavailable");
        }
        for (first, last) in [
            (EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE),
            (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
        ] {
            let hook = SetWinEventHook(
                first,
                last,
                None,
                Some(layout_event),
                pid,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            if hook.is_invalid() {
                audit("taskbar.recovery_degraded", "shell_event_hook_failed");
            } else {
                self.hooks.push(hook);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Layout {
    parent: usize,
    bar: RECT,
    dpi: u32,
    x: i32,
    requested: i32,
    height: i32,
}

type LayoutResult = (u64, Result<Layout, &'static str>);
struct Discovery {
    requests: Sender<(u64, usize)>,
    results: Receiver<LayoutResult>,
    thread: Option<JoinHandle<()>>,
}

impl Discovery {
    fn start() -> windows::core::Result<Self> {
        let (requests, incoming) = mpsc::channel();
        let (outgoing, results) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("biflow-taskbar-uia".into())
            .spawn(move || {
                // SAFETY: this MTA owns no windows. COM interfaces are created,
                // used and released here; only copied layout data crosses threads.
                unsafe {
                    if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                        audit("taskbar.accessibility_failed", "com_initialization_failed");
                        while let Ok((epoch, _)) = incoming.recv() {
                            if epoch == u64::MAX
                                || outgoing
                                    .send((epoch, Err("com_initialization_failed")))
                                    .is_err()
                            {
                                break;
                            }
                        }
                        return;
                    }
                    discovery_loop(&incoming, &outgoing);
                    CoUninitialize();
                }
            })
            .map_err(|_| windows::core::Error::empty())?;
        Ok(Self {
            requests,
            results,
            thread: Some(thread),
        })
    }

    fn stop(&mut self) {
        if self.thread.is_none() {
            return;
        }
        // Shutdown is sent only after destroying the owned child. The window
        // pump never waits synchronously for an accessibility provider reply.
        if self.requests.send((u64::MAX, 0)).is_err() {
            audit("taskbar.accessibility_failed", "discovery_channel_closed");
        }
        if self
            .thread
            .take()
            .is_some_and(|thread| thread.join().is_err())
        {
            audit("taskbar.worker_failed", "discovery_thread_panicked");
        }
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        self.stop();
    }
}

unsafe fn discovery_loop(incoming: &Receiver<(u64, usize)>, outgoing: &Sender<LayoutResult>) {
    let automation = create_automation();
    while let Ok((epoch, display)) = incoming.recv() {
        if epoch == u64::MAX {
            break;
        }
        let result = match &automation {
            Ok(automation) => discover(automation, display),
            Err(_) => Err("accessibility_initialization_failed"),
        };
        if outgoing.send((epoch, result)).is_err() {
            break;
        }
    }
}

unsafe fn create_automation() -> windows::core::Result<IUIAutomation> {
    let automation: IUIAutomation2 = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
    automation.SetConnectionTimeout(1000)?;
    automation.SetTransactionTimeout(1000)?;
    Ok(automation.into())
}

unsafe fn discover(automation: &IUIAutomation, display: usize) -> Result<Layout, &'static str> {
    let parent = FindWindowW(w!("Shell_TrayWnd"), None).map_err(|_| "primary_taskbar_missing")?;
    FindWindowExW(
        Some(parent),
        None,
        w!("Windows.UI.Composition.DesktopWindowContentBridge"),
        None,
    )
    .map_err(|_| "unsupported_taskbar_layout")?;
    let tray = FindWindowExW(Some(parent), None, w!("TrayNotifyWnd"), None)
        .map_err(|_| "notification_area_missing")?;
    let mut bar = RECT::default();
    GetWindowRect(parent, &raw mut bar).map_err(|_| "taskbar_bounds_failed")?;
    let dpi = GetDpiForWindow(parent).max(96);
    let scale = |dip: i32| dip * i32::try_from(dpi).unwrap_or(96) / 96;
    let width = bar.right - bar.left;
    let height = bar.bottom - bar.top;
    if height < scale(32) || height > scale(100) || width < height {
        return Err("unsupported_taskbar_geometry");
    }
    let mut notification = RECT::default();
    GetWindowRect(tray, &raw mut notification).map_err(|_| "notification_bounds_failed")?;
    // Reserve the weather/widgets side even if Explorer exposes no weather
    // leaf. This deliberately trades coverage for conservative placement.
    let mut occupied = vec![(0, scale(220)), (notification.left - bar.left, width)];
    collect_controls(
        automation,
        parent,
        HWND(display as *mut _),
        &bar,
        &mut occupied,
    )?;
    let requested = scale(108);
    let x = find_gap(width, &occupied, requested, scale(4)).ok_or("insufficient_taskbar_space")?;
    Ok(Layout {
        parent: parent.0.addr(),
        bar,
        dpi,
        x,
        requested,
        height,
    })
}

unsafe fn collect_controls(
    automation: &IUIAutomation,
    parent: HWND,
    display: HWND,
    bar: &RECT,
    occupied: &mut Vec<(i32, i32)>,
) -> Result<(), &'static str> {
    let root = automation
        .ElementFromHandle(parent)
        .map_err(|_| "accessibility_unavailable")?;
    let condition = explorer_condition(automation, display)?;
    let cache = automation
        .CreateCacheRequest()
        .map_err(|_| "accessibility_cache_failed")?;
    cache
        .SetTreeFilter(&condition)
        .map_err(|_| "accessibility_cache_failed")?;
    cache
        .SetAutomationElementMode(AutomationElementMode_None)
        .map_err(|_| "accessibility_cache_failed")?;
    cache
        .SetTreeScope(TreeScope(TreeScope_Element.0 | TreeScope_Children.0))
        .map_err(|_| "accessibility_cache_failed")?;
    for property in [
        UIA_BoundingRectanglePropertyId,
        UIA_NativeWindowHandlePropertyId,
        UIA_AutomationIdPropertyId,
        UIA_IsOffscreenPropertyId,
        UIA_ControlTypePropertyId,
    ] {
        cache
            .AddProperty(property)
            .map_err(|_| "accessibility_cache_failed")?;
    }
    // Read one cached layout instead of making Current* calls while Explorer
    // can replace XAML nodes. Names and automation IDs never enter logs.
    let items = root
        .FindAllBuildCache(TreeScope_Descendants, &condition, &cache)
        .map_err(|error| snapshot_error(&error))?;
    let length = items.Length().map_err(|_| "accessibility_unavailable")?;
    if !(1..=512).contains(&length) {
        return Err("unsupported_accessibility_tree");
    }
    let mut start_found = false;
    let deadline = Instant::now() + Duration::from_secs(2);
    for index in 0..length {
        if Instant::now() >= deadline {
            return Err("accessibility_layout_timeout");
        }
        let element = items
            .GetElement(index)
            .map_err(|_| "accessibility_unavailable")?;
        if element
            .CachedIsOffscreen()
            .map_err(|_| "accessibility_unavailable")?
            .as_bool()
        {
            continue;
        }
        let native = element
            .CachedNativeWindowHandle()
            .map_err(|_| "accessibility_unavailable")?;
        if !display.is_invalid() && native == display {
            continue;
        }
        if element
            .CachedAutomationId()
            .map_err(|_| "accessibility_unavailable")?
            == "StartButton"
        {
            start_found = true;
        }
        let kind = element
            .CachedControlType()
            .map_err(|_| "accessibility_unavailable")?;
        // Include actionable controls' padding, even when labels/icons are
        // children. A gap between labels may belong to a clickable app button.
        let actionable = [
            UIA_ButtonControlTypeId,
            UIA_ListItemControlTypeId,
            UIA_MenuItemControlTypeId,
            UIA_EditControlTypeId,
        ]
        .contains(&kind);
        if !actionable && has_child(&element).map_err(|_| "accessibility_child_lookup_failed")? {
            continue;
        }
        let rect = element
            .CachedBoundingRectangle()
            .map_err(|_| "accessibility_unavailable")?;
        if rect.right <= rect.left || rect.bottom <= rect.top {
            continue;
        }
        if rect.left < bar.left
            || rect.right > bar.right
            || rect.top < bar.top
            || rect.bottom > bar.bottom
        {
            return Err("accessibility_bounds_untrusted");
        }
        occupied.push((rect.left - bar.left, rect.right - bar.left));
    }
    if !start_found {
        return Err("start_button_bounds_missing");
    }
    Ok(())
}

unsafe fn has_child(element: &IUIAutomationElement) -> windows::core::Result<bool> {
    // UIA documents S_OK/null for a leaf. The generated wrapper collapses that
    // to Error::empty(). Read the ABI to distinguish it from provider failure.
    let mut children = std::ptr::null_mut();
    (element.vtable().GetCachedChildren)(element.as_raw(), &raw mut children).ok()?;
    if children.is_null() {
        Ok(false)
    } else {
        // The COM call transfers one reference, released by this wrapper.
        let children = IUIAutomationElementArray::from_raw(children);
        Ok(children.Length()? > 0)
    }
}

unsafe fn explorer_condition(
    automation: &IUIAutomation,
    display: HWND,
) -> Result<IUIAutomationCondition, &'static str> {
    if display.is_invalid() {
        return automation
            .CreateTrueCondition()
            .map_err(|_| "accessibility_condition_failed");
    }
    // Exclude our child before building the cache, including cached children.
    // Keep our display outside Explorer occupancy, even though discovery now
    // runs independently of the window-owning message pump.
    let value = i32::try_from(display.0.addr()).map_err(|_| "unsupported_window_handle")?;
    let own = automation
        .CreatePropertyCondition(UIA_NativeWindowHandlePropertyId, &VARIANT::from(value))
        .map_err(|_| "accessibility_condition_failed")?;
    automation
        .CreateNotCondition(&own)
        .map_err(|_| "accessibility_condition_failed")
}

fn snapshot_error(error: &windows::core::Error) -> &'static str {
    tracing::warn!(
        event = "taskbar.accessibility_failed",
        section = "taskbar",
        initiator = "native_worker",
        cause = "snapshot_query_failed",
        trace_route = "taskbar->win32->uia",
        hresult = error.code().0,
        "taskbar accessibility snapshot query failed"
    );
    "accessibility_snapshot_failed"
}

unsafe fn run(
    input: &Arc<Mutex<Input>>,
    stopped: &Arc<AtomicBool>,
    click: &Arc<dyn Fn() + Send + Sync>,
) -> windows::core::Result<()> {
    CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    let result = message_loop(input, stopped, click);
    CoUninitialize();
    result
}

unsafe fn message_loop(
    input: &Arc<Mutex<Input>>,
    stopped: &Arc<AtomicBool>,
    click: &Arc<dyn Fn() + Send + Sync>,
) -> windows::core::Result<()> {
    let instance = GetModuleHandleW(None)?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance.into(),
        lpszClassName: w!("BiFlowTaskbarSpeed"),
        ..Default::default()
    };
    if RegisterClassW(&raw const class) == 0 {
        return Err(windows::core::Error::from_win32());
    }
    let result = pump(input, stopped, click);
    if UnregisterClassW(w!("BiFlowTaskbarSpeed"), Some(instance.into())).is_err() {
        audit("taskbar.cleanup_failed", "unregister_class_failed");
    }
    result
}

unsafe fn pump(
    input: &Arc<Mutex<Input>>,
    stopped: &Arc<AtomicBool>,
    click: &Arc<dyn Fn() + Send + Sync>,
) -> windows::core::Result<()> {
    // Hidden top-level host keeps receiving broadcasts when there is no child.
    let host = CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        w!("BiFlowTaskbarSpeed"),
        None,
        WS_POPUP,
        0,
        0,
        0,
        0,
        None,
        None,
        None,
        None,
    )?;
    let result = pump_host(host, input, stopped, click);
    if DestroyWindow(host).is_err() {
        audit("taskbar.cleanup_failed", "destroy_host_failed");
    }
    result
}

unsafe fn create_worker() -> windows::core::Result<Worker> {
    Ok(Worker {
        hooks: Vec::new(),
        display: HWND::default(),
        parent: HWND::default(),
        discovery: Discovery::start()?,
        pending: false,
        epoch: 0,
        last_layout: Instant::now()
            .checked_sub(Duration::from_secs(30))
            .unwrap_or_else(Instant::now),
        last_failure: None,
        retry_attempt: 0,
    })
}

unsafe fn pump_host(
    host: HWND,
    input: &Arc<Mutex<Input>>,
    stopped: &Arc<AtomicBool>,
    click: &Arc<dyn Fn() + Send + Sync>,
) -> windows::core::Result<()> {
    let mut worker = create_worker()?;
    let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
    if taskbar_created == 0 {
        audit(
            "taskbar.recovery_degraded",
            "shell_broadcast_registration_failed",
        );
    }
    WINDOWS.with(|state| state.set((host, HWND::default(), HWND::default())));
    if SetTimer(Some(host), 1, 500, None) == 0 {
        return Err(windows::core::Error::from_win32());
    }
    let mut previous_enabled = false;
    let mut dirty = false;
    let mut message_error = false;
    let mut msg = MSG::default();
    loop {
        let result = GetMessageW(&raw mut msg, None, 0, 0).0;
        if result <= 0 {
            if result < 0 {
                message_error = true;
                audit("taskbar.worker_failed", "get_message_failed");
            }
            break;
        }
        if stopped.load(Ordering::Acquire) {
            break;
        }
        let mut latest = *input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if latest.sampled.elapsed() > Duration::from_secs(6) {
            latest.upload = None;
            latest.download = None;
        }
        PAINT.with(|state| {
            *state.borrow_mut() = Some(PaintState {
                input: latest,
                click: click.clone(),
            });
        });
        let broadcast = msg.message == taskbar_created
            || matches!(
                msg.message,
                WM_DISPLAYCHANGE
                    | WM_DPICHANGED
                    | WM_SETTINGCHANGE
                    | WM_THEMECHANGED
                    | WM_POWERBROADCAST
            );
        if broadcast || msg.message == WM_LAYOUT || latest.enabled != previous_enabled {
            layout_requested(msg.message);
            worker.epoch = worker.epoch.wrapping_add(1);
            if owns_window(worker.display) {
                let _was_visible = ShowWindow(worker.display, SW_HIDE);
            }
        }
        dirty |= msg.message == WM_LAYOUT;
        worker.finish_layout(latest.enabled);
        let interval = if worker.retry_attempt > 3 { 30 } else { 2 };
        if broadcast
            || (dirty && worker.last_layout.elapsed() >= Duration::from_millis(200))
            || latest.enabled != previous_enabled
            || worker.last_layout.elapsed() >= Duration::from_secs(interval)
        {
            worker.layout(latest.enabled);
            worker.last_layout = Instant::now();
            previous_enabled = latest.enabled;
            dirty = false;
        }
        input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .status = if !latest.enabled {
            Status::Disabled
        } else if worker.display.is_invalid() {
            Status::Unavailable
        } else {
            Status::Attached
        };
        if msg.message == WM_TIMER {
            repaint(worker.display);
        }
        let _translated = TranslateMessage(&raw const msg);
        DispatchMessageW(&raw const msg);
    }
    worker.destroy();
    worker.discovery.stop();
    if KillTimer(Some(host), 1).is_err() {
        audit("taskbar.cleanup_failed", "kill_timer_failed");
    }
    PAINT.with(|state| *state.borrow_mut() = None);
    WINDOWS.with(|state| state.set((HWND::default(), HWND::default(), HWND::default())));
    stopped_event();
    if message_error {
        Err(windows::core::Error::empty())
    } else {
        Ok(())
    }
}

fn layout_requested(message: u32) {
    tracing::debug!(
        event = "taskbar.layout_requested",
        section = "taskbar",
        initiator = "message_pump",
        cause = "layout_event",
        trace_route = "win32->taskbar",
        message_id = message,
        "taskbar layout check requested"
    );
}

unsafe fn repaint(hwnd: HWND) {
    if !owns_window(hwnd) {
        return;
    }
    if !InvalidateRect(Some(hwnd), None, false).as_bool() || !UpdateWindow(hwnd).as_bool() {
        audit("taskbar.paint_failed", "repaint_failed");
    }
}

fn stopped_event() {
    tracing::info!(
        event = "taskbar.stopped",
        section = "taskbar",
        initiator = "native_worker",
        cause = "application_shutdown",
        trace_route = "taskbar->win32",
        "taskbar resources released"
    );
}

struct GdiObject(HGDIOBJ);
impl Drop for GdiObject {
    fn drop(&mut self) {
        // SAFETY: guards are created, deselected and dropped on the owner thread.
        if unsafe { !DeleteObject(self.0).as_bool() } {
            audit("taskbar.cleanup_failed", "delete_gdi_object_failed");
        }
    }
}

unsafe fn paint(hwnd: HWND, input: Input) {
    let mut ps = PAINTSTRUCT::default();
    let dc = BeginPaint(hwnd, &raw mut ps);
    if dc.is_invalid() {
        audit("taskbar.paint_failed", "begin_paint_failed");
        return;
    }
    let mut rect = RECT::default();
    inspect_surface(hwnd, dc);
    if GetClientRect(hwnd, &raw mut rect).is_err() {
        audit("taskbar.paint_failed", "client_bounds_failed");
    } else if let Err(cause) = paint_content(dc, rect, GetDpiForWindow(hwnd).max(96), input) {
        audit("taskbar.paint_failed", cause);
    }
    if !EndPaint(hwnd, &raw const ps).as_bool() {
        audit("taskbar.paint_failed", "end_paint_failed");
    }
}

unsafe fn inspect_surface(hwnd: HWND, dc: HDC) {
    let mut clip = RECT::default();
    let region = GetClipBox(dc, &raw mut clip);
    let layered =
        GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & isize::try_from(WS_EX_LAYERED.0).unwrap_or(0) != 0;
    let surface = if clip.right <= clip.left || clip.bottom <= clip.top {
        3
    } else if layered {
        1
    } else {
        2
    };
    let previous = RENDER_SURFACE.swap(surface, Ordering::Relaxed);
    if surface != 1 && previous != surface {
        audit("taskbar.surface_unavailable", "composition_or_clip_invalid");
    }
    PAINT_DIAGNOSED.with(|diagnosed| {
        if !diagnosed.replace(true) {
            tracing::info!(
                event = "taskbar.paint_surface",
                section = "taskbar",
                initiator = "native_renderer",
                cause = "first_paint",
                trace_route = "taskbar->gdi",
                layered,
                clip_kind = region.0,
                clip_width = clip.right - clip.left,
                clip_height = clip.bottom - clip.top,
                "native paint surface inspected"
            );
        }
    });
}

unsafe fn theme_colors() -> (COLORREF, COLORREF) {
    let mut light = 1_u32;
    let mut size = 4_u32;
    let theme = RegGetValueW(
        HKEY_CURRENT_USER,
        w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
        w!("SystemUsesLightTheme"),
        RRF_RT_REG_DWORD,
        None,
        Some(std::ptr::from_mut(&mut light).cast()),
        Some(&raw mut size),
    );
    if theme.is_err() {
        // Missing preferences are normal for a fresh profile. Read system colors
        // as an explicit fallback; this feature never writes to the registry.
        if !THEME_FALLBACK_LOGGED.swap(true, Ordering::Relaxed) {
            tracing::info!(
                event = "taskbar.theme_fallback",
                section = "taskbar",
                initiator = "native_worker",
                cause = "system_theme_preference_unavailable",
                trace_route = "taskbar->win32",
                "taskbar is using system colors"
            );
        }
        return (
            COLORREF(GetSysColor(COLOR_BTNFACE)),
            COLORREF(GetSysColor(COLOR_BTNTEXT)),
        );
    }
    if light == 0 {
        (COLORREF(0x0020_2020), COLORREF(0x00ff_ffff))
    } else {
        (COLORREF(0x00f3_f3f3), COLORREF(0x0010_1010))
    }
}

unsafe fn paint_content(dc: HDC, rect: RECT, dpi: u32, input: Input) -> Result<(), &'static str> {
    let (_background, foreground) = theme_colors();
    let brush = CreateSolidBrush(BACKGROUND_KEY);
    if brush.is_invalid() {
        return Err("create_brush_failed");
    }
    let _brush_guard = GdiObject(brush.into());
    if FillRect(dc, &raw const rect, brush) == 0 {
        return Err("fill_background_failed");
    }
    let font = CreateFontW(
        -i32::try_from(12 * dpi / 96).unwrap_or(12),
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        DEFAULT_QUALITY,
        DEFAULT_PITCH.0.into(),
        w!("Consolas"),
    );
    if font.is_invalid() {
        return Err("create_font_failed");
    }
    let _font_guard = GdiObject(font.into());
    let previous = SelectObject(dc, font.into());
    if previous.is_invalid() {
        return Err("select_font_failed");
    }
    let result = if SetBkMode(dc, TRANSPARENT) == 0 || SetTextColor(dc, foreground).0 == u32::MAX {
        Err("text_style_failed")
    } else {
        paint_rows(dc, rect, dpi, input)
    };
    if SelectObject(dc, previous).is_invalid() {
        audit("taskbar.paint_failed", "restore_font_failed");
    }
    result
}

unsafe fn paint_rows(dc: HDC, rect: RECT, dpi: u32, input: Input) -> Result<(), &'static str> {
    let middle = rect.bottom / 2;
    let row_height = i32::try_from(15 * dpi / 96).unwrap_or(16);
    let label_width = i32::try_from(15 * dpi / 96).unwrap_or(24);
    for ((arrow, rate), top) in rows(input.upload, input.download)
        .into_iter()
        .zip([middle - row_height, middle])
    {
        // Each arrow and value has its own fixed area, independent of digits/units.
        let mut label = RECT {
            left: 4,
            top,
            right: label_width,
            bottom: top + row_height,
        };
        let mut value = RECT {
            left: label_width,
            top,
            right: rect.right - 4,
            bottom: top + row_height,
        };
        let mut arrow: Vec<u16> = arrow.encode_utf16().collect();
        let mut text: Vec<u16> = format_rate(rate).encode_utf16().collect();
        let flags = DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX;
        if DrawTextW(dc, &mut arrow, &raw mut label, flags | DT_LEFT) == 0
            || DrawTextW(dc, &mut text, &raw mut value, flags | DT_RIGHT) == 0
        {
            return Err("draw_text_failed");
        }
    }
    Ok(())
}
