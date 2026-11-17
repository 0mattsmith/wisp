mod cookies;
mod elsewhere;
mod jobs;
mod links;
mod meta;
mod settings;
mod tags;
mod tools;
mod tray;

use jobs::{Dirs, Job, Manager};
use meta::{Candidate, Query, TrackMeta};
use serde::Deserialize;
use settings::Settings;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager as _, RunEvent, State, WindowEvent};

type Mgr<'a> = State<'a, Arc<Manager>>;

// ------------------------------------------------------------------ settings

#[tauri::command]
fn get_settings(mgr: Mgr) -> Settings {
    mgr.settings()
}

#[tauri::command]
fn set_settings(app: AppHandle, mgr: Mgr, settings: Settings) -> Result<Settings, String> {
    let saved = mgr.set_settings(settings)?;
    apply_dock(&app, &saved);
    let _ = app.emit("settings-changed", &saved);
    Ok(saved)
}

fn apply_dock(app: &AppHandle, settings: &Settings) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_dock_visibility(!settings.hide_dock_icon);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, settings);
    }
}

// --------------------------------------------------------------------- queue

#[tauri::command]
fn list_jobs(mgr: Mgr) -> Vec<Job> {
    mgr.list()
}

#[tauri::command]
fn add_urls(mgr: Mgr, urls: Vec<String>, format: Option<String>, quality: Option<String>) -> usize {
    mgr.add_urls(urls, format, quality)
}

#[tauri::command]
async fn add_files(
    mgr: Mgr<'_>,
    paths: Vec<String>,
    convert: bool,
    format: Option<String>,
    quality: Option<String>,
) -> Result<usize, String> {
    Ok(mgr.inner().add_files(paths, convert, format, quality).await)
}

#[tauri::command]
fn pause_job(mgr: Mgr, id: String) {
    mgr.pause(&id)
}

#[tauri::command]
fn resume_job(mgr: Mgr, id: String) {
    mgr.resume(&id)
}

#[tauri::command]
fn cancel_job(mgr: Mgr, id: String) {
    mgr.cancel(&id)
}

#[tauri::command]
fn remove_job(mgr: Mgr, id: String) {
    mgr.remove(&id)
}

#[tauri::command]
fn clear_finished(mgr: Mgr) {
    mgr.clear_finished()
}

#[tauri::command]
fn pause_all(mgr: Mgr) {
    mgr.pause_all()
}

#[tauri::command]
fn resume_all(mgr: Mgr) {
    mgr.resume_all()
}

#[tauri::command]
async fn search_tracks(mgr: Mgr<'_>, query: String, site: String) -> Result<Vec<jobs::SearchHit>, String> {
    mgr.inner().search(&query, &site).await
}

// ---------------------------------------------------------------------- tags

#[tauri::command]
async fn read_tags(path: String) -> Result<tags::FileTags, String> {
    tokio::task::spawn_blocking(move || tags::read(&PathBuf::from(path)))
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoverInput {
    /// keep | remove | url | file
    kind: String,
    value: Option<String>,
}

/// Save edited tags (and optionally new cover art) to a file, renaming it to
/// follow the filename template if asked. Returns the file's path afterwards.
#[tauri::command]
async fn write_tags(
    mgr: Mgr<'_>,
    job_id: Option<String>,
    path: String,
    meta: TrackMeta,
    cover: CoverInput,
    rename: bool,
) -> Result<String, String> {
    let mgr = mgr.inner().clone();
    let settings = mgr.settings();
    let file = PathBuf::from(&path);
    if !file.is_file() {
        return Err("This file has been moved or deleted.".into());
    }

    let raw: Option<Vec<u8>> = match (cover.kind.as_str(), cover.value) {
        ("url", Some(url)) => Some(
            meta::fetch_image(&mgr.http, &url)
                .await
                .ok_or("Could not download that artwork.")?,
        ),
        ("file", Some(p)) => Some(tokio::fs::read(&p).await.map_err(|e| format!("Could not read the image: {e}"))?),
        _ => None,
    };
    let remove = cover.kind == "remove";
    let square = settings.square_cover;

    let (tag_file, tag_meta) = (file.clone(), meta.clone());
    let thumb = tokio::task::spawn_blocking(move || -> Result<Option<Option<String>>, String> {
        let (change, thumb) = match raw {
            Some(bytes) => {
                let jpeg = tags::prepare_cover(&bytes, square, 1400)?;
                let thumb = tags::data_url(&jpeg, 112);
                (tags::CoverChange::Set(jpeg), Some(thumb))
            }
            None if remove => (tags::CoverChange::Remove, Some(None)),
            None => (tags::CoverChange::Keep, None),
        };
        tags::write(&tag_file, &tag_meta, change, false)?;
        Ok(thumb)
    })
    .await
    .map_err(|e| e.to_string())??;

    let mut final_path = file.clone();
    if rename {
        let dir = file.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let ext = file.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        let old_stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let stem = jobs::render_filename(&settings.filename_template, &meta, &old_stem);
        if stem != old_stem {
            let mut target = dir.join(format!("{stem}.{ext}"));
            let mut n = 2;
            while target.exists() {
                target = dir.join(format!("{stem} ({n}).{ext}"));
                n += 1;
            }
            tokio::fs::rename(&file, &target)
                .await
                .map_err(|e| format!("Tags saved, but the file could not be renamed: {e}"))?;
            final_path = target;
        }
    }

    let out = final_path.to_string_lossy().into_owned();
    if let Some(id) = job_id {
        let size = std::fs::metadata(&final_path).map(|m| m.len()).ok();
        mgr.mutate(&id, |j| {
            if let Some(t) = meta.title.clone().filter(|t| !t.trim().is_empty()) {
                j.title = t;
            }
            j.artist = meta.artist.clone();
            j.album = meta.album.clone();
            if let Some(t) = thumb {
                j.thumbnail = t;
            }
            j.output = Some(out.clone());
            j.size = size;
            j.error = None;
        });
    }
    Ok(out)
}

#[tauri::command]
async fn search_metadata(mgr: Mgr<'_>, query: Query) -> Result<Vec<Candidate>, String> {
    let mgr = mgr.inner().clone();
    Ok(meta::search(&mgr.http, &mgr.settings(), &query).await)
}

// --------------------------------------------------------------------- tools

#[tauri::command]
async fn tools_status(mgr: Mgr<'_>) -> Result<Vec<tools::ToolStatus>, String> {
    Ok(tools::status(&mgr.dirs.bin).await)
}

#[tauri::command]
async fn install_tool(app: AppHandle, mgr: Mgr<'_>, name: String) -> Result<(), String> {
    let mgr = mgr.inner().clone();
    tools::install(&app, &mgr.http, &mgr.dirs.bin, &name).await?;
    let _ = app.emit("tools-changed", ());
    mgr.pump();
    Ok(())
}

#[tauri::command]
async fn ytdlp_latest(mgr: Mgr<'_>) -> Result<Option<String>, String> {
    Ok(tools::latest_ytdlp(&mgr.http).await)
}

// ------------------------------------------------------------------- windows

#[tauri::command]
fn open_main(app: AppHandle) {
    tray::hide_panel(&app, false);
    tray::show_main(&app);
}

#[tauri::command]
fn hide_panel(app: AppHandle) {
    tray::hide_panel(&app, false);
}

// -------------------------------------------------------------------- player

/// Let the player read one finished track, and return where it is. Files
/// are opened up one at a time rather than exposing whole folders to the UI.
#[tauri::command]
fn player_source(app: AppHandle, mgr: Mgr, id: String) -> Result<String, String> {
    let path = mgr
        .get(&id)
        .and_then(|j| j.output)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .ok_or("This file has been moved or deleted.")?;
    app.asset_protocol_scope()
        .allow_file(&path)
        .map_err(|e| format!("Could not open the file for playback: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

/// The most recent "play these" request. It is kept until the player confirms
/// it, because the player window may still be starting up when it is sent and
/// would otherwise miss it.
#[derive(Default)]
struct PlayRequest(std::sync::Mutex<Option<serde_json::Value>>);

fn request_play(app: &AppHandle, ids: Vec<String>, index: usize, muted: bool) {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let request = serde_json::json!({ "type": "play", "ids": ids, "index": index, "seq": seq, "muted": muted });
    *app.state::<PlayRequest>().0.lock().unwrap() = Some(request.clone());
    tray::show_player(app);
    let _ = app.emit_to("player", "player-command", request);
}

#[tauri::command]
fn player_play(app: AppHandle, ids: Vec<String>, index: usize) {
    request_play(&app, ids, index, false);
}

/// Called by the player when it starts, to pick up a request it missed.
#[tauri::command]
fn player_pending(pending: State<PlayRequest>) -> Option<serde_json::Value> {
    pending.0.lock().unwrap().clone()
}

#[tauri::command]
fn player_ack(pending: State<PlayRequest>, seq: u64) {
    let mut slot = pending.0.lock().unwrap();
    if slot.as_ref().and_then(|r| r.get("seq")).and_then(|s| s.as_u64()) == Some(seq) {
        *slot = None;
    }
}

#[tauri::command]
fn player_show(app: AppHandle) {
    tray::show_player(&app);
}

#[tauri::command]
fn player_hide(app: AppHandle) {
    tray::hide_player(&app);
}

/// Whether this is a development build, so the UI knows to send diagnostics.
#[tauri::command]
fn dev_enabled() -> bool {
    cfg!(debug_assertions)
}

/// Development builds print what the windows report (script errors, player
/// progress) to the terminal. Release builds ignore it.
#[tauri::command]
fn dev_log(window: tauri::Window, message: String) {
    #[cfg(debug_assertions)]
    {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let t = START.get_or_init(std::time::Instant::now).elapsed().as_secs_f32();
        eprintln!("[ui:{} +{t:.1}s] {message}", window.label());
    }
    #[cfg(not(debug_assertions))]
    let _ = (window, message);
}

/// Work out where a browser the user picked keeps its profile.
#[tauri::command]
async fn detect_browser(path: String) -> Result<cookies::CustomBrowser, String> {
    tokio::task::spawn_blocking(move || cookies::detect(std::path::Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
}

/// Unlock the chosen browser's cookies as a download would, and report what was found.
#[tauri::command]
async fn check_cookies(mgr: Mgr<'_>) -> Result<String, String> {
    let mgr = mgr.inner().clone();
    cookies::check(&mgr.settings(), &mgr.dirs.bin, &mgr.dirs.work).await
}

/// The bookmark code for the "Send to Dubplate" browser button.
#[tauri::command]
fn bookmarklet() -> String {
    links::bookmarklet()
}

/// Put "Send to Dubplate" in the macOS Share menu.
#[tauri::command]
async fn add_share_shortcut(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?;
        tokio::task::spawn_blocking(move || links::add_share_shortcut(&dir))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err("The Share menu Shortcut is only available on macOS.".into())
    }
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    #[cfg(desktop)]
    {
        // A development build pointed at its own scratch folder may run
        // alongside the installed app; otherwise a second launch just
        // brings the first one forward.
        let scratch_run = cfg!(debug_assertions) && std::env::var_os("DUBPLATE_DEV_DIR").is_some();
        if !scratch_run {
            builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
                // A dubplate:// link opened while the app runs (Windows and Linux start a
                // second copy to deliver it) is handled by the deep-link plugin; anything
                // else, such as opening the app again, brings the window forward.
                if !args.iter().any(|a| a.starts_with(&format!("{}:", links::SCHEME))) {
                    tray::show_main(app);
                }
            }));
        }
        builder = builder
            .plugin(tauri_plugin_updater::Builder::new().build())
            // Started at login, the app opens straight into the menu bar / tray.
            .plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                Some(vec!["--hidden"]),
            ));
    }
    builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let data = app.path().app_data_dir()?;
            let cache = app.path().app_cache_dir()?;
            let music = app
                .path()
                .audio_dir()
                .or_else(|_| app.path().download_dir())
                .unwrap_or_else(|_| data.clone());
            // Development builds can be pointed at a throwaway folder so that
            // testing never touches the real library or Music folder.
            #[cfg(debug_assertions)]
            let (data, cache, music) = match std::env::var_os("DUBPLATE_DEV_DIR").map(PathBuf::from) {
                Some(dev) => (dev.join("data"), dev.join("cache"), dev.join("music")),
                None => (data, cache, music),
            };
            let dirs = Dirs {
                work: cache.join("work"),
                bin: data.join("bin"),
                settings_file: data.join("settings.json"),
                jobs_file: data.join("library.json"),
            };
            // Leftovers from a crash; paused jobs restart cleanly without them.
            let _ = std::fs::remove_dir_all(&dirs.work);
            let mgr = Arc::new(Manager::new(handle.clone(), dirs, music.join("Dubplate")));
            apply_dock(&handle, &mgr.settings());
            mgr.start_background();
            #[cfg(debug_assertions)]
            if let Ok(items) = std::env::var("DUBPLATE_DEV_ADD") {
                // Links are queued for download, file paths for conversion.
                let (files, urls): (Vec<String>, Vec<String>) =
                    items.split_whitespace().map(String::from).partition(|i| i.starts_with('/'));
                mgr.add_urls(urls, None, None);
                let adder = mgr.clone();
                tauri::async_runtime::spawn(async move { adder.add_files(files, true, None, None).await });
            }
            app.manage(mgr);
            app.manage(PlayRequest::default());
            tray::setup(&handle)?;

            // dubplate:// links from the browser button or the Share menu.
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                // Installers register the link type; this covers an AppImage that was
                // never installed, and development builds on Windows.
                #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
                let _ = handle.deep_link().register_all();
                let app = handle.clone();
                handle.deep_link().on_open_url(move |event| links::handle(&app, event.urls()));
                // A link that launched the app (Windows, Linux) arrives here instead.
                if let Ok(Some(urls)) = handle.deep_link().get_current() {
                    links::handle(&handle, urls);
                }
            }
            #[cfg(debug_assertions)]
            if let Ok(link) = std::env::var("DUBPLATE_DEV_LINK") {
                if let Ok(url) = tauri::Url::parse(&link) {
                    links::handle(&handle, vec![url]);
                }
            }
            // Development aid: start the first library track playing, silently, so
            // playback can be checked from a terminal.
            #[cfg(debug_assertions)]
            if std::env::var_os("DUBPLATE_DEV_PLAY").is_some() {
                let ids: Vec<String> = handle
                    .state::<Arc<Manager>>()
                    .list()
                    .into_iter()
                    .filter(|j| j.output.is_some())
                    .map(|j| j.id)
                    .collect();
                // Muted, so a test run does not blare out of the speakers.
                request_play(&handle, ids, 0, true);
            }
            // Development aid: close the main window a few seconds after launch,
            // exactly as the red button would, to check what the app does next.
            #[cfg(debug_assertions)]
            if std::env::var_os("DUBPLATE_DEV_CLOSE").is_some() {
                let app = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                    if let Some(win) = app.get_webview_window("main") {
                        eprintln!("[dev] closing the main window");
                        let _ = win.close();
                    }
                });
            }
            if !std::env::args().any(|a| a == "--hidden") {
                tray::show_main(&handle);
            }
            Ok(())
        })
        .on_page_load(|webview, payload| {
            // Development builds note each window's page load in the terminal.
            #[cfg(debug_assertions)]
            eprintln!("[page:{}] {:?} {}", webview.label(), payload.event(), payload.url());
            #[cfg(not(debug_assertions))]
            let _ = (webview, payload);
        })
        .on_window_event(|window, event| match (window.label(), event) {
            ("panel", WindowEvent::Focused(false)) => tray::hide_panel(window.app_handle(), true),
            ("main", WindowEvent::CloseRequested { api, .. }) => {
                let app = window.app_handle();
                if app.state::<Arc<Manager>>().settings().keep_in_tray {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    // The hidden panel window would otherwise keep the app alive.
                    app.exit(0);
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            list_jobs,
            add_urls,
            add_files,
            pause_job,
            resume_job,
            cancel_job,
            remove_job,
            clear_finished,
            pause_all,
            resume_all,
            search_tracks,
            read_tags,
            write_tags,
            search_metadata,
            tools_status,
            install_tool,
            ytdlp_latest,
            open_main,
            hide_panel,
            player_source,
            player_play,
            player_pending,
            player_ack,
            dev_enabled,
            player_show,
            player_hide,
            dev_log,
            bookmarklet,
            add_share_shortcut,
            detect_browser,
            check_cookies,
            quit_app,
        ])
        .build(tauri::generate_context!())
        .expect("error while starting Dubplate")
        .run(|app, event| match event {
            RunEvent::Exit => app.state::<Arc<Manager>>().shutdown(),
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => tray::show_main(app),
            _ => {}
        });
}
