//! Locating, versioning and installing the external tools (yt-dlp, ffmpeg, ffprobe).
//!
//! Resolution order: the app-managed copy in `<app data>/bin`, then anything on
//! PATH or in the usual package-manager folders. GUI apps on macOS start with a
//! bare PATH, so the well-known folders are searched explicitly.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const FFMPEG_RELEASE: &str = "b6.1.1";

pub fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// A `Command` that never opens a console window and dies with the app.
pub fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(std::process::Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            dirs.push(local.join("Microsoft").join("WinGet").join("Links"));
        }
        if let Some(h) = home() {
            dirs.push(h.join("scoop").join("shims"));
        }
        dirs.push(PathBuf::from(r"C:\ProgramData\chocolatey\bin"));
    } else {
        for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin", "/snap/bin", "/opt/local/bin"] {
            dirs.push(PathBuf::from(d));
        }
        if let Some(h) = home() {
            dirs.push(h.join(".local").join("bin"));
            dirs.push(h.join("bin"));
            dirs.push(h.join(".deno").join("bin"));
            // `pip install --user` on macOS: ~/Library/Python/3.x/bin, newest Python first.
            let mut user_pythons: Vec<PathBuf> = std::fs::read_dir(h.join("Library").join("Python"))
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path().join("bin"))
                .filter(|p| p.is_dir())
                .collect();
            user_pythons.sort();
            dirs.extend(user_pythons.into_iter().rev());
        }
    }
    dirs
}

/// Returns the tool path and whether it is the app-managed copy.
/// ffmpeg and ffprobe come from the same build and belong in the same folder:
/// yt-dlp only looks for ffprobe next to the ffmpeg it is given.
pub const FFMPEG_PAIR: [&str; 2] = ["ffmpeg", "ffprobe"];

pub fn find(name: &str, bin_dir: &Path) -> Option<(PathBuf, bool)> {
    let dirs: Vec<(PathBuf, bool)> = std::iter::once((bin_dir.to_path_buf(), true))
        .chain(search_dirs().into_iter().map(|d| (d, false)))
        .collect();
    find_in(name, dirs)
}

/// `find`, over a given list of folders (each marked as Wisp's own or not).
fn find_in(name: &str, dirs: Vec<(PathBuf, bool)>) -> Option<(PathBuf, bool)> {
    let file = exe(name);

    // Prefer a folder holding the complete pair (MacPorts' or Homebrew's ffmpeg
    // and ffprobe) over a lone ffmpeg found earlier in the list.
    if FFMPEG_PAIR.contains(&name) {
        let pair = dirs.iter().find(|(d, _)| FFMPEG_PAIR.iter().all(|t| d.join(exe(t)).is_file()));
        if let Some((dir, managed)) = pair {
            return Some((dir.join(&file), *managed));
        }
    }
    dirs.into_iter()
        .map(|(d, managed)| (d.join(&file), managed))
        .find(|(p, _)| p.is_file())
}

/// A JavaScript runtime for yt-dlp's YouTube signature solving, as a
/// `--js-runtimes` value. Always passed with an explicit path.
pub fn js_runtime(bin_dir: &Path) -> Option<String> {
    for name in ["deno", "node", "bun"] {
        if let Some((path, _)) = find(name, bin_dir) {
            return Some(format!("{name}:{}", path.display()));
        }
    }
    None
}

pub async fn version(name: &str, path: &Path) -> Option<String> {
    let arg = if name == "yt-dlp" { "--version" } else { "-version" };
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        command(path).arg(arg).output(),
    )
    .await
    .ok()?
    .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?.trim();
    if name == "yt-dlp" {
        Some(line.to_string())
    } else {
        // "ffmpeg version 8.1.1 Copyright ..."
        line.split_whitespace().nth(2).map(|s| s.to_string())
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub name: String,
    pub found: bool,
    pub managed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    /// Whether this platform has a build the app can install itself.
    pub installable: bool,
}

pub async fn status(bin_dir: &Path) -> Vec<ToolStatus> {
    let mut out = Vec::new();
    for name in ["yt-dlp", "ffmpeg", "ffprobe"] {
        let found = find(name, bin_dir);
        let version = match &found {
            Some((p, _)) => version(name, p).await,
            None => None,
        };
        out.push(ToolStatus {
            name: name.to_string(),
            found: found.is_some(),
            managed: found.as_ref().map(|f| f.1).unwrap_or(false),
            path: found.map(|f| f.0.to_string_lossy().into_owned()),
            version,
            installable: asset_url(name).is_some(),
        });
    }
    out
}

fn ytdlp_asset() -> Option<&'static str> {
    use std::env::consts::{ARCH, OS};
    match (OS, ARCH) {
        ("macos", _) => Some("yt-dlp_macos"),
        ("windows", "aarch64") => Some("yt-dlp_arm64.exe"),
        ("windows", "x86") => Some("yt-dlp_x86.exe"),
        ("windows", _) => Some("yt-dlp.exe"),
        ("linux", "x86_64") => Some("yt-dlp_linux"),
        ("linux", "aarch64") => Some("yt-dlp_linux_aarch64"),
        _ => None,
    }
}

fn ffmpeg_platform() -> Option<&'static str> {
    use std::env::consts::{ARCH, OS};
    match (OS, ARCH) {
        ("macos", "aarch64") => Some("darwin-arm64"),
        ("macos", "x86_64") => Some("darwin-x64"),
        ("linux", "x86_64") => Some("linux-x64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("linux", "x86") => Some("linux-ia32"),
        ("linux", "arm") => Some("linux-arm"),
        // x64 builds run under emulation on Windows on ARM.
        ("windows", "x86_64") | ("windows", "aarch64") => Some("win32-x64"),
        _ => None,
    }
}

fn asset_url(name: &str) -> Option<String> {
    match name {
        "yt-dlp" => ytdlp_asset().map(|a| {
            format!("https://github.com/yt-dlp/yt-dlp/releases/latest/download/{a}")
        }),
        "ffmpeg" | "ffprobe" => ffmpeg_platform().map(|p| {
            format!(
                "https://github.com/eugeneware/ffmpeg-static/releases/download/{FFMPEG_RELEASE}/{name}-{p}.gz"
            )
        }),
        _ => None,
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolProgress<'a> {
    name: &'a str,
    downloaded: u64,
    total: Option<u64>,
    done: bool,
}

async fn download_to(
    app: &AppHandle,
    http: &reqwest::Client,
    name: &str,
    url: &str,
    dest: &Path,
) -> Result<String, String> {
    let mut resp = http
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Download failed: {e}"))?;
    let total = resp.content_length();
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("Cannot write {}: {e}", dest.display()))?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0u64;
    let mut last_emit = std::time::Instant::now();
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("Download failed: {e}"))? {
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;
        if last_emit.elapsed().as_millis() > 120 {
            last_emit = std::time::Instant::now();
            let _ = app.emit("tool-progress", ToolProgress { name, downloaded, total, done: false });
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Download (or update) a tool into the app-managed bin folder. Asking for
/// ffmpeg or ffprobe installs both, so they always sit side by side.
pub async fn install(
    app: &AppHandle,
    http: &reqwest::Client,
    bin_dir: &Path,
    name: &str,
) -> Result<(), String> {
    if FFMPEG_PAIR.contains(&name) {
        for tool in FFMPEG_PAIR {
            install_one(app, http, bin_dir, tool).await?;
        }
        return Ok(());
    }
    install_one(app, http, bin_dir, name).await
}

async fn install_one(
    app: &AppHandle,
    http: &reqwest::Client,
    bin_dir: &Path,
    name: &str,
) -> Result<(), String> {
    let url = asset_url(name).ok_or_else(|| {
        format!("No prebuilt {name} for this platform. Install it with your package manager.")
    })?;
    tokio::fs::create_dir_all(bin_dir).await.map_err(|e| e.to_string())?;
    let tmp = bin_dir.join(format!("{name}.download"));
    let dest = bin_dir.join(exe(name));

    let result = async {
        let digest = download_to(app, http, name, &url, &tmp).await?;

        if name == "yt-dlp" {
            // yt-dlp publishes checksums next to every release.
            let asset = ytdlp_asset().unwrap_or_default();
            let sums = http
                .get("https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS")
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| format!("Could not fetch checksums: {e}"))?
                .text()
                .await
                .map_err(|e| e.to_string())?;
            let expected = sums
                .lines()
                .filter_map(|l| l.split_once(char::is_whitespace))
                .find(|(_, file)| file.trim().trim_start_matches('*') == asset)
                .map(|(hash, _)| hash.trim().to_lowercase())
                .ok_or("Checksum for this build was not published")?;
            if expected != digest {
                return Err("Checksum mismatch, download discarded".to_string());
            }
            tokio::fs::rename(&tmp, &dest).await.map_err(|e| e.to_string())?;
        } else {
            let (src, out) = (tmp.clone(), dest.clone());
            tokio::task::spawn_blocking(move || -> std::io::Result<()> {
                let part = out.with_extension("unpack");
                let mut gz = flate2::read::GzDecoder::new(std::fs::File::open(&src)?);
                let mut f = std::fs::File::create(&part)?;
                std::io::copy(&mut gz, &mut f)?;
                drop(f);
                std::fs::rename(&part, &out)?;
                std::fs::remove_file(&src)
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("Could not unpack {name}: {e}"))?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    .await;

    if result.is_err() {
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    let _ = app.emit("tool-progress", ToolProgress { name, downloaded: 0, total: None, done: true });
    result
}

/// Latest yt-dlp release tag, e.g. "2026.10.26".
pub async fn latest_ytdlp(http: &reqwest::Client) -> Option<String> {
    let v: serde_json::Value = http
        .get("https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    v.get("tag_name")?.as_str().map(|s| s.to_string())
}

/// Whether this ffmpeg build includes a given encoder. Package-manager builds
/// vary (Homebrew's, for one, ships without libvorbis), so it is checked
/// rather than assumed. The answer is remembered per binary.
pub async fn has_encoder(ffmpeg: &Path, encoder: &str) -> bool {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let known = cache.lock().unwrap().get(ffmpeg).cloned();
    let listing = match known {
        Some(l) => l,
        None => {
            let out = command(ffmpeg).args(["-hide_banner", "-encoders"]).output().await;
            let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
            if !text.is_empty() {
                cache.lock().unwrap().insert(ffmpeg.to_path_buf(), text.clone());
            }
            text
        }
    };
    listing.lines().any(|l| l.split_whitespace().nth(1) == Some(encoder))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, names: &[&str]) {
        std::fs::create_dir_all(dir).unwrap();
        for n in names {
            std::fs::write(dir.join(exe(n)), b"").unwrap();
        }
    }

    /// A friend's Mac: a lone ffmpeg in /usr/local/bin, and MacPorts' complete
    /// set (ffmpeg, ffprobe, yt-dlp, deno) in /opt/local/bin, searched later.
    #[test]
    fn keeps_ffmpeg_and_ffprobe_together() {
        let root = std::env::temp_dir().join(format!("wisp-tools-test-{}", std::process::id()));
        let (wisp, usr_local, macports) = (root.join("wisp"), root.join("usr-local"), root.join("opt-local"));
        touch(&wisp, &[]);
        touch(&usr_local, &["ffmpeg"]);
        touch(&macports, &["ffmpeg", "ffprobe", "yt-dlp", "deno"]);
        let dirs = || vec![(wisp.clone(), true), (usr_local.clone(), false), (macports.clone(), false)];

        assert_eq!(find_in("ffmpeg", dirs()).unwrap().0, macports.join(exe("ffmpeg")));
        assert_eq!(find_in("ffprobe", dirs()).unwrap().0, macports.join(exe("ffprobe")));
        assert_eq!(find_in("yt-dlp", dirs()).unwrap().0, macports.join(exe("yt-dlp")));

        // Once Wisp has installed its own pair, that wins.
        touch(&wisp, &["ffmpeg", "ffprobe"]);
        assert_eq!(find_in("ffmpeg", dirs()), Some((wisp.join(exe("ffmpeg")), true)));

        // With no complete pair anywhere, the first ffmpeg found is still used.
        let lone = vec![(usr_local.clone(), false)];
        assert_eq!(find_in("ffmpeg", lone.clone()).unwrap().0, usr_local.join(exe("ffmpeg")));
        assert_eq!(find_in("ffprobe", lone), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
