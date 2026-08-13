pub mod background_host;
pub mod clipboard;
pub mod hotkey;
pub mod logging;
pub mod native_window;
pub mod notify;
pub mod screen_recording;
pub mod shell;
pub mod shutdown;
pub mod storage;
pub mod system;
pub mod tray;
pub mod window_drag;
pub mod windowing;

use gpui::{App, AsyncApp};

pub fn app_ready(cx: &mut AsyncApp) -> bool {
    !crate::platform::shutdown::is_shutting_down() && cx.update(|_| ()).is_ok()
}

pub fn update_app(cx: &mut AsyncApp, f: impl FnOnce(&mut App)) -> bool {
    !crate::platform::shutdown::is_shutting_down() && cx.update(f).is_ok()
}
