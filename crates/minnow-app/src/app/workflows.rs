use crate::platform::{screen_recording, shell};
use crate::services::capture::{
    action::{ActionContext, ActionResult, CaptureAction, CaptureActionPlan, PinCaptureRequest},
    service::CaptureService,
};
use crate::services::geometry::Rect;
use crate::services::i18n;
use tracing::{error, info, warn};

/// Ensures the process can actually capture other windows before starting a
/// capture flow.
///
/// On macOS a missing Screen Recording permission makes the capture APIs
/// return only the desktop wallpaper instead of failing. This requests access
/// (prompting the user once), and if it is still denied, opens the relevant
/// system settings pane and warns the user rather than letting them capture an
/// empty desktop. Returns `true` when capture may proceed.
pub(crate) fn ensure_screen_recording_access() -> bool {
    if screen_recording::has_access() || screen_recording::request_access() {
        return true;
    }

    warn!("Screen Recording permission denied; capture would only see the desktop");
    screen_recording::open_settings();
    shell::show_notification(
        i18n::capture::permission_title().as_str(),
        i18n::capture::permission_denied().as_str(),
        shell::NotificationType::Info,
    );
    false
}

/// Executes the platform effects for a domain capture plan.
///
/// The capture service never reaches into clipboard, storage, or notification
/// adapters. This workflow is the single owner of that translation.
pub(crate) fn execute_capture_action(action: CaptureAction, context: ActionContext) -> ActionResult {
    let plan = action.plan(context);
    match plan {
        CaptureActionPlan::CopyImage(image) => {
            if shell::copy_image_to_clipboard(&image) {
                ActionResult::Copied
            } else {
                ActionResult::Error(i18n::capture::copy_failed())
            }
        }
        CaptureActionPlan::SaveImage { image, save_path_override } => match shell::save_image_to_user_dir(&image, save_path_override) {
            Ok(path) => {
                shell::play_shutter();
                ActionResult::Saved(path)
            }
            Err(error) => ActionResult::Error(error),
        },
        CaptureActionPlan::Pin {
            image,
            source_bounds,
            auto_ocr,
        } => {
            let Some(image_path) = shell::save_temp_image(&image) else {
                return ActionResult::Error(i18n::capture::pin_failed());
            };

            ActionResult::PinRequested(PinCaptureRequest {
                image_path,
                source_bounds,
                auto_ocr,
                ocr_image: image,
            })
        }
        CaptureActionPlan::Text(content) => ActionResult::OcrResult(content),
        CaptureActionPlan::ColorPicked(color) => ActionResult::ColorPicked(color),
        CaptureActionPlan::NoOp => ActionResult::NoOp,
        CaptureActionPlan::Error(error) => ActionResult::Error(error),
    }
}

/// Runs the tray/global-hotkey quick capture path and owns its user feedback.
pub(crate) fn run_quick_capture_with_notification() {
    info!("Starting quick capture workflow");
    if !ensure_screen_recording_access() {
        return;
    }
    let copied = CaptureService::capture_region(Rect::empty()).is_some_and(|image| {
        let copied = shell::copy_image_to_clipboard(&image);
        if copied {
            shell::play_shutter();
        }
        copied
    });

    if copied {
        info!("Quick capture image copied to clipboard");
        shell::show_notification(
            i18n::app::capture_name().as_str(),
            i18n::notify::quick_capture_copied().as_str(),
            shell::NotificationType::Copy,
        );
    } else {
        error!("Quick capture workflow failed");
        shell::show_notification(
            i18n::app::name().as_str(),
            i18n::notify::quick_capture_failed().as_str(),
            shell::NotificationType::Info,
        );
    }
}
