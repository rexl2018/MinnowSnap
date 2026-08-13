use super::components::window_brand;
use crate::platform::shell::{PopupDragBehavior, PopupDragRegionExt};
use gpui::{AnyElement, App, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div, px};
use gpui_component::{ActiveTheme as _, scroll::ScrollableElement, v_flex};

pub(crate) const SIDEBAR_WIDTH: f32 = 208.0;
pub(crate) const TITLEBAR_HEIGHT: f32 = 48.0;

pub(crate) fn title_bar(page_title: impl Into<SharedString>, close_button: impl IntoElement, cx: &App) -> AnyElement {
    let theme = cx.theme();

    div()
        .h(px(TITLEBAR_HEIGHT))
        .flex()
        .items_center()
        .border_b_1()
        .border_color(theme.border.alpha(0.7))
        .child(
            div()
                .w(px(SIDEBAR_WIDTH))
                .min_w(px(SIDEBAR_WIDTH))
                .max_w(px(SIDEBAR_WIDTH))
                .h_full()
                .px_4()
                .border_r_1()
                .border_color(theme.border)
                .bg(theme.background)
                .popup_drag_region(PopupDragBehavior::SystemMove)
                .child(window_brand()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .flex()
                .items_center()
                .gap_3()
                .px_4()
                .bg(theme.popover)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .h_full()
                        .flex()
                        .items_center()
                        .overflow_hidden()
                        .line_clamp(1)
                        .text_ellipsis()
                        .popup_drag_region(PopupDragBehavior::SystemMove)
                        .child(page_title.into()),
                )
                .child(close_button),
        )
        .into_any_element()
}

pub(crate) fn sidebar_panel(items: Vec<AnyElement>, cx: &App) -> AnyElement {
    let theme = cx.theme();

    div()
        .w(px(SIDEBAR_WIDTH))
        .min_w(px(SIDEBAR_WIDTH))
        .max_w(px(SIDEBAR_WIDTH))
        .flex()
        .flex_col()
        .min_h(px(0.))
        .overflow_hidden()
        .border_r_1()
        .border_color(theme.border)
        .bg(theme.background)
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .min_h(px(0.))
                .overflow_y_scrollbar()
                .px_2()
                .py_3()
                .child(v_flex().w_full().gap_1().children(items)),
        )
        .into_any_element()
}

pub(crate) fn content_panel(notice: Option<AnyElement>, page_body: AnyElement, cx: &App) -> AnyElement {
    let theme = cx.theme();

    div()
        .flex_1()
        .w_full()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(theme.popover)
        .child(
            v_flex()
                .id("preferences-content")
                .flex_1()
                .w_full()
                .min_h(px(0.))
                .overflow_y_scrollbar()
                .child(
                    v_flex()
                        .w_full()
                        .px_6()
                        .py_5()
                        .gap_5()
                        .children(notice)
                        .child(page_body),
                ),
        )
        .into_any_element()
}
