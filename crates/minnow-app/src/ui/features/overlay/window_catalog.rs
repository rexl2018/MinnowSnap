use crate::services::app_meta::APP_NAME;
use crate::services::geometry::Rect;
use serde::{Deserialize, Serialize};
use tracing::info;
use xcap::{Monitor, Window};

const MIN_VIRTUAL_WIDTH: i32 = 1920;
const MIN_VIRTUAL_HEIGHT: i32 = 1080;
const MIN_WINDOW_EDGE: i32 = 8;
const MENU_BAR_MAX_HEIGHT: i32 = 48;
const DOCK_MAX_HEIGHT: i32 = 96;
const SYSTEM_OVERLAYS: &[&str] = &[
    "程序坞",
    "Dock",
    "Window Server",
    "SystemUIServer",
    "Control Center",
    "Notification Center",
    "Spotlight",
    "StatusIndicator",
];

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WindowInfo {
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub app_name: String,
}

/// Convert native window bounds into overlay (GPUI logical) space.
///
/// On macOS, `xcap` window/monitor geometry comes from `kCGWindowBounds` /
/// `CGDisplayBounds`, which are already in points. Dividing by the backing
/// scale would shrink every window toward the origin and miss the hovered
/// window. On Windows the same APIs report physical pixels, so the scale is
/// applied there.
#[must_use]
pub(crate) fn overlay_coord_scale(native_scale: f32) -> f32 {
    #[cfg(target_os = "macos")]
    {
        let _ = native_scale;
        1.0
    }
    #[cfg(not(target_os = "macos"))]
    {
        native_scale.max(1.0)
    }
}

#[must_use]
pub(crate) fn to_overlay_rect(rect: Rect, origin_x: i32, origin_y: i32, coord_scale: f32) -> Option<(i32, i32, u32, u32)> {
    let scale = if coord_scale > 0.0 { coord_scale } else { 1.0 };
    let width = (rect.width as f32 / scale).round().max(1.0) as u32;
    let height = (rect.height as f32 / scale).round().max(1.0) as u32;
    let x = ((rect.x - origin_x) as f32 / scale).round() as i32;
    let y = ((rect.y - origin_y) as f32 / scale).round() as i32;
    Some((x, y, width, height))
}

fn is_system_overlay(app_name: &str, title: &str) -> bool {
    if app_name == APP_NAME {
        return true;
    }
    SYSTEM_OVERLAYS
        .iter()
        .any(|name| app_name.eq_ignore_ascii_case(name) || title.eq_ignore_ascii_case(name))
}

fn is_screen_chrome(rect: Rect, screen: Rect) -> bool {
    let full_width = rect.width >= screen.width.saturating_sub(4);
    let menu_bar = rect.y <= screen.y + 2 && rect.height <= MENU_BAR_MAX_HEIGHT;
    let dock = rect.y + rect.height >= screen.y + screen.height - 4 && rect.height <= DOCK_MAX_HEIGHT;
    full_width && (menu_bar || dock)
}

fn is_too_small(rect: Rect) -> bool {
    rect.width < MIN_WINDOW_EDGE || rect.height < MIN_WINDOW_EDGE
}

#[must_use]
pub fn fetch_windows_data() -> Vec<WindowInfo> {
    let windows = Window::all().unwrap_or_default();
    let monitors = Monitor::all().unwrap_or_default();
    let primary = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .or_else(|| monitors.first());
    let native_scale = primary
        .and_then(|monitor| monitor.scale_factor().ok())
        .filter(|scale| *scale > 0.0)
        .unwrap_or(1.0);
    let coord_scale = overlay_coord_scale(native_scale);
    info!(
        "Fetching window data, total windows found: {}, native_scale: {}, coord_scale: {}",
        windows.len(),
        native_scale,
        coord_scale
    );

    let (screen_rect, origin_x, origin_y, primary_rect) = if monitors.is_empty() {
        let fallback = Rect {
            x: 0,
            y: 0,
            width: 10000,
            height: 10000,
        };
        (fallback, 0, 0, fallback)
    } else {
        let origin_x = primary.and_then(|monitor| monitor.x().ok()).unwrap_or(0);
        let origin_y = primary.and_then(|monitor| monitor.y().ok()).unwrap_or(0);
        let primary_rect = primary
            .map(|monitor| Rect {
                x: monitor.x().unwrap_or(0),
                y: monitor.y().unwrap_or(0),
                width: i32::try_from(monitor.width().unwrap_or(0)).unwrap_or(0),
                height: i32::try_from(monitor.height().unwrap_or(0)).unwrap_or(0),
            })
            .unwrap_or(Rect {
                x: origin_x,
                y: origin_y,
                width: MIN_VIRTUAL_WIDTH,
                height: MIN_VIRTUAL_HEIGHT,
            });
        let (min_x, min_y, max_x, max_y) = monitors
            .iter()
            .fold((i32::MAX, i32::MAX, i32::MIN, i32::MIN), |(min_x, min_y, max_x, max_y), m| {
                let x = m.x().unwrap_or(0);
                let y = m.y().unwrap_or(0);
                let w = i32::try_from(m.width().unwrap_or(0)).unwrap_or(0);
                let h = i32::try_from(m.height().unwrap_or(0)).unwrap_or(0);
                (min_x.min(x), min_y.min(y), max_x.max(x + w), max_y.max(y + h))
            });

        (
            Rect {
                x: min_x,
                y: min_y,
                width: (max_x - min_x).max(MIN_VIRTUAL_WIDTH),
                height: (max_y - min_y).max(MIN_VIRTUAL_HEIGHT),
            },
            origin_x,
            origin_y,
            primary_rect,
        )
    };

    let results: Vec<WindowInfo> = windows
        .into_iter()
        .filter(|w| !w.is_minimized().unwrap_or(true))
        .filter_map(|window| {
            let w = window.width().ok().filter(|&w| w > 0)?;
            let h = window.height().ok().filter(|&h| h > 0)?;
            let x = window.x().unwrap_or(0);
            let y = window.y().unwrap_or(0);
            let w_i32 = i32::try_from(w).ok()?;
            let h_i32 = i32::try_from(h).ok()?;

            let current_rect = Rect {
                x,
                y,
                width: w_i32,
                height: h_i32,
            };
            let valid_rect = current_rect.intersect(screen_rect)?;
            if is_too_small(valid_rect) || is_screen_chrome(valid_rect, primary_rect) {
                return None;
            }

            let app_name = window.app_name().unwrap_or_else(|_| "Unknown".to_string());
            let title = window.title().unwrap_or_else(|_| "Unknown".to_string());
            if is_system_overlay(&app_name, &title) {
                return None;
            }

            let (logical_x, logical_y, logical_w, logical_h) = to_overlay_rect(valid_rect, origin_x, origin_y, coord_scale)?;

            Some(WindowInfo {
                title,
                x: logical_x,
                y: logical_y,
                width: logical_w,
                height: logical_h,
                app_name,
            })
        })
        .collect();

    info!("Filtered visible windows: {}", results.len());
    results
}

#[must_use]
pub fn find_window_at(windows: &[WindowInfo], x: f64, y: f64) -> Option<usize> {
    windows.iter().position(|w| {
        let wx = f64::from(w.x);
        let wy = f64::from(w.y);
        let ww = f64::from(w.width);
        let wh = f64::from(w.height);
        x >= wx && x <= (wx + ww) && y >= wy && y <= (wy + wh)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_coordinates_stay_unscaled() {
        let (x, y, w, h) = to_overlay_rect(Rect::new(200, 80, 1000, 640), 0, 0, 1.0).unwrap();
        assert_eq!((x, y, w, h), (200, 80, 1000, 640));
    }

    #[test]
    fn overlay_coord_scale_matches_platform_window_space() {
        let scale = overlay_coord_scale(2.0);
        #[cfg(target_os = "macos")]
        assert_eq!(scale, 1.0);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(scale, 2.0);
    }

    #[test]
    fn physical_coordinates_are_scaled_into_overlay_space() {
        let (x, y, w, h) = to_overlay_rect(Rect::new(400, 160, 2000, 1280), 0, 0, 2.0).unwrap();
        assert_eq!((x, y, w, h), (200, 80, 1000, 640));
    }

    #[test]
    fn overlay_rect_subtracts_monitor_origin() {
        let (x, y, w, h) = to_overlay_rect(Rect::new(2120, 80, 800, 600), 1920, 0, 1.0).unwrap();
        assert_eq!((x, y, w, h), (200, 80, 800, 600));
    }

    #[test]
    fn find_window_at_returns_the_frontmost_containing_window() {
        let editor = WindowInfo {
            title: "Editor".into(),
            app_name: "FibonaxStudio".into(),
            x: 200,
            y: 80,
            width: 1000,
            height: 700,
        };
        let desktop = WindowInfo {
            title: "Desktop".into(),
            app_name: "Finder".into(),
            x: 0,
            y: 0,
            width: 1512,
            height: 982,
        };

        let front_to_back = vec![editor.clone(), desktop.clone()];
        let idx = find_window_at(&front_to_back, 500.0, 400.0).unwrap();
        assert_eq!(front_to_back[idx].title, "Editor");

        let covered = vec![desktop, editor];
        let idx = find_window_at(&covered, 500.0, 400.0).unwrap();
        assert_eq!(covered[idx].title, "Desktop");
    }

    #[test]
    fn system_overlays_are_detected() {
        assert!(is_system_overlay("Dock", ""));
        assert!(is_system_overlay("Window Server", "Menubar"));
        assert!(is_system_overlay(APP_NAME, "MinnowSnap"));
        assert!(!is_system_overlay("FibonaxStudio", "main"));
    }

    #[test]
    fn screen_chrome_matches_menu_bar_and_dock() {
        let screen = Rect::new(0, 0, 1512, 982);
        assert!(is_screen_chrome(Rect::new(0, 0, 1512, 24), screen));
        assert!(is_screen_chrome(Rect::new(0, 902, 1512, 80), screen));
        assert!(!is_screen_chrome(Rect::new(200, 80, 1000, 700), screen));
        assert!(!is_too_small(Rect::new(200, 80, 1000, 700)));
        assert!(is_too_small(Rect::new(10, 10, 4, 40)));
    }
}
