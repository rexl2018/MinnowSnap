//! Screen Recording permission handling.
//!
//! On macOS (Catalina 10.15+), capturing the screen requires the Screen
//! Recording TCC permission. Without it, the system APIs used by `xcap`
//! succeed silently but return a frame containing only the desktop wallpaper
//! and the cursor — every application window is omitted. This module turns
//! that silent failure into an explicit, actionable check.
//!
//! On every other platform the permission does not exist, so the checks are
//! no-ops that report "granted".

/// The macOS "Screen & System Audio Recording" privacy pane deep link.
#[cfg(target_os = "macos")]
const SCREEN_RECORDING_SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";

/// Returns `true` when the process is allowed to capture other windows.
///
/// This never triggers the system permission prompt; use [`request_access`]
/// for that. On non-macOS targets this always returns `true`.
#[must_use]
pub(crate) fn has_access() -> bool {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: `CGPreflightScreenCaptureAccess` is a parameterless C
        // function available since macOS 10.15 that returns a `bool`.
        unsafe { CGPreflightScreenCaptureAccess() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Requests Screen Recording access, prompting the user the first time.
///
/// macOS only shows the system prompt once per process identity; subsequent
/// calls just report the current state. Returns the access state after the
/// request. On non-macOS targets this always returns `true`.
#[must_use]
pub(crate) fn request_access() -> bool {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: `CGRequestScreenCaptureAccess` is a parameterless C function
        // available since macOS 10.15 that returns a `bool`.
        unsafe { CGRequestScreenCaptureAccess() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Opens the system Screen Recording privacy settings so the user can grant
/// access. No-op on non-macOS targets.
pub(crate) fn open_settings() {
    #[cfg(target_os = "macos")]
    {
        if let Err(err) = std::process::Command::new("open").arg(SCREEN_RECORDING_SETTINGS_URL).spawn() {
            tracing::error!("Failed to open Screen Recording settings: {err}");
        }
    }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}
