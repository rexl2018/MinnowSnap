use anyhow::{Result, anyhow};
use gpui::{App, Window};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn raw_window_handle(window: &Window) -> Result<RawWindowHandle> {
    HasWindowHandle::window_handle(window)
        .map(|handle| handle.as_raw())
        .map_err(|error| anyhow!("failed to get native window handle: {error}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Level {
    #[default]
    Normal,
    AlwaysOnTop,
}

pub trait WindowLevelExt {
    fn set_level(&mut self, level: Level) -> Result<()>;
    fn set_click_through(&mut self, enabled: bool) -> Result<()>;
}

impl WindowLevelExt for Window {
    fn set_level(&mut self, level: Level) -> Result<()> {
        platform::set_level(self, level)
    }

    fn set_click_through(&mut self, enabled: bool) -> Result<()> {
        platform::set_click_through(self, enabled)
    }
}

#[cfg_attr(any(target_os = "windows", target_os = "macos"), allow(dead_code))]
fn unsupported_platform_operation(operation: &str) -> Result<()> {
    Err(anyhow!("{operation} are not implemented for this platform"))
}

fn log_window_level_result(_level: Level, result: Result<()>) {
    if let Err(err) = result {
        tracing::warn!("Failed to apply native window level: {err}");
    }
}

/// cx.open_window(opts, with_level(Level::AlwaysOnTop, |window, cx| { ... }))
pub fn with_level<T>(level: Level, build: impl FnOnce(&mut Window, &mut App) -> T) -> impl FnOnce(&mut Window, &mut App) -> T {
    move |window, cx| {
        log_window_level_result(level, window.set_level(level));
        build(window, cx)
    }
}

/// Stretch a popup so it covers the active display, including the macOS menu bar.
pub fn cover_active_screen(window: &Window) -> Result<()> {
    platform::cover_active_screen(window)
}

/// Exclude a window from screen-capture output while keeping it visible on
/// screen. Used for the long-capture selection border/toolbar overlays so they
/// are not baked into the frames the stitcher grabs via the screen-capture API.
pub fn exclude_from_capture(window: &Window) -> Result<()> {
    platform::exclude_from_capture(window)
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use windows::Win32::Foundation::{GetLastError, HWND, SetLastError, WIN32_ERROR};
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetWindowDisplayAffinity,
        SetWindowLongPtrW, SetWindowPos, WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED, WS_EX_TRANSPARENT,
    };

    fn hwnd(window: &Window) -> Result<HWND> {
        let raw = super::raw_window_handle(window)?;

        match raw {
            RawWindowHandle::Win32(h) => Ok(HWND(h.hwnd.get() as *mut _)),
            other => Err(anyhow!("expected Win32 handle, got {other:?}")),
        }
    }

    pub(super) fn exclude_from_capture(window: &Window) -> Result<()> {
        let hwnd = hwnd(window)?;
        // WDA_EXCLUDEFROMCAPTURE hides the window from screen capture while
        // leaving it visible on screen (Windows 10 2004+).
        unsafe {
            SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).map_err(|e| anyhow!("SetWindowDisplayAffinity failed: {e}"))?;
        }
        Ok(())
    }

    pub(super) fn set_level(window: &Window, level: Level) -> Result<()> {
        let hwnd = hwnd(window)?;

        unsafe {
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let desired_style = match level {
                Level::Normal => ex_style & !WS_EX_LAYERED.0 & !WS_EX_TRANSPARENT.0,
                Level::AlwaysOnTop => ex_style | WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0,
            };

            if desired_style != ex_style {
                SetLastError(WIN32_ERROR(0));
                let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desired_style as isize);
                let last_error = GetLastError();
                if last_error != WIN32_ERROR(0) {
                    return Err(anyhow!("SetWindowLongPtrW failed: {last_error:?}"));
                }
            }

            let (insert_after, flags) = match level {
                Level::Normal => (HWND_NOTOPMOST, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW),
                Level::AlwaysOnTop => (HWND_TOPMOST, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW),
            };

            SetWindowPos(hwnd, Some(insert_after), 0, 0, 0, 0, flags).map_err(|e| anyhow!("SetWindowPos failed: {e}"))?;
        }

        Ok(())
    }

    pub(super) fn cover_active_screen(_window: &Window) -> Result<()> {
        Ok(())
    }

    pub(super) fn set_click_through(window: &Window, enabled: bool) -> Result<()> {
        let hwnd = hwnd(window)?;

        unsafe {
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let mut desired_style = ex_style | WS_EX_LAYERED.0;
            if enabled {
                desired_style |= WS_EX_TRANSPARENT.0;
            } else {
                desired_style &= !WS_EX_TRANSPARENT.0;
            }

            if desired_style != ex_style {
                SetLastError(WIN32_ERROR(0));
                let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desired_style as isize);
                let last_error = GetLastError();
                if last_error != WIN32_ERROR(0) {
                    return Err(anyhow!("SetWindowLongPtrW failed: {last_error:?}"));
                }
            }
        }

        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSScreen, NSScreenSaverWindowLevel, NSView, NSWindowCollectionBehavior, NSWindowSharingType, NSWindowStyleMask};
    use raw_window_handle::RawWindowHandle;

    pub(super) fn exclude_from_capture(window: &Window) -> Result<()> {
        // NSWindowSharingType::None keeps the window on screen but omits it from
        // the screen-capture/window-list APIs, so our selection border is not
        // baked into the frames the stitcher grabs.
        let ns_window = ns_window(window)?;
        ns_window.setSharingType(NSWindowSharingType::None);
        Ok(())
    }

    pub(super) fn set_level(window: &Window, level: Level) -> Result<()> {
        let ns_window = ns_window(window)?;
        match level {
            Level::Normal => ns_window.setLevel(0),
            Level::AlwaysOnTop => {
                ns_window.setLevel(NSScreenSaverWindowLevel);
                ns_window.setCollectionBehavior(
                    NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary,
                );
                ns_window.setHidesOnDeactivate(false);
            }
        }
        Ok(())
    }

    pub(super) fn cover_active_screen(window: &Window) -> Result<()> {
        let ns_window = ns_window(window)?;
        // GPUI popups still use NSTitledWindowMask, so AppKit constrains them
        // below the menu bar. Borderless windows can cover it once the level is
        // above NSMainMenuWindowLevel.
        ns_window.setStyleMask(NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel);
        ns_window.setHasShadow(false);
        ns_window.setOpaque(true);
        ns_window.setHidesOnDeactivate(false);
        ns_window.setIgnoresMouseEvents(false);
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        ns_window.setLevel(NSScreenSaverWindowLevel);

        let frame = ns_window
            .screen()
            .map(|screen| screen.frame())
            .or_else(|| MainThreadMarker::new().and_then(NSScreen::mainScreen).map(|screen| screen.frame()))
            .ok_or_else(|| anyhow!("no screen available for overlay"))?;
        ns_window.setFrame_display(frame, true);
        Ok(())
    }

    pub(super) fn set_click_through(window: &Window, enabled: bool) -> Result<()> {
        // Let mouse events (scroll, clicks) pass through the transparent frame
        // overlay to the window beneath it, so the user can scroll the target
        // content while the selection border stays drawn on top. Without this
        // the long-capture frame window would have to be torn down to remain
        // interactive, which is why the selection box used to vanish.
        let ns_window = ns_window(window)?;
        ns_window.setIgnoresMouseEvents(enabled);
        Ok(())
    }

    fn ns_window(window: &Window) -> Result<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
        let RawWindowHandle::AppKit(handle) = raw_window_handle(window)? else {
            return Err(anyhow!("expected AppKit window handle"));
        };
        // SAFETY: GPUI's AppKit handle is an NSView that outlives the Window.
        let ns_view = unsafe { objc2::rc::Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
            .ok_or_else(|| anyhow!("failed to retain NSView"))?;
        ns_view.window().ok_or_else(|| anyhow!("NSView has no NSWindow"))
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    use super::*;

    pub(super) fn exclude_from_capture(_window: &Window) -> Result<()> {
        Ok(())
    }

    pub(super) fn set_level(_window: &Window, level: Level) -> Result<()> {
        if matches!(level, Level::Normal) {
            Ok(())
        } else {
            unsupported_platform_operation("window levels")
        }
    }

    pub(super) fn cover_active_screen(_window: &Window) -> Result<()> {
        Ok(())
    }

    pub(super) fn set_click_through(_window: &Window, enabled: bool) -> Result<()> {
        if !enabled {
            Ok(())
        } else {
            unsupported_platform_operation("click-through")
        }
    }
}
