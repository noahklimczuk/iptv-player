//! A loading screen on the screen within milliseconds of the click.
//!
//! The app's real window cannot appear before its web view has painted something: it is
//! transparent so the video can composite behind it, and a transparent window with no
//! page in it is a hole straight through to the desktop (`commands::window_ready`).
//! Starting WebView2 is the slowest part of a launch -- one to two seconds on a warm
//! machine, more on a cold disk -- and until it is done there was nothing at all to
//! show that Aurora had been launched.
//!
//! So the very first thing `main` does is put up this: a plain Win32 window, drawn with
//! GDI, the same size and in the same place as the app's window, painted exactly like
//! the boot screen in `index.html` (the background, the mark, "Starting Aurora…"). It
//! needs no web view, so it is up in a few milliseconds. When the real window is shown
//! -- by the UI, or by the fallback in `main` -- this one is closed, and because the two
//! look the same and sit in the same place, the handover is the boot screen carrying on.
//!
//! It runs on a thread of its own with its own message loop, because the main thread is
//! busy building Tauri. Nothing waits on it, and if it cannot be made the launch carries
//! on exactly as it did before it existed.

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use windows::core::w;
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
        GetStockObject, RoundRect, SelectObject, SetBkMode, SetTextColor, UpdateWindow, DT_CENTER,
        DT_SINGLELINE, DT_VCENTER, HGDIOBJ, NULL_PEN, PAINTSTRUCT, TRANSPARENT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, LoadCursorW,
        PostMessageW, PostQuitMessage, RegisterClassW, ShowWindow, SystemParametersInfoW,
        TranslateMessage, IDC_ARROW, MSG, SPI_GETWORKAREA, SW_SHOWNORMAL,
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_DESTROY,
        WM_PAINT, WNDCLASSW, WS_CAPTION, WS_OVERLAPPED,
    };

    /// The window, once it exists, so another thread can close it. Zero when there is
    /// none: not made yet, already closed, or never made at all.
    static WINDOW: AtomicUsize = AtomicUsize::new(0);
    /// Set by [`close`], so a window that finishes being made after the app's own is
    /// already up takes itself down rather than sitting over it.
    static CLOSED: AtomicBool = AtomicBool::new(false);
    /// Milliseconds from `show` to the screen being painted, plus one; zero until then.
    pub static PAINTED_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    /// Ours, so Alt+F4 on the loading screen does not end it early and leave nothing.
    const CLOSE: u32 = WM_APP + 1;

    /// `tauri.conf.json`'s window, in logical pixels.
    const WIDTH: i32 = 1440;
    const HEIGHT: i32 = 900;

    // `index.html`'s boot screen, in COLORREF order (0x00BBGGRR).
    const BACKGROUND: COLORREF = COLORREF(0x000f_0b0b); // #0b0b0f
    const MARK: COLORREF = COLORREF(0x00e7_5c6c); // #6c5ce7
    const MARK_TEXT: COLORREF = COLORREF(0x00ff_ffff);
    const LINE: COLORREF = COLORREF(0x0078_6b6b); // #6b6b78

    // The app's window has a dark title bar; without this the loading screen's would be
    // light and the handover would flash. Linked directly rather than through a `windows`
    // feature, which would rebuild that whole crate for one call.
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: *mut core::ffi::c_void,
            attribute: u32,
            value: *const core::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    /// `DWMWA_USE_IMMERSIVE_DARK_MODE`.
    const DARK_TITLE_BAR: u32 = 20;

    fn rgb_brush(c: COLORREF) -> windows::Win32::Graphics::Gdi::HBRUSH {
        unsafe { CreateSolidBrush(c) }
    }

    pub fn show() {
        let asked = std::time::Instant::now();
        // For measuring a launch without it, and for harnesses that look for windows.
        if std::env::var_os("AURORA_NO_SPLASH").is_some() {
            return;
        }
        let _ = std::thread::Builder::new()
            .name("aurora-splash".into())
            .spawn(move || unsafe { run(asked) });
    }

    pub fn close() {
        CLOSED.store(true, Ordering::Release);
        let hwnd = WINDOW.swap(0, Ordering::AcqRel);
        if hwnd != 0 {
            unsafe {
                let _ = PostMessageW(
                    HWND(hwnd as *mut core::ffi::c_void),
                    CLOSE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }

    /// Where the app's window will be: its size at this DPI with its frame, centred in
    /// the work area and shrunk to fit it, as `window::place_in_middle` does.
    unsafe fn placement() -> (i32, i32, i32, i32) {
        let dpi = GetDpiForSystem().max(96) as i32;
        let mut frame = RECT {
            left: 0,
            top: 0,
            right: WIDTH * dpi / 96,
            bottom: HEIGHT * dpi / 96,
        };
        let style = WS_OVERLAPPED | WS_CAPTION;
        let _ = AdjustWindowRectExForDpi(&mut frame, style, false, WINDOW_EX_STYLE(0), dpi as u32);
        let (mut w, mut h) = (frame.right - frame.left, frame.bottom - frame.top);

        let mut area = RECT::default();
        if SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut area as *mut RECT as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_err()
        {
            return (0, 0, w, h);
        }
        let (aw, ah) = (area.right - area.left, area.bottom - area.top);
        w = w.min(aw);
        h = h.min(ah);
        (area.left + (aw - w) / 2, area.top + (ah - h) / 2, w, h)
    }

    unsafe fn run(asked: std::time::Instant) {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let class = w!("AuroraSplash");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: module.into(),
            lpszClassName: class,
            hbrBackground: rgb_brush(BACKGROUND),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return;
        }
        let (x, y, w, h) = placement();
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("Aurora TV"),
            WS_OVERLAPPED | WS_CAPTION,
            x,
            y,
            w,
            h,
            None,
            None,
            module,
            None,
        ) else {
            return;
        };
        WINDOW.store(hwnd.0 as usize, Ordering::Release);
        if CLOSED.load(Ordering::Acquire) {
            let _ = DestroyWindow(hwnd);
            WINDOW.store(0, Ordering::Release);
            return;
        }
        let dark: i32 = 1;
        DwmSetWindowAttribute(
            hwnd.0,
            DARK_TITLE_BAR,
            &dark as *const i32 as *const core::ffi::c_void,
            4,
        );
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = UpdateWindow(hwnd);
        PAINTED_MS.store(asked.elapsed().as_millis() as u64 + 1, Ordering::Release);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    unsafe extern "system" fn proc_(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            // Not from the keyboard: the loading screen goes when the app is up.
            WM_CLOSE => LRESULT(0),
            CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    unsafe fn font(px: i32, weight: i32) -> windows::Win32::Graphics::Gdi::HFONT {
        CreateFontW(
            -px,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            0, // DEFAULT_CHARSET is 1, ANSI_CHARSET 0; Segoe UI covers both for this text.
            0,
            0,
            5, // CLEARTYPE_QUALITY
            0,
            w!("Segoe UI"),
        )
    }

    /// The boot screen: the 56 px mark, an 18 px gap and the line under it, centred.
    unsafe fn paint(hwnd: HWND) {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let scale = |v: i32| v * GetDpiForSystem().max(96) as i32 / 96;

        let mut client = RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut client);
        let bg = rgb_brush(BACKGROUND);
        FillRect(hdc, &client, bg);
        let _ = DeleteObject(HGDIOBJ(bg.0));

        let (mark, gap, line) = (scale(56), scale(18), scale(18));
        let top = (client.bottom - (mark + gap + line)) / 2;
        let left = (client.right - mark) / 2;

        let brush = rgb_brush(MARK);
        let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        let r = scale(13) * 2;
        let _ = RoundRect(hdc, left, top, left + mark + 1, top + mark + 1, r, r);
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));

        SetBkMode(hdc, TRANSPARENT);
        let big = font(scale(26), 800);
        let old_font = SelectObject(hdc, HGDIOBJ(big.0));
        SetTextColor(hdc, MARK_TEXT);
        let mut a: Vec<u16> = "A".encode_utf16().collect();
        let mut rect = RECT {
            left,
            top,
            right: left + mark,
            bottom: top + mark,
        };
        DrawTextW(
            hdc,
            &mut a,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );

        let small = font(scale(13), 400);
        SelectObject(hdc, HGDIOBJ(small.0));
        SetTextColor(hdc, LINE);
        let mut text: Vec<u16> = "Starting Aurora\u{2026}".encode_utf16().collect();
        let mut rect = RECT {
            left: 0,
            top: top + mark + gap,
            right: client.right,
            bottom: top + mark + gap + line,
        };
        DrawTextW(
            hdc,
            &mut text,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );

        SelectObject(hdc, old_font);
        let _ = DeleteObject(HGDIOBJ(big.0));
        let _ = DeleteObject(HGDIOBJ(small.0));
        let _ = EndPaint(hwnd, &ps);
    }
}

/// Put the loading screen up. Returns at once; the window is made on its own thread.
#[cfg(windows)]
pub fn show() {
    imp::show();
}

/// Take it down: the app's own window is on screen. Safe to call any number of times,
/// from any thread, and before `show` has finished making it.
#[cfg(windows)]
pub fn close() {
    imp::close();
}

/// How long after `show` the loading screen was on screen, for the launch log. `None`
/// when it has not been painted (yet, or at all).
#[cfg(windows)]
pub fn painted_after_ms() -> Option<u64> {
    match imp::PAINTED_MS.load(std::sync::atomic::Ordering::Acquire) {
        0 => None,
        n => Some(n - 1),
    }
}

#[cfg(not(windows))]
pub fn painted_after_ms() -> Option<u64> {
    None
}

#[cfg(not(windows))]
pub fn show() {}

#[cfg(not(windows))]
pub fn close() {}
