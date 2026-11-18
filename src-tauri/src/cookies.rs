//! Signed-in browser cookies for yt-dlp, for age-restricted and members-only
//! items.
//!
//! Browsers yt-dlp knows (Chrome, Firefox, Safari…) are handed to it by name.
//! Firefox-family browsers it does not know (Zen, LibreWolf…) are handed over
//! by profile folder, since their cookies are stored unencrypted. Chromium-
//! family browsers it does not know (Helium, Arc, Thorium…) keep their cookies
//! encrypted with a key in the system keychain under a name yt-dlp cannot
//! guess, so on macOS Wisp unlocks them itself and gives yt-dlp a private
//! cookie file holding only the YouTube, Google and SoundCloud cookies. The
//! file is deleted as soon as the download that needed it ends.

use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Browsers yt-dlp reads by name.
pub const BUILT_IN: &[&str] = &["chrome", "chromium", "firefox", "safari", "edge", "brave", "opera", "vivaldi", "whale"];

/// Only these sites' cookies are ever written out.
const DOMAINS: &[&str] = &["youtube.com", "google.com", "soundcloud.com"];

/// A browser yt-dlp does not know by name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomBrowser {
    pub label: String,
    /// "chromium" or "firefox"
    pub kind: String,
    /// Chromium: the "User Data" folder (holding "Local State").
    /// Firefox: the profile folder (holding cookies.sqlite).
    pub profile: String,
    /// macOS keychain entry holding a Chromium browser's cookie key.
    pub keychain: Option<String>,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default()
}

/// Helium, by imput: Chromium-based, not known to yt-dlp.
pub fn helium() -> CustomBrowser {
    let profile = if cfg!(target_os = "macos") {
        home().join("Library/Application Support/net.imput.helium")
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default().join("imput").join("Helium").join("User Data")
    } else {
        home().join(".config/net.imput.helium")
    };
    CustomBrowser {
        label: "Helium".into(),
        kind: "chromium".into(),
        profile: profile.to_string_lossy().into_owned(),
        keychain: Some("Helium Storage Key".into()),
    }
}

// ------------------------------------------------------------ detection

fn is_chromium_data(dir: &Path) -> bool {
    dir.join("Local State").is_file()
}

/// The most recently used profile in a Firefox-family profiles folder.
fn firefox_profile(dir: &Path) -> Option<PathBuf> {
    if dir.join("cookies.sqlite").is_file() {
        return Some(dir.to_path_buf());
    }
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for base in [dir.to_path_buf(), dir.join("Profiles")] {
        for entry in std::fs::read_dir(&base).into_iter().flatten().flatten() {
            let cookies = entry.path().join("cookies.sqlite");
            if let Ok(modified) = std::fs::metadata(&cookies).and_then(|m| m.modified()) {
                if best.as_ref().map_or(true, |(t, _)| modified > *t) {
                    best = Some((modified, entry.path()));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Whether the macOS keychain has an entry by this name (its secret is not read).
#[cfg(target_os = "macos")]
fn keychain_has(service: &str) -> bool {
    std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", service])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn app_names(app: &Path) -> Vec<String> {
    let out = std::process::Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(app.join("Contents/Info.plist"))
        .output();
    let info: serde_json::Value = out.ok().and_then(|o| serde_json::from_slice(&o.stdout).ok()).unwrap_or_default();
    let mut names: Vec<String> = ["CFBundleIdentifier", "CFBundleDisplayName", "CFBundleName"]
        .iter()
        .filter_map(|k| info.get(*k).and_then(|v| v.as_str()).map(String::from))
        .collect();
    if let Some(stem) = app.file_stem() {
        names.push(stem.to_string_lossy().into_owned());
    }
    names.dedup();
    names
}

fn describe(dir: &Path, label: &str, names: &[String]) -> Option<CustomBrowser> {
    if is_chromium_data(dir) {
        #[cfg(target_os = "macos")]
        let keychain = names
            .iter()
            .filter(|n| !n.contains('.'))
            .flat_map(|n| [format!("{n} Safe Storage"), format!("{n} Storage Key")])
            .find(|svc| keychain_has(svc));
        #[cfg(not(target_os = "macos"))]
        let keychain = {
            let _ = names;
            None
        };
        return Some(CustomBrowser {
            label: label.to_string(),
            kind: "chromium".into(),
            profile: dir.to_string_lossy().into_owned(),
            keychain,
        });
    }
    firefox_profile(dir).map(|p| CustomBrowser {
        label: label.to_string(),
        kind: "firefox".into(),
        profile: p.to_string_lossy().into_owned(),
        keychain: None,
    })
}

/// Work out where a browser keeps its profile, from the browser app itself or
/// from a profile folder the user picked.
pub fn detect(path: &Path) -> Result<CustomBrowser, String> {
    let label = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Custom browser".into());

    #[cfg(target_os = "macos")]
    let names = if path.extension().is_some_and(|e| e == "app") { app_names(path) } else { vec![label.clone()] };
    #[cfg(not(target_os = "macos"))]
    let names = vec![label.clone()];

    // A folder: the profile itself, or the folder holding it.
    if path.is_dir() && path.extension().map_or(true, |e| e != "app") {
        return describe(path, &label, &names).ok_or_else(|| {
            "That folder is not a browser profile. Choose the folder that holds “Local State” (Chromium browsers) or “profiles.ini” (Firefox browsers).".to_string()
        });
    }

    // An app: look where browsers usually keep their data.
    let mut roots: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        roots.push(home().join("Library/Application Support"));
    } else if cfg!(windows) {
        for var in ["LOCALAPPDATA", "APPDATA"] {
            if let Some(dir) = std::env::var_os(var) {
                roots.push(PathBuf::from(dir));
            }
        }
    } else {
        roots.push(home().join(".config"));
        roots.push(home().join(".mozilla"));
        roots.push(home());
    }
    let wanted: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    let matches = |name: &str| {
        let name = name.trim_start_matches('.').to_lowercase();
        wanted.iter().any(|w| name == *w || (w.len() > 3 && name.contains(w.as_str())))
    };
    for root in &roots {
        for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            // The browser's own folder, or one level down ("Vendor/Browser", "Browser/User Data").
            let mut candidates = Vec::new();
            if matches(&name) {
                candidates.push(dir.clone());
                candidates.push(dir.join("User Data"));
            }
            for sub in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                if matches(&sub.file_name().to_string_lossy()) {
                    candidates.push(sub.path());
                    candidates.push(sub.path().join("User Data"));
                }
            }
            for candidate in candidates {
                if let Some(found) = describe(&candidate, &label, &names) {
                    return Ok(found);
                }
            }
        }
    }
    Err(format!("Could not find where {label} keeps its profile. Choose its profile folder instead."))
}

// -------------------------------------------------------------- unlocking

/// The cookie database of a Chromium "User Data" folder's last-used profile.
fn chromium_cookie_db(user_data: &Path) -> Option<PathBuf> {
    let state: serde_json::Value = std::fs::read(user_data.join("Local State"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let last = state.pointer("/profile/last_used").and_then(|v| v.as_str()).unwrap_or("Default");
    [last, "Default"]
        .iter()
        .flat_map(|p| [user_data.join(p).join("Cookies"), user_data.join(p).join("Network").join("Cookies")])
        .find(|p| p.is_file())
}

/// Turn a keychain password into the AES key Chromium uses on macOS.
pub fn derive_key(password: &[u8]) -> [u8; 16] {
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, b"saltysalt", 1003, &mut key);
    key
}

/// Decrypt one cookie value ("v10" + AES-128-CBC). Databases from version 24
/// on put a 32-byte hash of the cookie's domain in front of the value.
pub fn decrypt_value(key: &[u8; 16], encrypted: &[u8], db_version: i64) -> Option<String> {
    use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    let body = encrypted.strip_prefix(b"v10")?;
    let mut buf = body.to_vec();
    let plain = cbc::Decryptor::<aes::Aes128>::new(key.into(), &[b' '; 16].into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .ok()?;
    let plain = if db_version >= 24 { plain.get(32..)? } else { plain };
    String::from_utf8(plain.to_vec()).ok()
}

/// The cookie key for a keychain entry. macOS asks the user the first time.
#[cfg(target_os = "macos")]
fn keychain_key(service: &str) -> Result<[u8; 16], String> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    // Kept for the session, so the keychain is only asked once.
    static KEYS: OnceLock<Mutex<HashMap<String, [u8; 16]>>> = OnceLock::new();
    let keys = KEYS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(key) = keys.lock().unwrap().get(service) {
        return Ok(*key);
    }
    let out = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-w", "-s", service])
        .output()
        .map_err(|e| format!("Could not open the keychain: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "The keychain did not allow access to “{service}”. Try again and click Always Allow when macOS asks."
        ));
    }
    let password = String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string();
    let key = derive_key(password.as_bytes());
    keys.lock().unwrap().insert(service.to_string(), key);
    Ok(key)
}

pub struct Cookie {
    pub domain: String,
    pub path: String,
    pub secure: bool,
    /// Unix seconds; 0 for a cookie that lasts the browser session.
    pub expires: i64,
    pub name: String,
    pub value: String,
}

fn wanted_domain(host: &str) -> bool {
    let host = host.trim_start_matches('.');
    DOMAINS.iter().any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

/// Read and unlock a Chromium browser's cookies for the sites Wisp uses.
#[cfg(target_os = "macos")]
fn read_chromium(browser: &CustomBrowser, scratch: &Path) -> Result<Vec<Cookie>, String> {
    let label = &browser.label;
    let db = chromium_cookie_db(Path::new(&browser.profile))
        .ok_or_else(|| format!("Could not find {label}’s cookies. Has it been opened and signed in?"))?;
    let service = browser
        .keychain
        .as_deref()
        .ok_or_else(|| format!("Could not find {label}’s cookie key in the keychain."))?;
    let key = keychain_key(service)?;

    // The browser keeps its database locked while it runs; read a copy.
    std::fs::create_dir_all(scratch).map_err(|e| e.to_string())?;
    let copy = scratch.join("browser-cookies.db");
    std::fs::copy(&db, &copy).map_err(|e| format!("Could not read {label}’s cookies: {e}"))?;
    for side in ["-wal", "-journal"] {
        let from = PathBuf::from(format!("{}{side}", db.display()));
        if from.is_file() {
            let _ = std::fs::copy(&from, PathBuf::from(format!("{}{side}", copy.display())));
        }
    }
    let result = (|| -> rusqlite::Result<Vec<Cookie>> {
        let conn = rusqlite::Connection::open_with_flags(&copy, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let version: i64 = conn
            .query_row("SELECT value FROM meta WHERE key = 'version'", [], |r| r.get::<_, String>(0))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mut stmt =
            conn.prepare("SELECT host_key, path, is_secure, expires_utc, name, value, encrypted_value FROM cookies")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Vec<u8>>(6)?,
            ))
        })?;
        let mut cookies = Vec::new();
        for row in rows.flatten() {
            let (domain, path, secure, expires_utc, name, plain, encrypted) = row;
            if !wanted_domain(&domain) {
                continue;
            }
            let value = if encrypted.is_empty() { Some(plain) } else { decrypt_value(&key, &encrypted, version) };
            let Some(value) = value else { continue };
            // Chromium counts microseconds from 1601; cookie files use Unix seconds.
            let expires = if expires_utc == 0 { 0 } else { (expires_utc / 1_000_000 - 11_644_473_600).max(0) };
            cookies.push(Cookie { domain, path, secure: secure != 0, expires, name, value });
        }
        Ok(cookies)
    })();
    for side in ["", "-wal", "-journal"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{side}", copy.display())));
    }
    result.map_err(|e| format!("Could not read {label}’s cookies: {e}"))
}

/// A Netscape cookie file, the format yt-dlp reads with --cookies.
pub fn netscape(cookies: &[Cookie]) -> String {
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    for c in cookies {
        let sub = if c.domain.starts_with('.') { "TRUE" } else { "FALSE" };
        let secure = if c.secure { "TRUE" } else { "FALSE" };
        out.push_str(&format!("{}\t{sub}\t{}\t{secure}\t{}\t{}\t{}\n", c.domain, c.path, c.expires, c.name, c.value));
    }
    out
}

/// A cookie file that removes itself once the download using it is over.
pub struct CookieFile(PathBuf);

impl Drop for CookieFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(text.as_bytes())
}

/// The yt-dlp options for the cookies chosen in Settings. Keep the returned
/// file alive until yt-dlp has finished.
pub struct CookieArgs {
    pub args: Vec<String>,
    _file: Option<CookieFile>,
}

fn browser_for(settings: &Settings) -> Option<CustomBrowser> {
    match settings.cookies_browser.as_str() {
        "helium" => Some(helium()),
        "custom" => settings.cookies_custom.clone(),
        _ => None,
    }
}

pub fn prepare(settings: &Settings, scratch: &Path) -> Result<CookieArgs, String> {
    let choice = settings.cookies_browser.as_str();
    if choice.is_empty() {
        return Ok(CookieArgs { args: vec![], _file: None });
    }
    if BUILT_IN.contains(&choice) {
        return Ok(CookieArgs { args: vec!["--cookies-from-browser".into(), choice.into()], _file: None });
    }
    let browser = browser_for(settings).ok_or("Choose the browser to use cookies from under Settings → Downloads.")?;
    match browser.kind.as_str() {
        "firefox" => Ok(CookieArgs {
            args: vec!["--cookies-from-browser".into(), format!("firefox:{}", browser.profile)],
            _file: None,
        }),
        #[cfg(target_os = "macos")]
        _ => {
            let cookies = read_chromium(&browser, scratch)?;
            static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = scratch.join(format!("cookies-{}-{n}.txt", std::process::id()));
            write_private(&path, &netscape(&cookies)).map_err(|e| format!("Could not prepare cookies: {e}"))?;
            Ok(CookieArgs { args: vec!["--cookies".into(), path.to_string_lossy().into_owned()], _file: Some(CookieFile(path)) })
        }
        // Elsewhere yt-dlp unlocks Chromium cookies itself, given the profile folder.
        #[cfg(not(target_os = "macos"))]
        _ => {
            let _ = scratch;
            Ok(CookieArgs {
                args: vec!["--cookies-from-browser".into(), format!("chromium:{}", browser.profile)],
                _file: None,
            })
        }
    }
}

/// What to say when macOS privacy protection stops Wisp reading a browser's
/// cookies (Safari's always need Full Disk Access).
pub const PRIVACY_HELP: &str = "macOS is blocking Wisp from reading Safari’s cookies. Turn on Full Disk Access for Wisp (System Settings → Privacy & Security → Full Disk Access), then quit and reopen Wisp. Or pick another browser.";

/// Whether yt-dlp's output shows macOS privacy protection refusing a cookie file.
pub fn blocked_by_privacy(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("operation not permitted") && (lower.contains("cookies") || lower.contains("safari"))
}

/// A plain-language report of what the chosen browser's cookies would give.
pub fn summarise(cookies: &[Cookie]) -> String {
    let count = |site: &str| cookies.iter().filter(|c| c.domain.trim_start_matches('.').ends_with(site)).count();
    let has = |site: &str, names: &[&str]| {
        cookies.iter().any(|c| c.domain.trim_start_matches('.').ends_with(site) && names.contains(&c.name.as_str()))
    };
    let youtube = if has("youtube.com", &["LOGIN_INFO", "SAPISID", "__Secure-3PSID", "SID"]) {
        "signed in to YouTube".to_string()
    } else if count("youtube.com") > 0 {
        "has YouTube cookies but is not signed in to YouTube".to_string()
    } else {
        "has no YouTube cookies yet (open youtube.com in it and sign in)".to_string()
    };
    let soundcloud = if count("soundcloud.com") > 0 { "has SoundCloud cookies" } else { "has no SoundCloud cookies" };
    format!("{youtube}; {soundcloud}")
}

/// Read cookies from a Netscape cookie file (as yt-dlp writes them).
pub fn parse_netscape(text: &str) -> Vec<Cookie> {
    text.lines()
        .filter_map(|line| {
            let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
            if line.starts_with('#') {
                return None;
            }
            let f: Vec<&str> = line.split('\t').collect();
            (f.len() >= 7).then(|| Cookie {
                domain: f[0].into(),
                path: f[2].into(),
                secure: f[3] == "TRUE",
                expires: f[4].parse().unwrap_or(0),
                name: f[5].into(),
                value: f[6].into(),
            })
        })
        .filter(|c| wanted_domain(&c.domain))
        .collect()
}

/// For the Settings "Check" button: unlock the chosen browser's cookies the
/// same way a download would, and say what was found. Values are never shown.
pub async fn check(settings: &Settings, bin_dir: &Path, scratch: &Path) -> Result<String, String> {
    let prepared = {
        let (settings, scratch) = (settings.clone(), scratch.to_path_buf());
        tokio::task::spawn_blocking(move || prepare(&settings, &scratch)).await.map_err(|e| e.to_string())??
    };
    if prepared.args.is_empty() {
        return Err("Cookies are switched off.".into());
    }
    let name = match settings.cookies_browser.as_str() {
        "custom" => settings.cookies_custom.as_ref().map(|b| b.label.clone()).unwrap_or_default(),
        "helium" => "Helium".into(),
        other => {
            let mut c = other.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    };

    // Our own cookie file can be read directly; otherwise let yt-dlp load the
    // browser's cookies and save them to a scratch file we can count.
    if prepared.args[0] == "--cookies" {
        let text = std::fs::read_to_string(&prepared.args[1]).unwrap_or_default();
        return Ok(format!("{name} {}.", summarise(&parse_netscape(&text))));
    }
    let (ytdlp, _) = crate::tools::find("yt-dlp", bin_dir).ok_or("yt-dlp is not installed.")?;
    std::fs::create_dir_all(scratch).map_err(|e| e.to_string())?;
    let jar = CookieFile(scratch.join(format!("cookie-check-{}.txt", std::process::id())));
    write_private(&jar.0, "").map_err(|e| e.to_string())?;
    let out = crate::tools::command(&ytdlp)
        .args(&prepared.args)
        .arg("--cookies")
        .arg(&jar.0)
        .args(["--flat-playlist", "--skip-download", "--no-warnings", "--no-update", "--playlist-end", "1", "--", "ytsearch1:music"])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        if blocked_by_privacy(&err) {
            return Err(PRIVACY_HELP.to_string());
        }
        let line = err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or("yt-dlp could not read the cookies.");
        return Err(format!("Could not read {name}’s cookies. {}", line.trim_start_matches("ERROR:").trim()));
    }
    let text = std::fs::read_to_string(&jar.0).unwrap_or_default();
    Ok(format!("{name} {}.", summarise(&parse_netscape(&text))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};

    fn encrypt(key: &[u8; 16], host: &str, value: &str, version: i64) -> Vec<u8> {
        use sha2::{Digest, Sha256};
        let mut plain = if version >= 24 { Sha256::digest(host.as_bytes()).to_vec() } else { vec![] };
        plain.extend_from_slice(value.as_bytes());
        let len = plain.len();
        let mut buf = plain;
        buf.resize(len + 16, 0);
        let ct = cbc::Encryptor::<aes::Aes128>::new(key.into(), &[b' '; 16].into())
            .encrypt_padded_mut::<Pkcs7>(&mut buf, len)
            .unwrap()
            .to_vec();
        [b"v10".as_slice(), &ct].concat()
    }

    #[test]
    fn unlocks_chromium_cookie_values() {
        let key = derive_key(b"some keychain password");
        for version in [23, 24] {
            let enc = encrypt(&key, ".youtube.com", "LOGIN-VALUE-123", version);
            assert_eq!(decrypt_value(&key, &enc, version).as_deref(), Some("LOGIN-VALUE-123"));
        }
        // The wrong key or an unknown format gives nothing rather than garbage.
        let enc = encrypt(&key, ".youtube.com", "secret", 24);
        assert_eq!(decrypt_value(&derive_key(b"wrong"), &enc, 24), None);
        assert_eq!(decrypt_value(&key, b"v20abcdef", 24), None);
    }

    #[test]
    fn spots_macos_privacy_blocks() {
        // As reported from a friend's Mac, with Safari chosen.
        assert!(blocked_by_privacy("ERROR: [Errno 1] Operation not permitted: '/Users/user/Library/Cookies/Cookies.binarycookies'"));
        assert!(blocked_by_privacy("PermissionError: [Errno 1] Operation not permitted: '/Users/x/Library/Containers/com.apple.Safari/Data/Library/Cookies/Cookies.binarycookies'"));
        assert!(!blocked_by_privacy("ERROR: unable to download video data: HTTP Error 403: Forbidden"));
    }

    #[test]
    fn only_music_sites_are_kept() {
        assert!(wanted_domain(".youtube.com"));
        assert!(wanted_domain("accounts.google.com"));
        assert!(wanted_domain("soundcloud.com"));
        assert!(!wanted_domain(".notyoutube.com"));
        assert!(!wanted_domain("mail.example.com"));
        assert!(!wanted_domain(".google.com.evil.example"));
    }

    #[test]
    fn writes_and_reads_cookie_files() {
        let cookies = vec![
            Cookie { domain: ".youtube.com".into(), path: "/".into(), secure: true, expires: 1_900_000_000, name: "LOGIN_INFO".into(), value: "x".into() },
            Cookie { domain: "soundcloud.com".into(), path: "/".into(), secure: false, expires: 0, name: "sc".into(), value: "y".into() },
        ];
        let text = netscape(&cookies);
        assert!(text.contains(".youtube.com\tTRUE\t/\tTRUE\t1900000000\tLOGIN_INFO\tx"));
        let back = parse_netscape(&text);
        assert_eq!(back.len(), 2);
        assert!(summarise(&back).starts_with("signed in to YouTube"));
        assert!(summarise(&back[1..]).starts_with("has no YouTube cookies"));
    }
}

#[cfg(test)]
mod live {
    /// Looks at the browsers installed on this machine (no cookies are read):
    /// `cargo test detects_installed_browsers -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn detects_installed_browsers() {
        for entry in std::fs::read_dir("/Applications").into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let browser = ["Helium", "Chrome", "Firefox", "Brave", "Arc", "Zen", "Edge", "Vivaldi", "Opera", "LibreWolf", "Thorium", "Chromium", "Orion"]
                .iter()
                .any(|b| name.contains(b));
            if browser {
                match super::detect(&entry.path()) {
                    Ok(b) => println!("{name}: {} · {} · keychain {:?}", b.kind, b.profile, b.keychain),
                    Err(e) => println!("{name}: {e}"),
                }
            }
        }
        let h = super::helium();
        println!("Helium preset: {} · cookies db {:?}", h.profile, super::chromium_cookie_db(std::path::Path::new(&h.profile)));
    }
}
