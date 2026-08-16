mod coordinator;
mod layout;
mod view;

use crate::platform::shell::{self, PopupWindowSpec};
use crate::services::geometry::{Rect, RectF};
use crate::ui::support::appearance;
use gpui::{App, AppContext, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions};
use std::sync::Arc;
use view::{FrameWindowView, LongCaptureToolbarAction, PreviewWindowView, ToolbarWindowView};
use {coordinator::LongCaptureCoordinator, coordinator::LongCaptureWindowKind, layout::compute_window_layout};

#[derive(Clone)]
pub struct LongCaptureRequest {
    pub selection_rect: Rect,
    pub viewport_rect: RectF,
    pub viewport_scale: f64,
    pub viewport_origin_screen: (f64, f64),
}

impl LongCaptureRequest {
    #[must_use]
    pub fn selection_rectf(&self) -> RectF {
        RectF::new(
            f64::from(self.selection_rect.x),
            f64::from(self.selection_rect.y),
            f64::from(self.selection_rect.width.max(0)),
            f64::from(self.selection_rect.height.max(0)),
        )
    }

    #[must_use]
    pub fn map_local_rect_to_screen(&self, rect: RectF) -> RectF {
        RectF::new(
            self.viewport_origin_screen.0 + rect.x,
            self.viewport_origin_screen.1 + rect.y,
            rect.width,
            rect.height,
        )
    }
}

pub fn open_window(cx: &mut App, request: LongCaptureRequest) {
    let layout = compute_window_layout(&request, LongCaptureToolbarAction::ORDERED.len());
    let coordinator = Arc::new(LongCaptureCoordinator::new(request.clone()));

    if let Err(err) = cx.open_window(window_options(layout.frame_bounds, false), {
        let request = request.clone();
        let coordinator = coordinator.clone();
        move |window, cx| {
            appearance::apply_saved_preferences(Some(window), cx);
            shell::configure_window(window, cx, false);
            window.set_background_appearance(WindowBackgroundAppearance::Transparent);
            if let Err(err) = shell::set_always_on_top(window) {
                tracing::warn!("Failed to set frame window level: {err}");
            }
            if let Err(err) = shell::exclude_from_capture(window) {
                tracing::warn!("Failed to exclude frame window from capture: {err}");
            }

            let frame_click_through_ok = shell::set_click_through(window, true).is_ok();
            if !frame_click_through_ok {
                coordinator.on_frame_click_through_result(false);
                window.defer(cx, |window, _| {
                    window.remove_window();
                });
            } else {
                coordinator.register_window(LongCaptureWindowKind::Frame, window.window_handle());
            }

            cx.new(|cx| FrameWindowView::new(request, coordinator, window, cx))
        }
    }) {
        tracing::error!("Failed to open long capture frame window: {err}");
        coordinator.on_frame_click_through_result(false);
    }

    if let Err(err) = cx.open_window(window_options(layout.toolbar_bounds, true), {
        let coordinator = coordinator.clone();
        move |window, cx| {
            appearance::apply_saved_preferences(Some(window), cx);
            shell::configure_window(window, cx, true);
            window.set_background_appearance(WindowBackgroundAppearance::Transparent);
            if let Err(err) = shell::set_always_on_top(window) {
                tracing::warn!("Failed to set toolbar window level: {err}");
            }
            if let Err(err) = shell::exclude_from_capture(window) {
                tracing::warn!("Failed to exclude toolbar window from capture: {err}");
            }
            let focus_handle = cx.focus_handle();
            coordinator.register_window(LongCaptureWindowKind::Toolbar, window.window_handle());
            cx.new(|cx| ToolbarWindowView::new(coordinator, focus_handle, window, cx))
        }
    }) {
        tracing::error!("Failed to open long capture toolbar window: {err}");
        coordinator.cancel_capture();
        coordinator.close_windows_except(None, cx);
        return;
    }

    if let Err(err) = cx.open_window(window_options(layout.preview_bounds, false), move |window, cx| {
        appearance::apply_saved_preferences(Some(window), cx);
        shell::configure_window(window, cx, false);
        window.set_background_appearance(WindowBackgroundAppearance::Transparent);
        if let Err(err) = shell::set_always_on_top(window) {
            tracing::warn!("Failed to set preview window level: {err}");
        }
        if let Err(err) = shell::exclude_from_capture(window) {
            tracing::warn!("Failed to exclude preview window from capture: {err}");
        }
        coordinator.register_window(LongCaptureWindowKind::Preview, window.window_handle());
        cx.new(|cx| PreviewWindowView::new(coordinator, window, cx))
    }) {
        tracing::error!("Failed to open long capture preview window: {err}");
    }
}

fn window_options(bounds: gpui::Bounds<gpui::Pixels>, focus: bool) -> WindowOptions {
    shell::popup_window_options(PopupWindowSpec {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        kind: WindowKind::PopUp,
        focus,
        show: true,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id: None,
        window_min_size: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_maps_local_rect_to_screen_coordinates() {
        let request = LongCaptureRequest {
            selection_rect: Rect::new(10, 20, 100, 120),
            viewport_rect: RectF::new(0.0, 0.0, 1200.0, 800.0),
            viewport_scale: 1.0,
            viewport_origin_screen: (320.0, -80.0),
        };

        let local = RectF::new(50.0, 70.0, 200.0, 100.0);
        let mapped = request.map_local_rect_to_screen(local);

        assert_eq!(mapped, RectF::new(370.0, -10.0, 200.0, 100.0));
    }
}
