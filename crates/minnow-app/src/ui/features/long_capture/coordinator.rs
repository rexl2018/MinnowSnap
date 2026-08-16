use super::LongCaptureRequest;
use super::layout::frame_visibility_after_click_through;
use crate::services::capture::long_capture::{LongCaptureEvent, LongCaptureRuntime};
use crate::ui::support::render_image;
use gpui::{AnyWindowHandle, AppContext, AsyncWindowContext, Context, RenderImage, WeakEntity, Window, WindowId};
use image::RgbaImage;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

const COORDINATOR_POLL_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LongCaptureWindowKind {
    Frame,
    Toolbar,
    Preview,
}

#[derive(Clone)]
pub(crate) struct LongCaptureSnapshot {
    pub(crate) preview_image: Option<Arc<RenderImage>>,
    pub(crate) preview_height_px: i32,
    pub(crate) warning_text: String,
    pub(crate) busy: bool,
    pub(crate) frame_visible: bool,
}

impl Default for LongCaptureSnapshot {
    fn default() -> Self {
        Self {
            preview_image: None,
            preview_height_px: 0,
            warning_text: String::new(),
            busy: false,
            frame_visible: true,
        }
    }
}

#[derive(Clone, Default)]
struct LongCaptureWindowHandles {
    frame: Option<AnyWindowHandle>,
    toolbar: Option<AnyWindowHandle>,
    preview: Option<AnyWindowHandle>,
}

#[derive(Default)]
struct LongCaptureCoordinatorState {
    snapshot: LongCaptureSnapshot,
    capture_image: Option<RgbaImage>,
    handles: LongCaptureWindowHandles,
    revision: u64,
    poller_running: bool,
}

impl LongCaptureCoordinatorState {
    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    fn apply_runtime_events(&mut self, events: Vec<LongCaptureEvent>, final_image: Option<RgbaImage>) {
        let mut changed = false;
        let mut final_image = final_image;

        for event in events {
            match event {
                LongCaptureEvent::Started => {
                    changed = true;
                }
                LongCaptureEvent::Progress { height, preview_image } => {
                    self.snapshot.preview_height_px = height;
                    self.snapshot.preview_image = Some(render_image::from_rgba(preview_image));
                    changed = true;
                }
                LongCaptureEvent::Warning { text } => {
                    self.snapshot.warning_text = text;
                    changed = true;
                }
                LongCaptureEvent::Finished => {
                    if let Some(image) = final_image.take() {
                        self.capture_image = Some(image);
                    }
                    self.snapshot.busy = false;
                    changed = true;
                }
            }
        }

        if changed {
            self.bump_revision();
        }
    }

    fn snapshot(&self) -> LongCaptureSnapshot {
        self.snapshot.clone()
    }

    fn has_registered_windows(&self) -> bool {
        self.handles.frame.is_some() || self.handles.toolbar.is_some() || self.handles.preview.is_some()
    }

    fn clear_dead_windows(&mut self, frame_alive: bool, toolbar_alive: bool, preview_alive: bool) {
        if !frame_alive {
            self.handles.frame = None;
        }
        if !toolbar_alive {
            self.handles.toolbar = None;
        }
        if !preview_alive {
            self.handles.preview = None;
        }
    }

    fn register_window(&mut self, kind: LongCaptureWindowKind, handle: AnyWindowHandle) {
        match kind {
            LongCaptureWindowKind::Frame => self.handles.frame = Some(handle),
            LongCaptureWindowKind::Toolbar => self.handles.toolbar = Some(handle),
            LongCaptureWindowKind::Preview => self.handles.preview = Some(handle),
        }
        self.bump_revision();
    }

    fn set_frame_visibility(&mut self, success: bool) {
        self.snapshot.frame_visible = frame_visibility_after_click_through(success);
        self.bump_revision();
    }

    fn start_capture_action(&mut self) {
        self.snapshot.busy = true;
        self.snapshot.warning_text.clear();
        self.bump_revision();
    }

    fn finish_capture_action_with_warning(&mut self, warning_text: String) {
        self.snapshot.busy = false;
        self.snapshot.warning_text = warning_text;
        self.bump_revision();
    }

    fn take_capture_image(&mut self) -> Option<RgbaImage> {
        let image = self.capture_image.take();
        if image.is_some() {
            self.bump_revision();
        }
        image
    }

    fn start_poller(&mut self) -> bool {
        if self.poller_running {
            return false;
        }
        self.poller_running = true;
        true
    }

    fn stop_poller(&mut self) {
        self.poller_running = false;
    }

    fn retain_windows_except(&mut self, except: Option<WindowId>) {
        self.handles.frame = self
            .handles
            .frame
            .take()
            .filter(|handle| except.is_some_and(|id| id == handle.window_id()));
        self.handles.toolbar = self
            .handles
            .toolbar
            .take()
            .filter(|handle| except.is_some_and(|id| id == handle.window_id()));
        self.handles.preview = self
            .handles
            .preview
            .take()
            .filter(|handle| except.is_some_and(|id| id == handle.window_id()));
    }
}

pub(crate) struct LongCaptureCoordinator {
    runtime: LongCaptureRuntime,
    state: Mutex<LongCaptureCoordinatorState>,
}

impl LongCaptureCoordinator {
    pub(crate) fn new(request: LongCaptureRequest) -> Self {
        let runtime = LongCaptureRuntime::new();
        runtime.start_with_viewport(
            request.selection_rect,
            crate::services::geometry::RectF::new(
                request.viewport_rect.x,
                request.viewport_rect.y,
                request.viewport_rect.width,
                request.viewport_rect.height,
            ),
            request.viewport_scale as f32,
        );

        Self {
            runtime,
            state: Mutex::new(LongCaptureCoordinatorState {
                revision: 1,
                ..LongCaptureCoordinatorState::default()
            }),
        }
    }

    fn revision(&self) -> u64 {
        self.state_guard().revision
    }

    fn state_guard(&self) -> MutexGuard<'_, LongCaptureCoordinatorState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::error!("Long-capture coordinator state lock was poisoned; recovering state");
                let guard = poisoned.into_inner();
                self.state.clear_poison();
                guard
            }
        }
    }

    fn poll_runtime_events(&self) -> u64 {
        let events = self.runtime.drain_events();
        if events.is_empty() {
            return self.revision();
        }

        let final_image = events
            .iter()
            .any(|event| matches!(event, LongCaptureEvent::Finished))
            .then(|| self.runtime.take_result())
            .flatten();
        let mut state = self.state_guard();
        state.apply_runtime_events(events, final_image);
        state.revision
    }

    pub(crate) fn snapshot(&self) -> LongCaptureSnapshot {
        self.state_guard().snapshot()
    }

    fn has_registered_windows(&self) -> bool {
        self.state_guard().has_registered_windows()
    }

    fn refresh_window<C: AppContext>(handle: Option<AnyWindowHandle>, cx: &mut C) -> bool {
        handle.is_some_and(|handle| {
            handle
                .update(cx, |view, window, cx| {
                    window.refresh();
                    cx.notify(view.entity_id());
                })
                .is_ok()
        })
    }

    fn notify_registered_windows<C: AppContext>(&self, cx: &mut C) -> bool {
        let handles = self.state_guard().handles.clone();

        let frame_alive = Self::refresh_window(handles.frame, cx);
        let toolbar_alive = Self::refresh_window(handles.toolbar, cx);
        let preview_alive = Self::refresh_window(handles.preview, cx);

        self.state_guard().clear_dead_windows(frame_alive, toolbar_alive, preview_alive);

        frame_alive || toolbar_alive || preview_alive
    }

    fn mark_poller_stopped(&self) {
        self.state_guard().stop_poller();
    }

    pub(crate) fn ensure_runtime_poller<V>(self: &Arc<Self>, window: &mut Window, cx: &mut Context<V>)
    where
        V: 'static,
    {
        let should_spawn = self.state_guard().start_poller();

        if !should_spawn {
            return;
        }

        let coordinator = self.clone();
        cx.spawn_in(window, move |_this: WeakEntity<V>, cx: &mut AsyncWindowContext| {
            let mut cx = cx.clone();
            async move {
                let mut revision = coordinator.revision();
                loop {
                    cx.background_executor().timer(COORDINATOR_POLL_INTERVAL).await;
                    let next_revision = coordinator.poll_runtime_events();
                    if next_revision != revision {
                        revision = next_revision;
                        if !coordinator.notify_registered_windows(&mut cx) {
                            break;
                        }
                    } else if !coordinator.has_registered_windows() {
                        break;
                    }
                }
                coordinator.mark_poller_stopped();
            }
        })
        .detach();
    }

    pub(crate) fn register_window(&self, kind: LongCaptureWindowKind, handle: AnyWindowHandle) {
        self.state_guard().register_window(kind, handle);
    }

    pub(crate) fn on_frame_click_through_result(&self, success: bool) {
        self.state_guard().set_frame_visibility(success);
    }

    pub(crate) fn start_capture_action(&self) {
        self.state_guard().start_capture_action();
    }

    pub(crate) fn finish_capture_action_with_warning(&self, warning_text: String) {
        self.state_guard().finish_capture_action_with_warning(warning_text);
    }

    pub(crate) fn cancel_capture(&self) {
        self.runtime.stop();
    }

    pub(crate) fn take_capture_image(&self, timeout: Duration) -> Option<RgbaImage> {
        if let Some(image) = self.state_guard().take_capture_image() {
            return Some(image);
        }

        self.runtime.stop_and_take_result(timeout)
    }

    pub(crate) fn close_windows_except<C: AppContext>(&self, except: Option<WindowId>, cx: &mut C) {
        let handles = self.state_guard().handles.clone();

        for handle in [handles.frame, handles.toolbar, handles.preview].into_iter().flatten() {
            if except.is_some_and(|id| id == handle.window_id()) {
                continue;
            }
            let _ = handle.update(cx, |_, window, _| {
                window.remove_window();
            });
        }

        self.state_guard().retain_windows_except(except);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn runtime_event_batch_updates_state_with_one_revision() {
        let mut state = LongCaptureCoordinatorState::default();
        state.snapshot.busy = true;
        let final_image = RgbaImage::from_pixel(4, 3, Rgba([10, 20, 30, 255]));

        state.apply_runtime_events(
            vec![
                LongCaptureEvent::Started,
                LongCaptureEvent::Warning {
                    text: "unstable".to_string(),
                },
                LongCaptureEvent::Finished,
            ],
            Some(final_image),
        );

        assert_eq!(state.revision, 1);
        assert!(!state.snapshot.busy);
        assert_eq!(state.snapshot.warning_text, "unstable");
        assert_eq!(state.capture_image.as_ref().map(RgbaImage::dimensions), Some((4, 3)));
    }

    #[test]
    fn empty_runtime_event_batch_does_not_change_revision() {
        let mut state = LongCaptureCoordinatorState::default();

        state.apply_runtime_events(Vec::new(), None);

        assert_eq!(state.revision, 0);
    }

    #[test]
    fn capture_action_transitions_own_busy_warning_and_revision() {
        let mut state = LongCaptureCoordinatorState::default();
        state.snapshot.warning_text = "previous warning".to_string();

        state.start_capture_action();
        assert!(state.snapshot.busy);
        assert!(state.snapshot.warning_text.is_empty());
        assert_eq!(state.revision, 1);

        state.finish_capture_action_with_warning("save failed".to_string());
        assert!(!state.snapshot.busy);
        assert_eq!(state.snapshot.warning_text, "save failed");
        assert_eq!(state.revision, 2);
    }

    #[test]
    fn coordinator_recovers_and_clears_a_poisoned_state_lock() {
        let coordinator = LongCaptureCoordinator {
            runtime: LongCaptureRuntime::new(),
            state: Mutex::new(LongCaptureCoordinatorState::default()),
        };

        let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _state = coordinator.state.lock().expect("lock should start healthy");
            panic!("poison coordinator state for recovery test");
        }));

        assert!(panic_result.is_err());
        assert!(coordinator.state.is_poisoned());
        let _snapshot = coordinator.snapshot();
        assert!(!coordinator.state.is_poisoned());
    }
}
