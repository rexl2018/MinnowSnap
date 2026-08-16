use super::super::LongCaptureRequest;
use crate::ui::features::long_capture::coordinator::LongCaptureCoordinator;
use crate::ui::features::long_capture::layout::TOOLBAR_TOP_RESERVED;
use gpui::{Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, div, px};
use gpui_component::ActiveTheme as _;
use std::sync::Arc;

pub(crate) struct FrameWindowView {
    request: LongCaptureRequest,
    coordinator: Arc<LongCaptureCoordinator>,
}

impl FrameWindowView {
    pub(crate) fn new(request: LongCaptureRequest, coordinator: Arc<LongCaptureCoordinator>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        coordinator.ensure_runtime_poller(window, cx);
        Self { request, coordinator }
    }
}

impl Render for FrameWindowView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.coordinator.snapshot();
        let selection = self.request.selection_rectf();
        let theme = cx.theme();
        let warning_color = if snapshot.warning_text.is_empty() {
            theme.selection
        } else {
            theme.danger
        };

        let mut root = div().id("long-capture-frame").size_full().bg(gpui::transparent_black());

        if snapshot.frame_visible {
            // Draw the border just OUTSIDE the selection rectangle. The capture
            // loop grabs exactly the selection region via CGWindowListCreateImage,
            // which does not honor NSWindowSharingType exclusion — so any border
            // pixels inside the selection would be baked into the stitched image.
            // Offsetting the box out by the border width keeps the 2px stroke
            // entirely outside the captured pixels while staying visible.
            const BORDER: f32 = 2.0;
            root = root.child(
                div()
                    .absolute()
                    .left(px(selection.x as f32 - BORDER))
                    .top(px(selection.y as f32 - BORDER))
                    .w(px(selection.width as f32 + BORDER * 2.0))
                    .h(px(selection.height as f32 + BORDER * 2.0))
                    .rounded(theme.radius_lg)
                    .border_2()
                    .border_color(warning_color),
            );
        }

        if !snapshot.warning_text.is_empty() {
            root = root.child(
                div()
                    .absolute()
                    .left(px(selection.x as f32))
                    .top(px((selection.y - TOOLBAR_TOP_RESERVED).max(12.0) as f32))
                    .rounded(theme.radius_lg)
                    .bg(theme.danger)
                    .px_3()
                    .py_1()
                    .text_color(theme.danger_foreground)
                    .child(snapshot.warning_text),
            );
        }

        root
    }
}
