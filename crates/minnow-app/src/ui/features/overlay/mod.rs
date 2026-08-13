mod actions;
mod annotation;
pub(crate) mod render;
mod state;
mod view;
pub(crate) mod window_catalog;

use crate::platform::shell::{self, PopupWindowSpec};
use crate::ui::support::appearance;
use gpui::{App, AppContext, Bounds, WindowBounds, WindowKind, WindowOptions};
use gpui_component::Root;

pub use actions::bind_keys;
pub use state::OverlayHandle;
use view::OverlayView;

pub fn open_window(cx: &mut App) {
    let options = window_options(cx);
    let overlay_handle = cx.global::<OverlayHandle>().clone();

    if let Err(err) = cx.open_window(
        options,
        shell::with_screen_overlay(move |window, cx| {
            appearance::apply_saved_preferences(Some(window), cx);
            shell::configure_window(window, cx, true);
            let focus_handle = cx.focus_handle();
            let overlay_handle = overlay_handle.clone();
            let view = cx.new(move |cx| OverlayView::new(overlay_handle, focus_handle, cx));
            cx.new(move |cx| Root::new(view, window, cx))
        }),
    ) {
        tracing::error!("Failed to open overlay window: {err}");
    }
}

fn window_options(cx: &App) -> WindowOptions {
    let display_id = cx.primary_display().map(|display| display.id());
    let fullscreen_bounds = Bounds::maximized(display_id, cx);

    shell::popup_window_options(PopupWindowSpec {
        // Cover the real menu bar instead of entering macOS fullscreen, which
        // leaves the system menu bar visible on top of a screenshot that also
        // contains it.
        window_bounds: Some(WindowBounds::Windowed(fullscreen_bounds)),
        kind: WindowKind::PopUp,
        focus: false,
        show: true,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id,
        window_min_size: None,
    })
}
