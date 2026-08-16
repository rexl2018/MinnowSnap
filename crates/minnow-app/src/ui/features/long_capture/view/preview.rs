use crate::services::i18n;
use crate::ui::features::long_capture::coordinator::LongCaptureCoordinator;
use gpui::{Context, InteractiveElement, IntoElement, ObjectFit, ParentElement, Render, RenderImage, Styled, Window, canvas, div, px};
use gpui_component::ActiveTheme as _;
use std::sync::Arc;

pub(crate) struct PreviewWindowView {
    coordinator: Arc<LongCaptureCoordinator>,
    uploaded: Option<Arc<RenderImage>>,
}

impl PreviewWindowView {
    pub(crate) fn new(coordinator: Arc<LongCaptureCoordinator>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        coordinator.ensure_runtime_poller(window, cx);
        Self { coordinator, uploaded: None }
    }

    fn sync_uploaded_image(&mut self, next: Option<Arc<RenderImage>>, window: &mut Window) -> Option<Arc<RenderImage>> {
        let next_id = next.as_ref().map(|image| image.id);
        let uploaded_id = self.uploaded.as_ref().map(|image| image.id);
        if next_id != uploaded_id {
            if let Some(previous) = self.uploaded.take() {
                let _ = window.drop_image(previous);
            }
            self.uploaded = next;
        }
        self.uploaded.clone()
    }
}

impl Render for PreviewWindowView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.coordinator.snapshot();
        let theme = cx.theme();
        let preview_image = self.sync_uploaded_image(snapshot.preview_image, window);

        let mut panel = div()
            .id("long-capture-preview")
            .size_full()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .overflow_hidden();

        if theme.shadow {
            panel = panel.shadow_lg();
        }

        panel = if let Some(image) = preview_image {
            // Paint into the panel bounds ourselves. `img()` injects the bitmap's
            // aspect ratio into layout, which makes a growing stitch taller than
            // this window; overflow then clips to the unchanging page top.
            panel.child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let paint_bounds = ObjectFit::Contain.get_bounds(bounds, image.size(0));
                        let _ = window.paint_image(paint_bounds, Default::default(), image, 0, false);
                    },
                )
                .size_full(),
            )
        } else {
            panel.child(
                div()
                    .flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .text_color(theme.muted_foreground)
                    .child(i18n::overlay::long_capture_scroll_hint()),
            )
        };

        div().size_full().bg(gpui::transparent_black()).child(
            panel.child(
                div()
                    .absolute()
                    .right_2()
                    .bottom_2()
                    .px_2()
                    .py_0p5()
                    .rounded(theme.radius_lg)
                    .bg(theme.primary)
                    .text_color(theme.primary_foreground)
                    .text_size(px(12.0))
                    .child(format!("{} px", snapshot.preview_height_px.max(0))),
            ),
        )
    }
}
