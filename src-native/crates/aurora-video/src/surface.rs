//! The window mpv draws into (E29).
//!
//! A child of the app's window, kept at the bottom of the z-order so the web view —
//! transparent where the page wants the picture to show — sits above it. Disabled, so it
//! can never take a click or the focus from the page above it.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, SetWindowPos, HWND_BOTTOM,
    SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, WINDOW_EX_STYLE,
    WNDCLASSW, WS_CHILD, WS_DISABLED,
};

const CLASS: PCWSTR = w!("AuroraVideoSurface");

unsafe extern "system" fn proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(h, msg, w, l)
}

fn register() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // Safety: a class with a static name and a plain window procedure. A second
        // registration fails harmlessly, which `Once` avoids anyway.
        unsafe {
            let instance = GetModuleHandleW(None).unwrap_or_default();
            let class = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: instance.into(),
                lpszClassName: CLASS,
                hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                ..Default::default()
            };
            RegisterClassW(&class);
        }
    });
}

/// A rectangle in physical pixels of the parent's client area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub struct Surface {
    hwnd: HWND,
}

// Safety: a window handle is a plain identifier. The calls made on it here
// (`SetWindowPos`, `DestroyWindow` from the creating thread on drop) are ones Windows
// allows from any thread for a child window, and mpv uses it from its own threads by
// design — that is what `wid` is.
unsafe impl Send for Surface {}
unsafe impl Sync for Surface {}

impl Surface {
    /// Create a hidden surface inside `parent`.
    pub fn create(parent: isize, rect: Rect) -> Result<Self, String> {
        register();
        // Safety: a child of a window the caller owns, with a registered class.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS,
                w!(""),
                // Never `WS_CLIPSIBLINGS`. The web view is a sibling above this one and
                // covers the whole window, so with it the surface's visible region is
                // empty: Windows clips every frame mpv presents, and the transparent page
                // shows the desktop through a window with sound and no picture (1.0.81 to
                // 1.0.84). The web view is the one that must not be drawn over, and it
                // already is not, being above this in z-order.
                WS_CHILD | WS_DISABLED,
                rect.x,
                rect.y,
                rect.width.max(1) as i32,
                rect.height.max(1) as i32,
                HWND(parent as *mut _),
                None,
                GetModuleHandleW(None).unwrap_or_default(),
                None,
            )
        }
        .map_err(|e| format!("the video surface could not be created: {e}"))?;
        Ok(Self { hwnd })
    }

    /// The handle mpv is given as `wid`.
    pub fn wid(&self) -> isize {
        self.hwnd.0 as isize
    }

    /// Move and size it, keeping it under the web view.
    pub fn place(&self, rect: Rect) -> Result<(), String> {
        // Safety: our own window.
        unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_BOTTOM,
                rect.x,
                rect.y,
                rect.width.max(1) as i32,
                rect.height.max(1) as i32,
                SWP_NOACTIVATE,
            )
        }
        .map_err(|e| format!("the video surface could not be moved: {e}"))
    }

    pub fn set_visible(&self, visible: bool) -> Result<(), String> {
        let show = if visible {
            SWP_SHOWWINDOW
        } else {
            SWP_HIDEWINDOW
        };
        // Safety: our own window.
        unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_BOTTOM,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | show,
            )
        }
        .map_err(|e| format!("the video surface could not be shown or hidden: {e}"))
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // Safety: our own window, destroyed once.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
