//! Menu bar (macOS) / system tray (Windows, Linux) icon and the quick panel
//! that drops down from it.

use crate::jobs::Manager;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager as _, PhysicalPosition, Rect};

/// When the panel last hid itself because it lost focus. Clicking the tray
/// icon while the panel is open blurs it first; without this the same click
/// would immediately reopen it.
static LAST_BLUR_HIDE: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn show_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// Show the pop-out player. The first time, it is tucked into the bottom
/// right corner of the main screen; after that it stays wherever it was dragged.
pub fn show_player(app: &AppHandle) {
    static PLACED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let Some(win) = app.get_webview_window("player") else { return };
    if !PLACED.swap(true, Ordering::Relaxed) {
        if let (Ok(Some(monitor)), Ok(size)) = (win.primary_monitor(), win.outer_size()) {
            let scale = monitor.scale_factor();
            let x = monitor.position().x as f64 + monitor.size().width as f64 - size.width as f64 - 28.0 * scale;
            // Leave room for the Dock or taskbar.
            let y = monitor.position().y as f64 + monitor.size().height as f64 - size.height as f64 - 110.0 * scale;
            let _ = win.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
        }
    }
    let _ = win.show();
}

pub fn hide_player(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("player") {
        let _ = win.hide();
    }
}

pub fn hide_panel(app: &AppHandle, from_blur: bool) {
    if let Some(win) = app.get_webview_window("panel") {
        if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
            if from_blur {
                LAST_BLUR_HIDE.store(now_ms(), Ordering::Relaxed);
            }
        }
    }
}

fn toggle_panel(app: &AppHandle, rect: Rect) {
    let Some(win) = app.get_webview_window("panel") else { return };
    if win.is_visible().unwrap_or(false) {
        let _ = win.hide();
        return;
    }
    if now_ms().saturating_sub(LAST_BLUR_HIDE.load(Ordering::Relaxed)) < 350 {
        return;
    }

    let scale = win.scale_factor().unwrap_or(1.0);
    let icon_pos = rect.position.to_physical::<f64>(scale);
    let icon_size = rect.size.to_physical::<f64>(scale);
    let size = win.outer_size().map(|s| (s.width as f64, s.height as f64)).unwrap_or((384.0 * scale, 520.0 * scale));
    let gap = 6.0 * scale;

    let mut x = icon_pos.x + icon_size.width / 2.0 - size.0 / 2.0;
    let mut y = icon_pos.y + icon_size.height + gap;
    if let Ok(Some(monitor)) = app.monitor_from_point(icon_pos.x, icon_pos.y) {
        let (mx, my) = (monitor.position().x as f64, monitor.position().y as f64);
        let (mw, mh) = (monitor.size().width as f64, monitor.size().height as f64);
        // Tray at the bottom of the screen (Windows, some Linux panels): open upwards.
        if icon_pos.y > my + mh / 2.0 {
            y = icon_pos.y - size.1 - gap;
        }
        x = x.clamp(mx + gap, (mx + mw - size.0 - gap).max(mx + gap));
        y = y.clamp(my, (my + mh - size.1).max(my));
    }
    let _ = win.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
    let _ = win.show();
    let _ = win.set_focus();
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Dubplate", true, None::<&str>)?;
    let paste = MenuItem::with_id(app, "paste", "Download Link from Clipboard", true, None::<&str>)?;
    let player = MenuItem::with_id(app, "player", "Show Player", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause_all", "Pause All", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume_all", "Resume All", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Dubplate", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &player,
            &paste,
            &PredefinedMenuItem::separator(app)?,
            &pause,
            &resume,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("dubplate")
        .tooltip("Dubplate")
        .menu(&menu)
        // Linux trays do not report clicks, so the menu is the only entry point there.
        .show_menu_on_left_click(cfg!(target_os = "linux"))
        .on_menu_event(|app, event| {
            let mgr = app.state::<Arc<Manager>>().inner().clone();
            match event.id().as_ref() {
                "open" => show_main(app),
                "player" => show_player(app),
                "paste" => {
                    use tauri_plugin_clipboard_manager::ClipboardExt;
                    if let Ok(text) = app.clipboard().read_text() {
                        let urls: Vec<String> = text.split_whitespace().map(|s| s.to_string()).collect();
                        mgr.add_urls(urls, None, None);
                    }
                }
                "pause_all" => mgr.pause_all(),
                "resume_all" => mgr.resume_all(),
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                toggle_panel(tray.app_handle(), rect);
            }
        });

    // A monochrome template icon follows the macOS menu bar's light/dark
    // appearance; elsewhere the full-colour app icon reads better.
    if cfg!(target_os = "macos") {
        builder = builder
            .icon(tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?)
            .icon_as_template(true);
    } else if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
