//! The main window: where it opens, and the handle multi-view tiles are children of.

/// Where the window opens: in the middle of the main screen's work area (the screen
/// less the taskbar), shrunk to fit if the configured size is bigger than that.
///
/// Without this Windows put the window wherever it chose for a new window, which on a
/// two-screen desk was usually the top left of one of them.
///
/// `work` is `(x, y, width, height)` of the work area and `want` the window's outer
/// size, both in physical pixels. Returns the window's `(x, y, width, height)`.
pub fn centred(work: (i32, i32, u32, u32), want: (u32, u32)) -> (i32, i32, u32, u32) {
    let (wx, wy, ww, wh) = work;
    let w = want.0.min(ww);
    let h = want.1.min(wh);
    let x = wx + ((ww - w) / 2) as i32;
    let y = wy + ((wh - h) / 2) as i32;
    (x, y, w, h)
}

/// Move the main window to the middle of the primary screen. Best effort: a failure
/// leaves it where Windows put it, which is a worse place but not a broken app.
pub fn place_in_middle(window: &tauri::WebviewWindow) {
    let monitor = match window.primary_monitor() {
        Ok(Some(m)) => m,
        _ => match window.current_monitor() {
            Ok(Some(m)) => m,
            _ => return,
        },
    };
    let (Ok(outer), Ok(inner)) = (window.outer_size(), window.inner_size()) else {
        return;
    };
    let area = monitor.work_area();
    let (x, y, w, h) = centred(
        (
            area.position.x,
            area.position.y,
            area.size.width,
            area.size.height,
        ),
        (outer.width, outer.height),
    );
    if (w, h) != (outer.width, outer.height) {
        // `set_size` is the inner size; take off the frame so the outer size is (w, h).
        let frame_w = outer.width.saturating_sub(inner.width);
        let frame_h = outer.height.saturating_sub(inner.height);
        let _ = window.set_size(tauri::PhysicalSize::new(
            w.saturating_sub(frame_w),
            h.saturating_sub(frame_h),
        ));
    }
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

#[cfg(windows)]
pub fn record_mosaic_window(window: &tauri::WebviewWindow, mosaic: &crate::mosaic::Mosaic) {
    let size = window.inner_size().ok();
    match window.hwnd() {
        Ok(hwnd) => mosaic.set_window(
            hwnd.0 as isize,
            size.map(|s| s.width).unwrap_or(1280),
            size.map(|s| s.height).unwrap_or(720),
        ),
        Err(e) => tracing::warn!("multi-view will have no video surfaces: {e}"),
    }
}

/// Off Windows there is no handle to be a child of and tiles draw nothing, so the size
/// is all the mosaic needs, and it needs that only so the tile rectangles it reports to
/// the UI are the right shape.
#[cfg(not(windows))]
pub fn record_mosaic_window(window: &tauri::WebviewWindow, mosaic: &crate::mosaic::Mosaic) {
    let size = window.inner_size().ok();
    mosaic.set_window(
        0,
        size.map(|s| s.width).unwrap_or(1280),
        size.map(|s| s.height).unwrap_or(720),
    );
}

#[cfg(test)]
mod tests {
    use super::centred;

    #[test]
    fn the_window_opens_in_the_middle_of_the_screen_above_the_taskbar() {
        // A 1080p screen with a 48 px taskbar, and the default 1440x900 window.
        assert_eq!(
            centred((0, 0, 1920, 1032), (1440, 900)),
            (240, 66, 1440, 900)
        );
        // A screen to the left of the main one keeps its own origin.
        assert_eq!(
            centred((-1920, 0, 1920, 1032), (1440, 900)),
            (-1680, 66, 1440, 900)
        );
    }

    #[test]
    fn a_window_bigger_than_the_screen_is_shrunk_to_fit_it() {
        // 1440x900 at 150% scaling is 2160x1350 physical, on a 1920x1032 work area.
        assert_eq!(
            centred((0, 0, 1920, 1032), (2160, 1350)),
            (0, 0, 1920, 1032)
        );
    }
}
