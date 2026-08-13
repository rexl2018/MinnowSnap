use super::{SelectAction, ToggleAction};
use crate::services::assets::asset_paths;
use crate::services::i18n;
use crate::ui::features::preferences::{
    state::{
        PreferencesNotice,
        frame::{ActionRowProps, ButtonProps, SelectOption, SelectRowProps, SidebarItemProps, ToggleRowProps},
    },
    view::PreferencesView,
};
use gpui::{
    AnyElement, App, ClickEvent, Context, Div, InteractiveElement, IntoElement, ParentElement, SharedString, Stateful, Styled, Window, div, img,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable, Icon, IconNamed, Sizable, Size,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    switch::Switch,
    v_flex,
};

#[derive(Clone, Copy)]
pub(crate) enum PreferencesChromeIcon {
    Close,
}

impl IconNamed for PreferencesChromeIcon {
    fn path(self) -> SharedString {
        match self {
            Self::Close => asset_paths::icons::CLOSE.into(),
        }
    }
}

pub(crate) fn sidebar_item(item: &SidebarItemProps, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let hover_bg = theme.accent.alpha(0.12);
    let active_bg = theme.accent.alpha(0.16);
    let text_color = if item.is_active { theme.foreground } else { theme.muted_foreground };

    let mut element = div()
        .id(item.page.id())
        .w_full()
        .h(px(40.))
        .cursor_pointer()
        .rounded_lg()
        .px_2()
        .overflow_hidden()
        .child(
            div().flex().h_full().items_center().px_3().min_w(px(0.)).overflow_hidden().child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .line_clamp(1)
                    .text_ellipsis()
                    .text_color(text_color)
                    .child(item.title.clone()),
            ),
        );

    if item.is_active {
        element = element.bg(active_bg);
    } else {
        element = element.hover(move |style| style.bg(hover_bg));
    }

    element
}

pub(super) fn window_brand() -> AnyElement {
    div()
        .flex()
        .h_full()
        .items_center()
        .gap_3()
        .min_w(px(0.))
        .child(img(asset_paths::LOGO_PATH).w(px(20.)).h(px(20.)))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .line_clamp(1)
                .text_ellipsis()
                .child(i18n::preferences::title()),
        )
        .into_any_element()
}

pub(crate) fn close_button(on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static, cx: &App) -> AnyElement {
    let theme = cx.theme();

    Button::new("preferences-close")
        .ghost()
        .compact()
        .icon(Icon::new(PreferencesChromeIcon::Close).small().text_color(theme.muted_foreground))
        .tooltip(i18n::common::close())
        .on_click(on_click)
        .into_any_element()
}

pub(crate) fn notice_banner(notice: &PreferencesNotice, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let border = if notice.is_error() {
        theme.danger.alpha(0.35)
    } else {
        theme.primary.alpha(0.35)
    };
    let background = if notice.is_error() {
        theme.danger.alpha(0.1)
    } else {
        theme.primary.alpha(0.1)
    };
    let foreground = if notice.is_error() { theme.danger } else { theme.foreground };

    div()
        .w_full()
        .rounded_lg()
        .border_1()
        .border_color(border)
        .bg(background)
        .px_4()
        .py_3()
        .text_color(foreground)
        .child(notice.message.clone())
        .into_any_element()
}

pub(super) fn surface_card(content: impl IntoElement, cx: &App) -> AnyElement {
    let theme = cx.theme();

    div()
        .w_full()
        .rounded(theme.radius_lg)
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .overflow_hidden()
        .child(content)
        .into_any_element()
}

pub(super) fn setting_section(rows: impl IntoIterator<Item = AnyElement>, cx: &App) -> AnyElement {
    let mut body = v_flex().w_full();

    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            body = body.child(render_divider(cx));
        }
        body = body.child(div().w_full().px_4().py_3().child(row));
    }

    surface_card(body, cx)
}

pub(super) fn setting_toggle(props: &ToggleRowProps, on_toggle: ToggleAction, cx: &mut Context<PreferencesView>) -> AnyElement {
    setting_row(
        props.title.clone(),
        props.description.clone(),
        props.disabled,
        Switch::new(props.id)
            .checked(props.checked)
            .disabled(props.disabled)
            .on_click(cx.listener(move |this, checked: &bool, window, cx| {
                on_toggle(this, *checked, window, cx);
            })),
        cx,
    )
}

pub(super) fn setting_dropdown(props: &SelectRowProps, on_select: SelectAction, cx: &App) -> AnyElement {
    let current_label = SelectOption::label_for(props.current_value.as_ref(), &props.options);
    let current_value = props.current_value.clone();
    let options = props.options.clone();

    setting_row(
        props.title.clone(),
        props.description.clone(),
        props.disabled,
        Button::new(props.id)
            .label(current_label)
            .dropdown_caret(true)
            .outline()
            .with_size(Size::Small)
            .disabled(props.disabled)
            .dropdown_menu_with_anchor(gpui::Corner::TopRight, move |menu: PopupMenu, _, _| {
                let current_value = current_value.clone();

                options.iter().fold(menu, move |menu, option| {
                    let checked = current_value == option.value;
                    let value = option.value.clone();

                    menu.item(PopupMenuItem::new(option.label.clone()).checked(checked).on_click(move |_, window, cx| {
                        on_select(value.clone(), window, cx);
                    }))
                })
            }),
        cx,
    )
}

pub(super) fn setting_action(props: &ActionRowProps, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static, cx: &App) -> AnyElement {
    setting_row(
        props.title.clone(),
        props.description.clone(),
        props.disabled,
        secondary_button(&props.button(), on_click),
        cx,
    )
}

pub(super) fn secondary_button(props: &ButtonProps, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> AnyElement {
    Button::new(props.id)
        .label(props.label.clone())
        .outline()
        .with_size(Size::Small)
        .disabled(props.disabled)
        .on_click(on_click)
        .into_any_element()
}

fn setting_row(title: SharedString, description: SharedString, disabled: bool, control: impl IntoElement, cx: &App) -> AnyElement {
    div()
        .w_full()
        .when(disabled, |this| this.opacity(0.55))
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .child(
                    v_flex()
                        .flex_1()
                        .gap_1()
                        .child(title)
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(description),
                        ),
                )
                .child(div().flex_shrink_0().child(control)),
        )
        .into_any_element()
}

fn render_divider(cx: &App) -> impl IntoElement {
    div().h(px(1.)).w_full().bg(cx.theme().border.alpha(0.6))
}
