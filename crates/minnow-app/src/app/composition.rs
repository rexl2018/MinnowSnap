use gpui::{App, Application};
use tokio::sync::broadcast;
use tracing::info;

use super::workflows;
#[cfg(target_os = "windows")]
use crate::platform::notify::init_windows_notification_app_id;
use crate::platform::{self, hotkey::HotkeyActionSink, shutdown, system::install_ui_system_actions, tray::TrayActions};
use crate::services::{assets::AppAssets, settings};
use crate::ui::{
    features::{overlay, pin, preferences},
    support::{appearance, locale},
};

#[cfg(target_os = "macos")]
use crate::services::app_meta::APP_ID;

pub(super) fn run_application(set_auto_start: fn(bool), _hide_dock_icon: fn()) {
    #[cfg(target_os = "windows")]
    init_windows_notification_app_id();

    #[cfg(target_os = "macos")]
    if let Err(err) = notify_rust::set_application(APP_ID) {
        tracing::error!("Failed to set application: {err}");
    }

    let app = Application::new().with_assets(AppAssets);

    app.run(move |cx| {
        // GPUI installs an NSApplication subclass with its own `platform`
        // ivar. Calling `sharedApplication` before `Application::new()` would
        // create the standard NSApplication class and make GPUI panic.
        #[cfg(target_os = "macos")]
        _hide_dock_icon();

        install_shutdown_listener(cx);

        let locale_choice = settings::language();
        locale::apply(&locale_choice);
        gpui_component::init(cx);
        appearance::apply_saved_preferences(None, cx);
        install_ui_system_actions(cx, set_auto_start);
        overlay::bind_keys(cx);
        pin::bind_keys(cx);
        pin::install(cx);
        set_auto_start(settings::auto_start_enabled());
        platform::hotkey::install_hotkey_service(
            cx,
            HotkeyActionSink::new(open_capture_overlay, workflows::run_quick_capture_with_notification),
        );
        let overlay_handle = overlay::OverlayHandle::new(cx);
        cx.set_global(overlay_handle);

        if let Err(err) = platform::tray::SystemTray::install(
            cx,
            TrayActions::new(
                open_capture_overlay,
                workflows::run_quick_capture_with_notification,
                open_preferences_window,
            ),
        ) {
            tracing::error!("Failed to install system tray: {err}");
            cx.quit();
            return;
        }

        if let Err(err) = platform::background_host::install(cx) {
            tracing::error!("Failed to install background host window: {err}");
            cx.quit();
        }
    });
}

fn install_shutdown_listener(cx: &mut App) {
    let Some(mut shutdown_rx) = shutdown::subscribe() else {
        tracing::warn!("Shutdown control plane is not initialized; skip shutdown listener.");
        return;
    };
    let Some(shutdown_token) = shutdown::cancellation_token() else {
        tracing::warn!("Shutdown cancellation token is unavailable; skip shutdown listener.");
        return;
    };

    cx.spawn(async move |cx| {
        tokio::select! {
            _ = shutdown_token.cancelled() => {
                request_app_quit(cx);
            }
            trigger = shutdown_rx.recv() => {
                match trigger {
                    Ok(trigger) => {
                        info!("Applying shutdown trigger: {trigger:?}");
                        request_app_quit(cx);
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        request_app_quit(cx);
                    }
                    Err(broadcast::error::RecvError::Closed) => {}
                }
            }
        }
    })
    .detach();
}

fn request_app_quit(cx: &mut gpui::AsyncApp) {
    let _ = cx.update(|app| {
        app.quit();
    });
}

fn prepare_overlay_session(cx: &mut gpui::App) {
    let overlay_handle = cx.global::<overlay::OverlayHandle>().clone();
    overlay_handle.prepare(cx);
}

fn open_capture_overlay(cx: &mut gpui::App) {
    if !workflows::ensure_screen_recording_access() {
        return;
    }
    prepare_overlay_session(cx);
    overlay::open_window(cx);
}

fn open_preferences_window(cx: &mut gpui::App) {
    preferences::open_window(cx);
}
