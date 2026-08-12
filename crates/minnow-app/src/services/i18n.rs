pub(crate) const SYSTEM_LOCALE: &str = "System";

pub(crate) fn init(language: &str) -> String {
    let locale = resolve_locale(language);
    rust_i18n::set_locale(&locale);
    locale
}

pub(crate) fn resolve_locale(language: &str) -> String {
    if language.trim().is_empty() || language == SYSTEM_LOCALE {
        return detect_system_locale();
    }

    normalize_locale_tag(language)
}

fn normalize_locale_tag(locale: &str) -> String {
    match locale.trim() {
        "" | "System" => detect_system_locale(),
        "zh_CN" | "zh-CN" | "zh" => "zh-CN".to_string(),
        "en_US" | "en-US" | "en" => "en".to_string(),
        other => other.replace('_', "-"),
    }
}

pub(crate) fn detect_system_locale() -> String {
    let system = sys_locale::get_locale().unwrap_or_else(|| "en-US".to_string());
    match system.to_ascii_lowercase().as_str() {
        value if value.starts_with("zh") => "zh-CN".to_string(),
        value if value.starts_with("en") => "en".to_string(),
        _ => "en".to_string(),
    }
}

macro_rules! i18n_fns {
    ($($fn_name:ident => $key:literal),* $(,)?) => {
        $(
            pub fn $fn_name() -> String {
                t!($key).into_owned()
            }
        )*
    };
}

macro_rules! i18n_fns_with_args {
    ($($fn_name:ident($($arg:ident),+) => $key:literal),* $(,)?) => {
        $(
            pub fn $fn_name($($arg: impl ToString),+) -> String {
                t!($key, $($arg = $arg.to_string()),+).into_owned()
            }
        )*
    };
}

pub mod app {
    use rust_i18n::t;

    i18n_fns! {
        name => "app.name",
        capture_name => "app.capture_name",
    }
}

pub mod common {
    use rust_i18n::t;

    i18n_fns! {
        close => "common.actions.close",
        cancel => "common.actions.cancel",
        copy => "common.actions.copy",
        save => "common.actions.save",
        pin => "common.actions.pin",
        ocr => "common.actions.ocr",
        scan_qr => "common.actions.scan_qr",
        scroll => "common.actions.scroll",
    }
}

pub mod overlay {
    use rust_i18n::t;

    i18n_fns! {
        action_unavailable => "overlay.status.action_unavailable",
        annotation_tool_arrow => "overlay.annotation.tool.arrow",
        annotation_tool_rectangle => "overlay.annotation.tool.rectangle",
        annotation_tool_circle => "overlay.annotation.tool.circle",
        annotation_tool_counter => "overlay.annotation.tool.counter",
        annotation_tool_text => "overlay.annotation.tool.text",
        annotation_tool_mosaic => "overlay.annotation.tool.mosaic",
        annotation_undo => "overlay.annotation.actions.undo",
        annotation_redo => "overlay.annotation.actions.redo",
        annotation_toggle_fill => "overlay.annotation.actions.toggle_fill",
        annotation_stroke_up => "overlay.annotation.actions.stroke_up",
        annotation_stroke_down => "overlay.annotation.actions.stroke_down",
        annotation_edit_text => "overlay.annotation.actions.edit_text",
        annotation_mosaic_mode_pixelate => "overlay.annotation.actions.mosaic_mode_pixelate",
        annotation_mosaic_mode_blur => "overlay.annotation.actions.mosaic_mode_blur",
        annotation_mosaic_intensity_up => "overlay.annotation.actions.mosaic_intensity_up",
        annotation_mosaic_intensity_down => "overlay.annotation.actions.mosaic_intensity_down",
        qr_not_found => "overlay.notify.qr_not_found",
        long_capture_processing => "overlay.long_capture.processing",
        long_capture_scroll_hint => "overlay.long_capture.scroll_hint",
        long_capture_empty => "overlay.long_capture.empty",
    }

    i18n_fns_with_args! {
        picker_value_and_format(value, format) => "overlay.picker.value_and_format",
        picker_coordinates(x, y) => "overlay.picker.coordinates",
        picker_shortcuts(copy_key, cycle_key) => "overlay.picker.shortcuts",
    }
}

pub mod notify {
    use rust_i18n::t;

    i18n_fns! {
        copied_image => "notify.capture.copied_image",
        copied_qr => "notify.capture.copied_qr",
        copied_text => "notify.capture.copied_text",
        quick_capture_copied => "notify.capture.quick_capture_copied",
        quick_capture_failed => "notify.capture.quick_capture_failed",
    }

    i18n_fns_with_args! {
        saved_image(path) => "notify.capture.saved_image",
    }
}

pub mod capture {
    use rust_i18n::t;

    i18n_fns! {
        copy_failed => "capture.errors.copy_failed",
        pin_failed => "capture.errors.pin_failed",
        permission_title => "capture.permission.title",
        permission_denied => "capture.permission.denied",
    }
}

pub mod preferences {
    use rust_i18n::t;

    i18n_fns! {
        title => "preferences.window.title",
        page_general => "preferences.pages.general.title",
        page_shortcuts => "preferences.pages.shortcuts.title",
        page_notifications => "preferences.pages.notifications.title",
        page_ocr => "preferences.pages.ocr.title",
        page_about => "preferences.pages.about.title",
        auto_start => "preferences.fields.auto_start",
        auto_start_description => "preferences.fields.auto_start_description",
        language => "preferences.fields.language",
        language_description => "preferences.fields.language_description",
        theme => "preferences.fields.theme",
        theme_description => "preferences.fields.theme_description",
        font_family => "preferences.fields.font_family",
        font_family_description => "preferences.fields.font_family_description",
        ocr => "preferences.fields.ocr",
        ocr_model => "preferences.fields.ocr_model",
        notifications_enabled => "preferences.fields.notifications_enabled",
        notifications_enabled_description => "preferences.fields.notifications_enabled_description",
        save_notification => "preferences.fields.save_notification",
        save_notification_description => "preferences.fields.save_notification_description",
        copy_notification => "preferences.fields.copy_notification",
        copy_notification_description => "preferences.fields.copy_notification_description",
        qr_code_notification => "preferences.fields.qr_code_notification",
        qr_code_notification_description => "preferences.fields.qr_code_notification_description",
        shutter_sound => "preferences.fields.shutter_sound",
        shutter_sound_description => "preferences.fields.shutter_sound_description",
        save_directory => "preferences.fields.save_directory",
        image_compression => "preferences.fields.image_compression",
        image_compression_description => "preferences.fields.image_compression_description",
        capture_shortcut => "preferences.fields.capture_shortcut",
        quick_capture_shortcut => "preferences.fields.quick_capture_shortcut",
        capture_shortcut_description => "preferences.fields.capture_shortcut_description",
        quick_capture_shortcut_description => "preferences.fields.quick_capture_shortcut_description",
        ocr_enabled_description => "preferences.fields.ocr_enabled_description",
        select_save_directory => "preferences.actions.select_save_directory",
        browse => "preferences.actions.browse",
        open => "preferences.actions.open",
        shortcuts_recording => "preferences.actions.shortcuts_recording",
        shortcuts_recording_hint => "preferences.actions.shortcuts_recording_hint",
        shortcuts_restore_defaults => "preferences.actions.shortcuts_restore_defaults",
        ocr_download_action => "preferences.actions.ocr_download",
        ocr_redownload_action => "preferences.actions.ocr_redownload",
        ocr_download_in_progress => "preferences.actions.ocr_download_in_progress",
        follow_system => "preferences.options.follow_system",
        theme_light => "preferences.options.light",
        theme_dark => "preferences.options.dark",
        language_zh_cn => "preferences.options.zh_cn",
        language_en_us => "preferences.options.en_us",
        about_summary => "preferences.about.summary",
        version_label => "preferences.about.version",
        github_repository => "preferences.about.github_repository",
        github_repository_description => "preferences.about.github_repository_description",
        report_issue => "preferences.about.report_issue",
        report_issue_description => "preferences.about.report_issue_description",
        open_log_folder => "preferences.about.open_log_folder",
        shortcuts_conflict => "preferences.shortcuts.conflict",
        ocr_status_missing => "preferences.ocr.status_missing",
        ocr_status_ready => "preferences.ocr.status_ready",
        ocr_download_completed => "preferences.ocr.download_completed",
        ocr_download_failed => "preferences.ocr.download_failed",
        ocr_note => "preferences.ocr.note",
        folder_picker_failed => "preferences.errors.folder_picker_failed",
    }

    i18n_fns_with_args! {
        default_path_with_value(path) => "preferences.fields.default_path_with_value",
        ocr_status_downloading(progress) => "preferences.ocr.status_downloading",
        ocr_status_failed(message) => "preferences.ocr.status_failed",
    }
}

pub mod pin {
    use rust_i18n::t;

    i18n_fns! {
        close_all => "pin.menu.close_all",
    }
}

pub mod tray {
    use rust_i18n::t;

    i18n_fns! {
        capture_overlay => "tray.actions.capture_overlay",
        quick_capture => "tray.actions.quick_capture",
        preferences => "tray.actions.preferences",
        exit => "tray.actions.exit",
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_locale_tag;

    #[test]
    fn normalize_locale_tag_maps_supported_aliases() {
        assert_eq!(normalize_locale_tag("zh_CN"), "zh-CN");
        assert_eq!(normalize_locale_tag("zh"), "zh-CN");
        assert_eq!(normalize_locale_tag("en-US"), "en");
        assert_eq!(normalize_locale_tag("en"), "en");
    }

    #[test]
    fn normalize_locale_tag_replaces_underscores_for_other_locales() {
        assert_eq!(normalize_locale_tag("pt_BR"), "pt-BR");
    }
}
