//! `wisp://` links, so a browser button or the Share menu can hand the app a
//! page to download:
//!
//!   wisp://add?url=<the page's address, URL-encoded>[&format=flac]
//!
//! Links made before the app was renamed (`dubplate://…`) still work.
//!
//! Any web page can try to open a link like this, so only YouTube and
//! SoundCloud pages are accepted, a link can add at most a handful of items,
//! and every link that arrives is announced with a notification.

use crate::jobs::{self, Manager};
use std::sync::Arc;
use tauri::{AppHandle, Manager as _, Url};

pub const SCHEME: &str = "wisp";
/// The app's name before it became Wisp; bookmarks made then still use it.
pub const OLD_SCHEME: &str = "dubplate";
const MAX_PER_LINK: usize = 5;

#[derive(Debug, PartialEq)]
pub struct AddRequest {
    pub urls: Vec<String>,
    pub format: Option<String>,
}

pub fn parse(link: &Url) -> Result<AddRequest, String> {
    if link.scheme() != SCHEME && link.scheme() != OLD_SCHEME {
        return Err("Not a Wisp link.".into());
    }
    // "wisp://add?url=…" puts "add" in the host; "wisp:add?url=…" in the path.
    let action = link.host_str().unwrap_or_else(|| link.path()).trim_matches('/');
    if action != "add" {
        return Err(format!("Wisp does not know what to do with “{action}”."));
    }

    let mut urls = Vec::new();
    let mut format = None;
    for (key, value) in link.query_pairs() {
        match key.as_ref() {
            "url" => {
                let candidate = value.trim().to_string();
                let parsed = Url::parse(&candidate).map_err(|_| "That is not a valid web address.".to_string())?;
                if !matches!(parsed.scheme(), "http" | "https") {
                    return Err("Only web addresses can be sent to Wisp.".into());
                }
                if jobs::site_of(&candidate) == "web" {
                    return Err("Wisp only accepts YouTube and SoundCloud links.".into());
                }
                if !urls.contains(&candidate) {
                    urls.push(candidate);
                }
            }
            "format" if crate::settings::FORMATS.contains(&value.as_ref()) => format = Some(value.into_owned()),
            _ => {}
        }
    }
    if urls.is_empty() {
        return Err("The link did not include a page to download.".into());
    }
    if urls.len() > MAX_PER_LINK {
        return Err(format!("A link can add at most {MAX_PER_LINK} items at once."));
    }
    Ok(AddRequest { urls, format })
}

fn notify(app: &AppHandle, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title("Wisp").body(body).show();
}

/// Queue whatever the links ask for, and say what happened.
pub fn handle(app: &AppHandle, links: Vec<Url>) {
    let Some(mgr) = app.try_state::<Arc<Manager>>().map(|s| s.inner().clone()) else { return };
    for link in links {
        match parse(&link) {
            Ok(request) => {
                let count = request.urls.len();
                let added = mgr.add_urls(request.urls, request.format, None);
                let body = match (added, count) {
                    (0, _) => "That is already in your queue.".to_string(),
                    (1, _) => "Added to your downloads.".to_string(),
                    (n, _) => format!("Added {n} items to your downloads."),
                };
                notify(app, &body);
            }
            Err(e) => notify(app, &e),
        }
    }
}

/// The bookmark people add to their browser: it sends the current page here.
pub fn bookmarklet() -> String {
    format!("javascript:(()=>{{location.href='{SCHEME}://add?url='+encodeURIComponent(location.href)}})()")
}

/// The Shortcut that puts "Send to Wisp" in the Share menu: it takes the
/// shared web page, URL-encodes its address and opens a wisp:// link.
const SHARE_SHORTCUT: &str = include_str!("../resources/share-shortcut.plist");

/// Build the Share-menu Shortcut, sign it on this Mac, and hand it to the
/// Shortcuts app, which asks the user whether to add it.
#[cfg(target_os = "macos")]
pub fn add_share_shortcut(work_dir: &std::path::Path) -> Result<(), String> {
    use std::process::Command;
    let dir = work_dir.join("shortcut");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let source = dir.join("share.plist");
    let unsigned = dir.join("unsigned.shortcut");
    // The file name becomes the Shortcut's name.
    let signed = dir.join("Send to Wisp.shortcut");
    let _ = std::fs::remove_file(&signed);
    std::fs::write(&source, SHARE_SHORTCUT).map_err(|e| e.to_string())?;

    let run = |cmd: &mut Command, what: &str| -> Result<(), String> {
        let out = cmd.output().map_err(|e| format!("Could not {what}: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            Err(format!("Could not {what}. {}", err.lines().last().unwrap_or("").trim()))
        }
    };
    run(
        Command::new("/usr/bin/plutil").args(["-convert", "binary1", "-o"]).arg(&unsigned).arg(&source),
        "prepare the Shortcut",
    )?;
    // Signed with this Mac's own account, so only this Mac's Shortcuts app sees it.
    run(
        Command::new("/usr/bin/shortcuts")
            .args(["sign", "--mode", "people-who-know-me", "--input"])
            .arg(&unsigned)
            .arg("--output")
            .arg(&signed),
        "sign the Shortcut (is iCloud signed in?)",
    )?;
    run(Command::new("/usr/bin/open").arg(&signed), "open the Shortcuts app")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<AddRequest, String> {
        parse(&Url::parse(s).unwrap())
    }

    #[test]
    fn accepts_music_links() {
        let r = p("wisp://add?url=https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3DjNQXAC9IVRw%26t%3D3").unwrap();
        assert_eq!(r.urls, vec!["https://www.youtube.com/watch?v=jNQXAC9IVRw&t=3"]);
        assert_eq!(r.format, None);
        let r = p("wisp://add?url=https://soundcloud.com/a/b&format=flac").unwrap();
        assert_eq!(r.urls, vec!["https://soundcloud.com/a/b"]);
        assert_eq!(r.format.as_deref(), Some("flac"));
        // Unknown formats are ignored rather than refused.
        assert_eq!(p("wisp://add?url=https://youtu.be/x&format=exe").unwrap().format, None);
        assert!(p("wisp:add?url=https://youtu.be/x").is_ok());
        // Bookmarks made before the rename.
        assert!(p("dubplate://add?url=https://youtu.be/x").is_ok());
        assert!(p("wisp://add/?url=https://youtu.be/x").is_ok());
    }

    #[test]
    fn refuses_anything_else() {
        assert!(p("wisp://add?url=https://example.com/file.mp3").is_err());
        assert!(p("wisp://add?url=file:///etc/passwd").is_err());
        assert!(p("wisp://add?url=javascript:alert(1)").is_err());
        assert!(p("wisp://add?url=https://youtube.com.evil.example/watch").is_err());
        assert!(p("wisp://add").is_err());
        assert!(p("wisp://delete?url=https://youtu.be/x").is_err());
        assert!(p("https://youtu.be/x").is_err());
        let many: String = (0..6).map(|i| format!("url=https://youtu.be/{i}&")).collect();
        assert!(p(&format!("wisp://add?{many}")).is_err());
    }

    #[test]
    fn share_shortcut_builds_the_same_link() {
        // The Shortcut glues the encoded page address onto this prefix; the
        // placeholder for it must sit right after the prefix it was measured from.
        let prefix = format!("{SCHEME}://add?url=");
        assert!(SHARE_SHORTCUT.contains(&format!("{prefix}&#xFFFC;")));
        assert!(SHARE_SHORTCUT.contains(&format!("{{{}, 1}}", prefix.chars().count())));
    }

    #[test]
    fn bookmarklet_builds_a_parseable_link() {
        let code = bookmarklet();
        assert!(code.starts_with("javascript:"));
        assert!(code.contains("wisp://add?url="));
    }
}
