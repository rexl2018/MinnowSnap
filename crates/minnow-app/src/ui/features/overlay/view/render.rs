use super::{OverlayView, SelectionHudVisibility, should_show_property_panel};
use crate::services::geometry::RectF;
use crate::ui::features::overlay::actions::OVERLAY_CONTEXT;
use crate::ui::features::overlay::render::OverlayActionHandler;
use crate::ui::features::overlay::render::annotation::overlay_annotations_layer;
#[cfg(feature = "overlay-diagnostics")]
use crate::ui::features::overlay::render::diagnostics::overlay_diagnostics_hud;
use crate::ui::features::overlay::render::hud::{resolution_tooltip, window_info_tooltip, window_info_tooltip_content};
use crate::ui::features::overlay::render::layout::{
    resolve_info_reserved_slot_layout, resolve_info_tooltip_layout, resolve_property_layout, resolve_resolution_tooltip_layout_with_occupied,
    resolve_toolbar_layout,
};
use crate::ui::features::overlay::render::picker::overlay_picker;
use crate::ui::features::overlay::render::properties::{OverlayPropertyState, overlay_properties_panel};
use crate::ui::features::overlay::render::selection::{overlay_mask, selection_frame, selection_handles};
use crate::ui::features::overlay::render::toolbar::{OverlayToolbarState, overlay_toolbar, toolbar_button_count};
use crate::ui::features::overlay::state::{AnnotationKindTag, OverlayCommand, OverlayFrame, PickerVm};
use crate::ui::features::overlay::window_catalog::WindowInfo;
use gpui::InteractiveElement;
use gpui::{Context, Div, Entity, IntoElement, MouseButton, ObjectFit, ParentElement, Stateful, Styled, StyledImage, Window, div, img};
use gpui_component::{ActiveTheme, color_picker::ColorPickerState};
use std::rc::Rc;

impl OverlayView {
    fn background_layer(background_image: Option<std::sync::Arc<gpui::RenderImage>>, background: gpui::Hsla) -> Div {
        if let Some(image) = background_image {
            Self::overlay_layer().child(img(image).size_full().object_fit(ObjectFit::Fill))
        } else {
            Self::overlay_layer().bg(background)
        }
    }

    fn overlay_action_handler(&self) -> OverlayActionHandler {
        let handle = self.handle.clone();
        Rc::new(move |command: OverlayCommand, window: &mut Window, app: &mut gpui::App| {
            handle.dispatch(command, window, app);
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn render_selection_layer(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        frame: &OverlayFrame,
        viewport_w: f64,
        viewport_h: f64,
        color_picker_state: &Entity<ColorPickerState>,
        recent_custom_colors: &[u32],
    ) -> Div {
        let Some(selection) = frame.selection.selection else {
            return Self::overlay_layer();
        };
        let hud_visibility = SelectionHudVisibility::from_selection_state(frame.selection.drag_mode, frame.selection_move_delta.is_some());
        let selected_is_text = frame
            .annotation
            .selected
            .as_ref()
            .is_some_and(|item| item.kind == AnnotationKindTag::Text);
        let selected_is_mosaic = frame
            .annotation
            .selected
            .as_ref()
            .is_some_and(|item| item.kind == AnnotationKindTag::Mosaic);
        let show_property_panel = should_show_property_panel(frame.annotation.tool, frame.annotation.selected.map(|item| item.kind));

        let toolbar_layout = hud_visibility
            .show_toolbar
            .then(|| resolve_toolbar_layout(selection, toolbar_button_count(), viewport_w, viewport_h, &[]));
        let property_layout = if hud_visibility.show_toolbar && show_property_panel {
            toolbar_layout.map(|toolbar| {
                let reserved_info_slot = resolve_info_reserved_slot_layout(selection, viewport_w, viewport_h);
                let occupied = [toolbar];
                resolve_property_layout(
                    toolbar,
                    reserved_info_slot,
                    selected_is_text,
                    selected_is_mosaic,
                    viewport_w,
                    viewport_h,
                    &occupied,
                )
            })
        } else {
            None
        };
        let resolution_layout = hud_visibility.show_resolution_tooltip.then(|| {
            let mut occupied = Vec::new();
            if let Some(layout) = toolbar_layout {
                occupied.push(layout);
            }
            if let Some(layout) = property_layout {
                occupied.push(layout);
            }
            resolve_resolution_tooltip_layout_with_occupied(selection, viewport_w, viewport_h, &occupied)
        });

        let mut layer = Self::overlay_layer()
            .child(overlay_annotations_layer(cx, selection, &frame.annotation.layer))
            .child(selection_frame(cx, selection, hud_visibility.show_handles));
        if hud_visibility.show_handles {
            layer = layer.child(selection_handles(cx, selection));
        }
        if let Some(layout) = resolution_layout {
            layer = layer.child(resolution_tooltip(cx, selection, layout));
        }
        if let Some(layout) = toolbar_layout {
            let on_action = self.overlay_action_handler();
            layer = layer.child(overlay_toolbar(
                window,
                cx,
                layout,
                OverlayToolbarState {
                    tool: frame.annotation.tool,
                    can_undo: frame.annotation.can_undo,
                    can_redo: frame.annotation.can_redo,
                },
                on_action.clone(),
            ));
            if let Some(property_layout) = property_layout {
                layer = layer.child(overlay_properties_panel(
                    cx,
                    property_layout,
                    OverlayPropertyState {
                        style: frame.annotation.style,
                        active_tool: frame.annotation.tool,
                        selected_annotation: frame.annotation.selected,
                        text_editing: frame.annotation.text_editing,
                        recent_custom_colors: recent_custom_colors.to_vec(),
                    },
                    color_picker_state,
                    on_action,
                ));
            }
        }
        layer
    }

    fn render_hover_layer(cx: &mut Context<Self>, target: RectF, hovered_window: Option<WindowInfo>, viewport_w: f64, viewport_h: f64) -> Div {
        let mut layer = Self::overlay_layer().child(selection_frame(cx, target, false));
        if let Some(window_info) = hovered_window.as_ref()
            && let Some(content) = window_info_tooltip_content(window_info)
        {
            let info_layout = resolve_info_tooltip_layout(target, content.height, viewport_w, viewport_h);
            layer = layer.child(window_info_tooltip(cx, &content, info_layout));
        }
        layer
    }

    fn render_picker_layer(cx: &mut Context<Self>, picker: &PickerVm, viewport_w: f64, viewport_h: f64) -> Div {
        Self::overlay_layer().child(overlay_picker(
            cx,
            picker.cursor,
            picker.sample.as_ref(),
            picker.neighborhood.as_ref(),
            picker.format,
            viewport_w,
            viewport_h,
        ))
    }

    fn bind_root_interactions(&self, root: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        root.key_context(OVERLAY_CONTEXT)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(Self::on_action_copy_selection))
            .on_action(cx.listener(Self::on_action_save_selection))
            .on_action(cx.listener(Self::on_action_pin_selection))
            .on_action(cx.listener(Self::on_action_qr_selection))
            .on_action(cx.listener(Self::on_action_pick_color_selection))
            .on_action(cx.listener(Self::on_action_copy_pixel_color))
            .on_action(cx.listener(Self::on_action_cycle_picker_format))
            .on_action(cx.listener(Self::on_action_move_picker_up))
            .on_action(cx.listener(Self::on_action_move_picker_down))
            .on_action(cx.listener(Self::on_action_move_picker_left))
            .on_action(cx.listener(Self::on_action_move_picker_right))
            .on_action(cx.listener(Self::on_action_select_arrow_tool))
            .on_action(cx.listener(Self::on_action_select_rectangle_tool))
            .on_action(cx.listener(Self::on_action_select_circle_tool))
            .on_action(cx.listener(Self::on_action_select_counter_tool))
            .on_action(cx.listener(Self::on_action_select_text_tool))
            .on_action(cx.listener(Self::on_action_select_mosaic_tool))
            .on_action(cx.listener(Self::on_action_undo_annotation))
            .on_action(cx.listener(Self::on_action_redo_annotation))
            .on_action(cx.listener(Self::on_action_delete_annotation))
            .on_action(cx.listener(Self::on_action_cycle_annotation_color))
            .on_action(cx.listener(Self::on_action_toggle_annotation_fill))
            .on_action(cx.listener(Self::on_action_increase_annotation_stroke))
            .on_action(cx.listener(Self::on_action_decrease_annotation_stroke))
            .on_action(cx.listener(Self::on_action_start_text_edit))
            .on_action(cx.listener(Self::on_action_reset_selection))
            .on_action(cx.listener(Self::on_action_close_overlay))
    }
}

impl gpui::Render for OverlayView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let picker_state = self.ensure_color_picker_state(window, cx);
        self.apply_picker_color_if_changed(window, cx);

        let frame = self.handle.prepare_frame(window, cx);
        self.sync_picker_with_style(frame.annotation.style.stroke_color, window, cx);

        let viewport = window.viewport_size();
        let viewport_w = viewport.width.to_f64();
        let viewport_h = viewport.height.to_f64();
        let active_rect = frame.selection.selection.or(frame.selection.target);

        let background = cx.theme().background;
        let mut root = self.bind_root_interactions(div().id("overlay-root").size_full(), cx);
        root = root.track_focus(&self.focus_handle);

        root = root.child(Self::background_layer(frame.background_image.clone(), background));
        root = root.child(overlay_mask(cx, active_rect, viewport_w, viewport_h));

        if frame.selection.selection.is_some() {
            root = root.child(self.render_selection_layer(window, cx, &frame, viewport_w, viewport_h, &picker_state, &self.recent_custom_colors));
        } else if let Some(target) = frame.selection.target {
            root = root.child(Self::render_hover_layer(
                cx,
                target,
                frame.hud.hovered_window.clone(),
                viewport_w,
                viewport_h,
            ));
        }

        if let Some(picker) = frame.picker.as_ref() {
            root = root.child(Self::render_picker_layer(cx, picker, viewport_w, viewport_h));
        }

        #[cfg(feature = "overlay-diagnostics")]
        {
            root = root.child(overlay_diagnostics_hud(cx, &frame.diagnostics, viewport_w, viewport_h));
        }

        root
    }
}
