//! The work queue: downloading with yt-dlp, converting with ffmpeg, tagging,
//! and keeping the list of jobs (which doubles as the library) on disk.

use crate::cookies;
use crate::elsewhere;
use crate::meta::{self, TrackMeta};
use crate::settings::Settings;
use crate::{tags, tools};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Queued,
    Fetching,
    Downloading,
    Converting,
    Tagging,
    Done,
    Failed,
    Canceled,
    Paused,
}

impl Status {
    fn active(self) -> bool {
        matches!(self, Status::Fetching | Status::Downloading | Status::Converting | Status::Tagging)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Job {
    pub id: String,
    /// download | convert | import | playlist (a placeholder while a playlist is read)
    pub kind: String,
    /// The URL, or the source file for convert/import jobs.
    pub source: String,
    /// youtube | soundcloud | web | file
    pub site: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Remote URL while downloading, a small data URL once the file is tagged.
    pub thumbnail: Option<String>,
    pub duration: Option<f64>,
    pub format: String,
    pub quality: String,
    pub status: Status,
    pub progress: f32,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed: Option<f64>,
    pub eta: Option<u64>,
    pub error: Option<String>,
    /// Final file on disk.
    pub output: Option<String>,
    pub size: Option<u64>,
    /// Which database the tags came from, if one matched.
    pub matched: Option<String>,
    pub match_score: Option<f32>,
    /// Hidden from the Downloads list but still in the Library.
    pub cleared: bool,
    /// How many times this job has been started since it was last queued by hand.
    pub attempts: u8,
    /// When the original page cannot provide the track (DRM, preview-only),
    /// the matching upload found elsewhere that is downloaded instead.
    pub resolved: Option<String>,
    /// Who and what the track is, as the original page described it. Kept so
    /// a track fetched from elsewhere is still tagged as the track asked for.
    pub identity: Option<TrackMeta>,
    /// Why the track came from elsewhere: "drm" or "preview". Shown as a badge.
    pub workaround: Option<String>,
    /// A short line about what is happening right now, shown under the progress bar.
    pub note: Option<String>,
    pub created: u64,
    pub finished: Option<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub url: String,
    pub site: String,
    pub title: String,
    pub uploader: Option<String>,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Cancel,
    Pause,
}

enum Outcome {
    Stopped(Stop),
    Failed(String),
}

fn fail(msg: impl Into<String>) -> Outcome {
    Outcome::Failed(msg.into())
}

struct Running {
    stop: watch::Sender<Option<Stop>>,
    pid: Option<u32>,
}

pub struct Dirs {
    pub work: PathBuf,
    pub bin: PathBuf,
    pub settings_file: PathBuf,
    pub jobs_file: PathBuf,
}

pub struct Manager {
    pub app: AppHandle,
    pub http: reqwest::Client,
    pub dirs: Dirs,
    settings: RwLock<Settings>,
    jobs: Mutex<Vec<Job>>,
    running: Mutex<HashMap<String, Running>>,
    dirty: AtomicBool,
    finished_batch: AtomicUsize,
    seq: AtomicU64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn kill_tree(pid: u32) {
    // yt-dlp runs ffmpeg (and, as a packaged binary, a second copy of itself)
    // as children, so the whole group has to go.
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000)
            .output();
    }
}

/// What the user sees while a track is fetched from elsewhere, stage by stage.
fn workaround_note(reason: &str, stage: u8) -> &'static str {
    let note = workaround_note_text(reason, stage);
    #[cfg(debug_assertions)]
    eprintln!("[note] {note}");
    note
}

fn workaround_note_text(reason: &str, stage: u8) -> &'static str {
    match (reason, stage) {
        ("drm", 0) => "DRM protection detected",
        ("drm", 1) => "Finding an unprotected copy…",
        ("drm", _) => "Downloading an unprotected copy",
        (_, 0) => "Only a preview is available here",
        (_, 1) => "Finding the full track…",
        (_, _) => "Downloading the full track",
    }
}

/// SoundCloud serves only a 30-second clip of subscriber-only tracks.
fn is_preview(info: &Value) -> bool {
    info.get("format_id").and_then(|f| f.as_str()).is_some_and(|f| f.contains("preview"))
}

/// Failures worth one more automatic go: the network or the site hiccuped.
fn is_transient(message: &str) -> bool {
    message.starts_with("Could not reach the site") || message.starts_with("The site did not respond in time")
}

// ------------------------------------------------------------ URL handling

pub fn site_of(url: &str) -> &'static str {
    let host = tauri::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.trim_end_matches('.').to_lowercase()))
        .unwrap_or_default();
    // The domain itself or one of its subdomains, so "notyoutube.com" does not count.
    let on = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    if on("youtube.com") || on("youtu.be") || on("youtube-nocookie.com") {
        "youtube"
    } else if on("soundcloud.com") {
        "soundcloud"
    } else {
        "web"
    }
}

/// Whether a URL points at many items (playlist, set, channel) rather than one.
pub fn is_collection(url: &str) -> bool {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_lowercase();
    let path = path.split(['?', '#']).next().unwrap_or("");
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match site_of(url) {
        "youtube" => match segs.first().copied() {
            Some("playlist") | Some("channel") | Some("c") | Some("user") | Some("browse") => true,
            Some(s) => s.starts_with('@'),
            None => false,
        },
        "soundcloud" if host == "soundcloud.com" || host == "www.soundcloud.com" || host == "m.soundcloud.com" => {
            const TABS: [&str; 7] = ["tracks", "albums", "sets", "likes", "reposts", "popular-tracks", "spotlight"];
            match segs.len() {
                0 => false,
                1 => !["discover", "search", "upload", "you", "stream"].contains(&segs[0]),
                2 => TABS.contains(&segs[1]),
                _ => segs[1] == "sets",
            }
        }
        _ => false,
    }
}

/// A readable stand-in for an item whose title is not known yet:
/// ".../artist/some-track-name" becomes "some track name".
fn title_from_url(url: &str) -> String {
    let slug = url.split(['?', '#']).next().unwrap_or(url).trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let words = slug.replace(['-', '_'], " ");
    // Only SoundCloud links end in the track's name.
    if site_of(url) != "soundcloud" || words.trim().is_empty() || words.contains(':') || words.contains('%') || !words.chars().any(|c| c.is_alphabetic()) {
        url.to_string()
    } else {
        words.trim().to_string()
    }
}

fn normalize_url(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || t.contains(char::is_whitespace) {
        return None;
    }
    let url = if t.starts_with("http://") || t.starts_with("https://") {
        t.to_string()
    } else if t.contains('.') && !t.starts_with('-') {
        format!("https://{t}")
    } else {
        return None;
    };
    // A bare YouTube channel lists its tabs, not its uploads.
    if site_of(&url) == "youtube" {
        let path = url.split("://").nth(1).and_then(|r| r.split_once('/')).map(|x| x.1).unwrap_or("");
        let path = path.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
        if path.starts_with('@') && !path.contains('/') {
            return Some(format!("{}/videos", url.split(['?', '#']).next().unwrap_or(&url).trim_end_matches('/')));
        }
    }
    Some(url)
}

// -------------------------------------------------------------- file names

fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim_matches(|c: char| c == '.' || c == ' ' || c == '-' || c == '_');
    trimmed.chars().take(180).collect::<String>().trim().to_string()
}

fn fill_template(template: &str, meta: &TrackMeta) -> String {
    let get = |v: &Option<String>| v.clone().unwrap_or_default();
    template
        .replace("{albumartist}", &get(&meta.album_artist.clone().or_else(|| meta.artist.clone())))
        .replace("{artist}", &get(&meta.artist))
        .replace("{title}", &get(&meta.title))
        .replace("{album}", &get(&meta.album))
        .replace("{year}", &meta.year.map(|y| y.to_string()).unwrap_or_default())
        .replace("{track}", &meta.track.map(|t| format!("{t:02}")).unwrap_or_default())
}

/// The file name (without extension) for a track. Only the part of the
/// template after its last "/" names the file; the rest names folders.
pub fn render_filename(template: &str, meta: &TrackMeta, fallback: &str) -> String {
    let template = template.replace('\\', "/");
    let last = template.rsplit('/').next().unwrap_or(&template);
    let name = sanitize_filename(&fill_template(last, meta));
    if name.is_empty() || meta.title.is_none() {
        let fb = sanitize_filename(fallback);
        if fb.is_empty() { "Untitled".into() } else { fb }
    } else {
        name
    }
}

/// Sub-folders named by the template, e.g. "{albumartist}/{album}/{title}"
/// gives "Artist/Album". A folder whose tags are missing is simply skipped.
pub fn render_folders(template: &str, meta: &TrackMeta) -> PathBuf {
    let template = template.replace('\\', "/");
    let mut parts: Vec<&str> = template.split('/').collect();
    parts.pop();
    parts
        .into_iter()
        .map(|p| sanitize_filename(&fill_template(p, meta)))
        .filter(|p| !p.is_empty())
        .collect()
}

/// `dir/stem.ext`, or `dir/stem (2).ext` and so on if that is taken.
fn unique_path(dir: &Path, stem: &str, ext: &str, ignore: Option<&Path>) -> PathBuf {
    let mut candidate = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while candidate.exists() && Some(candidate.as_path()) != ignore {
        candidate = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    candidate
}

async fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if tokio::fs::rename(from, to).await.is_ok() {
        return Ok(());
    }
    // Different volume: copy, then remove the original.
    tokio::fs::copy(from, to).await?;
    tokio::fs::remove_file(from).await
}

// --------------------------------------------------------- format handling

struct YtFormat {
    selector: &'static str,
    audio_format: &'static str,
    quality: Option<String>,
}

/// How OGG files get made with the ffmpeg at hand.
#[derive(Clone, Copy, PartialEq)]
enum Ogg {
    Vorbis,
    /// No Vorbis encoder in this ffmpeg: Opus in an Ogg container instead.
    Opus,
    Unavailable,
}

async fn ogg_mode(ffmpeg: &Path) -> Ogg {
    if tools::has_encoder(ffmpeg, "libvorbis").await {
        Ogg::Vorbis
    } else if tools::has_encoder(ffmpeg, "libopus").await {
        Ogg::Opus
    } else {
        Ogg::Unavailable
    }
}

const NO_OGG: &str = "This copy of ffmpeg cannot write OGG. Install the Dubplate-managed ffmpeg under Settings → Tools, or pick another format.";

fn bitrate(quality: &str, best: &str) -> String {
    match quality {
        "best" | "" => best.to_string(),
        q => q.to_string(),
    }
}

fn yt_format(format: &str, quality: &str, ogg: Ogg) -> YtFormat {
    // Asking for a source that already holds the target codec lets yt-dlp copy
    // the stream instead of re-encoding it: faster, and no quality loss.
    const ANY: &str = "bestaudio/best";
    const AAC_FIRST: &str = "bestaudio[ext=m4a]/bestaudio/best";
    match format {
        "flac" => YtFormat { selector: ANY, audio_format: "flac", quality: None },
        "wav" => YtFormat { selector: ANY, audio_format: "wav", quality: None },
        "m4a" => YtFormat { selector: AAC_FIRST, audio_format: "m4a", quality: Some(format!("{}K", bitrate(quality, "256"))) },
        "aac" => YtFormat { selector: AAC_FIRST, audio_format: "aac", quality: Some(format!("{}K", bitrate(quality, "256"))) },
        // YouTube's best audio is already Opus, so this is usually a straight copy.
        "ogg" if ogg == Ogg::Opus => YtFormat {
            selector: "bestaudio[acodec=opus]/bestaudio/best",
            audio_format: "opus",
            quality: Some(format!("{}K", bitrate(quality, "192"))),
        },
        "ogg" => YtFormat { selector: ANY, audio_format: "vorbis", quality: Some(format!("{}K", bitrate(quality, "256"))) },
        _ => YtFormat { selector: ANY, audio_format: "mp3", quality: Some(format!("{}K", bitrate(quality, "320"))) },
    }
}

fn ffmpeg_codec_args(format: &str, quality: &str, ogg: Ogg) -> Vec<String> {
    let args: Vec<&str> = match format {
        "flac" => vec!["-c:a", "flac"],
        "wav" => vec!["-c:a", "pcm_s16le"],
        "m4a" => vec!["-c:a", "aac", "-b:a", "@256", "-movflags", "+faststart"],
        "aac" => vec!["-c:a", "aac", "-b:a", "@256"],
        "ogg" if ogg == Ogg::Opus => vec!["-c:a", "libopus", "-b:a", "@192"],
        "ogg" => vec!["-c:a", "libvorbis", "-b:a", "@256"],
        _ => vec!["-c:a", "libmp3lame", "-b:a", "@320", "-id3v2_version", "3"],
    };
    args.into_iter()
        .map(|a| match a.strip_prefix('@') {
            Some(best) => format!("{}k", bitrate(quality, best)),
            None => a.to_string(),
        })
        .collect()
}

fn friendly_error(line: &str) -> String {
    let msg = line.trim().trim_start_matches("ERROR:").trim();
    let lower = msg.to_lowercase();
    if lower.contains("403") || lower.contains("nsig") || lower.contains("requested format is not available") {
        "The site refused this download, which usually means yt-dlp is out of date. Update it under Settings → Tools, then retry.".into()
    } else if lower.contains("drm") {
        "This track is DRM-protected by the site, so it cannot be downloaded.".into()
    } else if lower.contains("sign in to confirm") || lower.contains("age") && lower.contains("restricted") {
        "This item needs a signed-in browser. Pick one under Settings → Downloads → Use cookies from.".into()
    } else if lower.contains("private video") || lower.contains("video unavailable") || lower.contains("404") {
        format!("Not available: {msg}")
    } else if lower.contains("unable to download") && lower.contains("webpage")
        || ["getaddrinfo", "no route to host", "failed to establish a new connection", "network is unreachable", "connection refused"]
            .iter()
            .any(|sign| lower.contains(sign))
    {
        "Could not reach the site. Check your connection and retry.".into()
    } else if lower.contains("timed out") || lower.contains("connection reset") || lower.contains("temporary failure") {
        "The site did not respond in time. Check your connection and retry.".into()
    } else {
        msg.chars().take(300).collect()
    }
}

/// Look a song up by name on YouTube or SoundCloud.
pub async fn search_tracks(bin_dir: &Path, query: &str, site: &str) -> Result<Vec<SearchHit>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(vec![]);
    }
    let prefix = if site == "soundcloud" { "scsearch" } else { "ytsearch" };
    // The prefix also keeps a query that starts with "-" from being read as an option.
    search_spec(bin_dir, &format!("{prefix}10:{query}")).await
}

/// List what yt-dlp finds for a search ("ytsearch10:…") or a search page URL,
/// without downloading anything.
pub async fn search_spec(bin_dir: &Path, spec: &str) -> Result<Vec<SearchHit>, String> {
    let json = ytdlp_json(bin_dir, &["--flat-playlist", "--playlist-end", "12"], spec).await?;
    let text = |v: &Value, k: &str| v.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(|x| x.to_string());
    Ok(json
        .get("entries")
        .and_then(|e| e.as_array())
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let url = text(e, "webpage_url").or_else(|| text(e, "url"))?;
            let thumbs: Vec<String> = e
                .get("thumbnails")
                .and_then(|t| t.as_array())
                .into_iter()
                .flatten()
                .filter_map(|t| text(t, "url"))
                .collect();
            Some(SearchHit {
                site: site_of(&url).to_string(),
                title: text(e, "title").unwrap_or_else(|| title_from_url(&url)),
                uploader: text(e, "uploader").or_else(|| text(e, "channel")),
                duration: e.get("duration").and_then(|d| d.as_f64()),
                // SoundCloud lists a full-size original last; a small rendition is plenty here.
                thumbnail: thumbs
                    .iter()
                    .find(|t| t.contains("t300x300") || t.contains("-large."))
                    .or(thumbs.last())
                    .cloned(),
                url,
            })
        })
        .collect())
}

/// Everything yt-dlp knows about one page, even when it cannot be downloaded
/// (DRM, region locks): the title, artist and length are still readable.
pub async fn fetch_info(bin_dir: &Path, url: &str) -> Result<Value, String> {
    ytdlp_json(bin_dir, &["--no-playlist", "--ignore-no-formats-error", "--skip-download"], url).await
}

async fn ytdlp_json(bin_dir: &Path, args: &[&str], target: &str) -> Result<Value, String> {
    let (ytdlp, _) = tools::find("yt-dlp", bin_dir)
        .ok_or("yt-dlp is not installed. Open Settings → Tools to install it.")?;
    let mut cmd = tools::command(&ytdlp);
    cmd.args(["-J", "--no-warnings", "--no-update", "--encoding", "utf-8"]).args(args);
    if let Some(rt) = tools::js_runtime(bin_dir) {
        cmd.args(["--js-runtimes", &rt]);
    }
    // "--" ends the options, so a target can never be read as one.
    cmd.arg("--").arg(target).env("PYTHONIOENCODING", "utf-8");
    let out = tokio::time::timeout(Duration::from_secs(45), cmd.output())
        .await
        .map_err(|_| "The search timed out. Check your connection and try again.")?
        .map_err(|e| format!("Could not start yt-dlp: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or("The search failed.");
        return Err(friendly_error(line));
    }
    serde_json::from_slice(&out.stdout).map_err(|_| "Unexpected data from yt-dlp".to_string())
}

// ------------------------------------------------------------------ manager

impl Manager {
    pub fn new(app: AppHandle, dirs: Dirs, default_output: PathBuf) -> Self {
        let settings = Settings::load(&dirs.settings_file, default_output);
        let mut jobs: Vec<Job> = std::fs::read(&dirs.jobs_file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        jobs.retain(|j| j.kind != "playlist");
        for j in &mut jobs {
            // Anything that was mid-flight when the app last closed can be resumed.
            if j.status.active() || j.status == Status::Queued {
                j.status = Status::Paused;
                j.speed = None;
                j.eta = None;
            }
        }
        let http = reqwest::Client::builder()
            .user_agent(meta::USER_AGENT)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self {
            app,
            http,
            dirs,
            settings: RwLock::new(settings),
            jobs: Mutex::new(jobs),
            running: Mutex::new(HashMap::new()),
            dirty: AtomicBool::new(false),
            finished_batch: AtomicUsize::new(0),
            seq: AtomicU64::new(0),
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub fn set_settings(self: &Arc<Self>, mut new: Settings) -> Result<Settings, String> {
        new.sanitize();
        if new.output_dir.trim().is_empty() {
            new.output_dir = self.settings().output_dir;
        }
        new.save(&self.dirs.settings_file).map_err(|e| format!("Could not save settings: {e}"))?;
        *self.settings.write().unwrap() = new.clone();
        self.pump();
        Ok(new)
    }

    pub fn list(&self) -> Vec<Job> {
        self.jobs.lock().unwrap().clone()
    }

    pub fn get(&self, id: &str) -> Option<Job> {
        self.jobs.lock().unwrap().iter().find(|j| j.id == id).cloned()
    }

    fn new_id(&self) -> String {
        format!("{:x}{:03x}", now_ms(), self.seq.fetch_add(1, Ordering::Relaxed) & 0xfff)
    }

    fn emit(&self, job: &Job) {
        let _ = self.app.emit("job-update", job);
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut Job)) -> Option<Job> {
        let mut jobs = self.jobs.lock().unwrap();
        let job = jobs.iter_mut().find(|j| j.id == id)?;
        f(job);
        Some(job.clone())
    }

    /// Change a job, tell the UI, and schedule a save.
    pub fn mutate(&self, id: &str, f: impl FnOnce(&mut Job)) -> Option<Job> {
        let job = self.update(id, f)?;
        self.emit(&job);
        self.dirty.store(true, Ordering::Relaxed);
        Some(job)
    }

    fn push(&self, job: Job) {
        self.jobs.lock().unwrap().push(job.clone());
        self.emit(&job);
        self.dirty.store(true, Ordering::Relaxed);
    }

    fn drop_job(&self, id: &str) {
        self.jobs.lock().unwrap().retain(|j| j.id != id);
        let _ = self.app.emit("job-removed", id);
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn save(&self) {
        let jobs: Vec<Job> = self.list().into_iter().filter(|j| j.kind != "playlist").collect();
        let file = &self.dirs.jobs_file;
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = file.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec(&jobs).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(tmp, file);
        }
    }

    /// Periodic saving, plus keeping an app-managed yt-dlp current.
    pub fn start_background(self: &Arc<Self>) {
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                if me.dirty.swap(false, Ordering::Relaxed) {
                    let saver = me.clone();
                    let _ = tokio::task::spawn_blocking(move || saver.save()).await;
                }
            }
        });
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(6)).await;
            let Some((path, true)) = tools::find("yt-dlp", &me.dirs.bin) else { return };
            let (Some(current), Some(latest)) =
                (tools::version("yt-dlp", &path).await, tools::latest_ytdlp(&me.http).await)
            else {
                return;
            };
            if current != latest && tools::install(&me.app, &me.http, &me.dirs.bin, "yt-dlp").await.is_ok() {
                let _ = me.app.emit("tools-changed", ());
            }
        });
    }

    pub fn shutdown(&self) {
        for (_, run) in self.running.lock().unwrap().drain() {
            if let Some(pid) = run.pid {
                kill_tree(pid);
            }
        }
        {
            let mut jobs = self.jobs.lock().unwrap();
            for j in jobs.iter_mut().filter(|j| j.status.active() || j.status == Status::Queued) {
                j.status = Status::Paused;
            }
        }
        self.save();
    }

    // ------------------------------------------------------------ adding

    pub fn add_urls(self: &Arc<Self>, urls: Vec<String>, format: Option<String>, quality: Option<String>) -> usize {
        let settings = self.settings();
        let format = format.filter(|f| crate::settings::FORMATS.contains(&f.as_str())).unwrap_or(settings.format);
        let quality = quality.unwrap_or(settings.quality);
        let mut added = 0;
        for url in urls.iter().filter_map(|u| normalize_url(u)) {
            let duplicate = self.jobs.lock().unwrap().iter().any(|j| {
                j.source == url && j.format == format && !matches!(j.status, Status::Done | Status::Failed | Status::Canceled)
            });
            if duplicate {
                continue;
            }
            added += 1;
            let collection = is_collection(&url);
            let job = Job {
                id: self.new_id(),
                kind: if collection { "playlist" } else { "download" }.into(),
                site: site_of(&url).into(),
                title: if collection { "Reading playlist…".into() } else { url.clone() },
                source: url,
                format: format.clone(),
                quality: quality.clone(),
                status: if collection { Status::Fetching } else { Status::Queued },
                created: now_ms(),
                ..Default::default()
            };
            self.push(job.clone());
            if collection {
                let me = self.clone();
                tauri::async_runtime::spawn(async move { me.expand_playlist(job).await });
            }
        }
        self.pump();
        added
    }

    async fn expand_playlist(self: Arc<Self>, placeholder: Job) {
        let result: Result<Vec<Job>, String> = async {
            let (ytdlp, _) = tools::find("yt-dlp", &self.dirs.bin)
                .ok_or("yt-dlp is not installed. Open Settings → Tools to install it.")?;
            let settings = self.settings();
            let mut cmd = tools::command(&ytdlp);
            cmd.args(["--flat-playlist", "-J", "--no-warnings", "--no-update", "--encoding", "utf-8"]);
            let cookie_args = {
                let (s, scratch) = (settings.clone(), self.dirs.work.clone());
                let _ = std::fs::create_dir_all(&scratch);
                tokio::task::spawn_blocking(move || cookies::prepare(&s, &scratch)).await.map_err(|e| e.to_string())??
            };
            cmd.args(&cookie_args.args);
            cmd.arg(&placeholder.source).env("PYTHONIOENCODING", "utf-8");
            let out = tokio::time::timeout(Duration::from_secs(180), cmd.output())
                .await
                .map_err(|_| "Reading the playlist timed out")?
                .map_err(|e| format!("Could not start yt-dlp: {e}"))?;
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                let line = err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or("Could not read this playlist");
                return Err(friendly_error(line));
            }
            let json: Value = serde_json::from_slice(&out.stdout).map_err(|_| "Unexpected playlist data")?;
            let text = |v: &Value, k: &str| v.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(|x| x.to_string());

            let Some(entries) = json.get("entries").and_then(|e| e.as_array()) else {
                // It was a single item after all.
                return Ok(vec![Job {
                    kind: "download".into(),
                    title: text(&json, "title").unwrap_or_else(|| placeholder.source.clone()),
                    ..placeholder.clone()
                }]);
            };
            let album = text(&json, "title");
            Ok(entries
                .iter()
                .filter_map(|e| {
                    let url = text(e, "url").or_else(|| text(e, "webpage_url"))?;
                    Some(Job {
                        id: self.new_id(),
                        kind: "download".into(),
                        site: site_of(&url).into(),
                        title: text(e, "title").unwrap_or_else(|| title_from_url(&url)),
                        artist: text(e, "uploader").or_else(|| text(e, "channel")),
                        album: album.clone(),
                        thumbnail: e
                            .get("thumbnails")
                            .and_then(|t| t.as_array())
                            .and_then(|t| t.last())
                            .and_then(|t| text(t, "url")),
                        duration: e.get("duration").and_then(|d| d.as_f64()),
                        source: url,
                        format: placeholder.format.clone(),
                        quality: placeholder.quality.clone(),
                        status: Status::Queued,
                        created: now_ms(),
                        ..Default::default()
                    })
                })
                .take(2000)
                .collect())
        }
        .await;

        // The placeholder may have been removed while the playlist was read.
        if self.get(&placeholder.id).is_none() {
            return;
        }
        match result {
            Ok(items) if !items.is_empty() => {
                self.drop_job(&placeholder.id);
                for mut job in items {
                    job.status = Status::Queued;
                    if job.id == placeholder.id {
                        job.id = self.new_id();
                    }
                    self.push(job);
                }
                self.pump();
            }
            Ok(_) => {
                self.mutate(&placeholder.id, |j| {
                    j.status = Status::Failed;
                    j.title = j.source.clone();
                    j.error = Some("This playlist is empty or private.".into());
                });
            }
            Err(e) => {
                self.mutate(&placeholder.id, |j| {
                    j.status = Status::Failed;
                    j.title = j.source.clone();
                    j.error = Some(e);
                });
            }
        }
    }

    /// Look a song up by name on YouTube or SoundCloud.
    pub async fn search(&self, query: &str, site: &str) -> Result<Vec<SearchHit>, String> {
        search_tracks(&self.dirs.bin, query, site).await
    }

    /// Add local files, either to convert them or just to tag them in the Library.
    pub async fn add_files(self: &Arc<Self>, paths: Vec<String>, convert: bool, format: Option<String>, quality: Option<String>) -> usize {
        const AUDIO: [&str; 14] = ["mp3", "flac", "wav", "m4a", "aac", "ogg", "opus", "aiff", "aif", "wma", "mp4", "webm", "mka", "oga"];
        let settings = self.settings();
        let format = format.filter(|f| crate::settings::FORMATS.contains(&f.as_str())).unwrap_or(settings.format);
        let quality = quality.unwrap_or(settings.quality);
        let mut added = 0;
        for raw in paths {
            let path = PathBuf::from(&raw);
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !path.is_file() || !AUDIO.contains(&ext.as_str()) {
                continue;
            }
            let already = self.jobs.lock().unwrap().iter().any(|j| {
                if convert {
                    j.kind == "convert" && j.source == raw && j.format == format && (j.status.active() || j.status == Status::Queued)
                } else {
                    j.status == Status::Done && j.output.as_deref() == Some(raw.as_str())
                }
            });
            if already {
                continue;
            }
            let probe = path.clone();
            let info = tokio::task::spawn_blocking(move || tags::read(&probe)).await.ok().and_then(|r| r.ok()).unwrap_or_default();
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let size = std::fs::metadata(&path).map(|m| m.len()).ok();
            added += 1;
            self.push(Job {
                id: self.new_id(),
                kind: if convert { "convert" } else { "import" }.into(),
                site: "file".into(),
                title: info.meta.title.clone().unwrap_or(stem),
                artist: info.meta.artist.clone(),
                album: info.meta.album.clone(),
                thumbnail: info.cover.clone(),
                duration: info.duration,
                format: if convert { format.clone() } else { ext },
                quality: quality.clone(),
                status: if convert { Status::Queued } else { Status::Done },
                progress: if convert { 0.0 } else { 1.0 },
                output: (!convert).then(|| raw.clone()),
                size: if convert { None } else { size },
                cleared: !convert,
                created: now_ms(),
                finished: (!convert).then(now_ms),
                source: raw,
                ..Default::default()
            });
        }
        self.pump();
        added
    }

    // ----------------------------------------------------------- control

    fn signal(&self, id: &str, stop: Stop) -> bool {
        match self.running.lock().unwrap().get(id) {
            Some(run) => {
                let _ = run.stop.send(Some(stop));
                true
            }
            None => false,
        }
    }

    fn clean_work(&self, id: &str) {
        let _ = std::fs::remove_dir_all(self.dirs.work.join(id));
    }

    pub fn pause(&self, id: &str) {
        if !self.signal(id, Stop::Pause) {
            self.mutate(id, |j| {
                if j.status == Status::Queued {
                    j.status = Status::Paused;
                }
            });
        }
    }

    pub fn resume(self: &Arc<Self>, id: &str) {
        self.mutate(id, |j| {
            if matches!(j.status, Status::Paused | Status::Failed | Status::Canceled) {
                if j.status != Status::Paused {
                    j.progress = 0.0;
                    j.downloaded = 0;
                }
                j.status = Status::Queued;
                j.error = None;
                j.cleared = false;
                j.attempts = 0;
            }
        });
        self.pump();
    }

    pub fn cancel(&self, id: &str) {
        if !self.signal(id, Stop::Cancel) {
            self.mutate(id, |j| {
                if matches!(j.status, Status::Queued | Status::Paused) {
                    j.status = Status::Canceled;
                }
            });
            self.clean_work(id);
        }
    }

    pub fn remove(&self, id: &str) {
        self.signal(id, Stop::Cancel);
        self.drop_job(id);
        if !self.running.lock().unwrap().contains_key(id) {
            self.clean_work(id);
        }
    }

    /// Tidy the Downloads list: finished items stay in the Library, failed
    /// and cancelled ones are dropped.
    pub fn clear_finished(&self) {
        let ids: Vec<(String, Status)> = self.list().into_iter().map(|j| (j.id, j.status)).collect();
        for (id, status) in ids {
            match status {
                Status::Done => {
                    self.mutate(&id, |j| j.cleared = true);
                }
                Status::Failed | Status::Canceled => self.drop_job(&id),
                _ => {}
            }
        }
    }

    pub fn pause_all(&self) {
        for j in self.list() {
            if j.kind != "playlist" && (j.status.active() || j.status == Status::Queued) {
                self.pause(&j.id);
            }
        }
    }

    pub fn resume_all(self: &Arc<Self>) {
        for j in self.list() {
            if j.status == Status::Paused {
                self.mutate(&j.id, |j| j.status = Status::Queued);
            }
        }
        self.pump();
    }

    // ------------------------------------------------------------ running

    /// Start queued jobs until the concurrency limit is reached.
    pub fn pump(self: &Arc<Self>) {
        let max = self.settings().max_concurrent;
        loop {
            let next = {
                let mut jobs = self.jobs.lock().unwrap();
                let active = jobs.iter().filter(|j| j.kind != "playlist" && j.status.active()).count();
                if active >= max {
                    return;
                }
                match jobs.iter_mut().find(|j| j.status == Status::Queued && j.kind != "playlist") {
                    Some(j) => {
                        j.status = Status::Fetching;
                        j.error = None;
                        j.attempts = j.attempts.saturating_add(1);
                        j.clone()
                    }
                    None => break,
                }
            };
            self.emit(&next);
            let (tx, rx) = watch::channel(None);
            self.running.lock().unwrap().insert(next.id.clone(), Running { stop: tx, pid: None });
            let me = self.clone();
            tauri::async_runtime::spawn(async move { me.run(next, rx).await });
        }

        // Queue drained: one notification for the whole batch.
        let idle = !self.jobs.lock().unwrap().iter().any(|j| j.status.active() || j.status == Status::Queued);
        if idle {
            let done = self.finished_batch.swap(0, Ordering::Relaxed);
            if done > 0 && self.settings().notify {
                use tauri_plugin_notification::NotificationExt;
                let body = if done == 1 { "1 track is ready.".to_string() } else { format!("{done} tracks are ready.") };
                let _ = self.app.notification().builder().title("Dubplate").body(body).show();
            }
        }
    }

    async fn run(self: Arc<Self>, job: Job, mut stop: watch::Receiver<Option<Stop>>) {
        let result = if job.kind == "convert" {
            self.run_convert(&job, &mut stop).await
        } else {
            self.run_download(&job, &mut stop).await
        };
        self.running.lock().unwrap().remove(&job.id);
        match result {
            Ok(()) => {
                self.finished_batch.fetch_add(1, Ordering::Relaxed);
                self.clean_work(&job.id);
            }
            Err(Outcome::Stopped(Stop::Pause)) => {
                self.mutate(&job.id, |j| {
                    j.status = Status::Paused;
                    j.note = None;
                    j.speed = None;
                    j.eta = None;
                });
            }
            Err(Outcome::Stopped(Stop::Cancel)) => {
                self.mutate(&job.id, |j| {
                    j.status = Status::Canceled;
                    j.note = None;
                    j.speed = None;
                    j.eta = None;
                });
                self.clean_work(&job.id);
            }
            Err(Outcome::Failed(msg)) => {
                // A timeout or dropped connection gets up to two automatic
                // retries, a few seconds apart, before it is reported.
                let attempts = self.get(&job.id).map(|j| j.attempts).unwrap_or(u8::MAX);
                if is_transient(&msg) && attempts < 3 {
                    self.mutate(&job.id, |j| {
                        j.status = Status::Queued;
                        j.note = None;
                        j.progress = 0.0;
                        j.speed = None;
                        j.eta = None;
                    });
                    self.clean_work(&job.id);
                    tokio::time::sleep(Duration::from_secs(4)).await;
                } else {
                    self.mutate(&job.id, |j| {
                        j.status = Status::Failed;
                        j.note = None;
                        j.error = Some(msg);
                        j.speed = None;
                        j.eta = None;
                    });
                    self.clean_work(&job.id);
                }
            }
        }
        self.pump();
    }

    /// Run a child process, feeding each output line to `on_line`
    /// (the flag is true for stderr) and honouring pause/cancel.
    async fn run_process(
        &self,
        id: &str,
        mut cmd: tokio::process::Command,
        stop: &mut watch::Receiver<Option<Stop>>,
        mut on_line: impl FnMut(&str, bool),
    ) -> Result<std::process::ExitStatus, Outcome> {
        if let Some(s) = *stop.borrow() {
            return Err(Outcome::Stopped(s));
        }
        cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd.spawn().map_err(|e| fail(format!("Could not start the tool: {e}")))?;
        let pid = child.id();
        if let Some(run) = self.running.lock().unwrap().get_mut(id) {
            run.pid = pid;
        }
        let mut out = BufReader::new(child.stdout.take().expect("piped stdout")).split(b'\n');
        let mut err = BufReader::new(child.stderr.take().expect("piped stderr")).split(b'\n');
        let (mut out_open, mut err_open) = (true, true);
        while out_open || err_open {
            tokio::select! {
                seg = out.next_segment(), if out_open => match seg {
                    Ok(Some(bytes)) => on_line(String::from_utf8_lossy(&bytes).trim_end_matches('\r'), false),
                    _ => out_open = false,
                },
                seg = err.next_segment(), if err_open => match seg {
                    Ok(Some(bytes)) => {
                        let text = String::from_utf8_lossy(&bytes);
                        // Development builds echo the tools' own messages to the terminal.
                        #[cfg(debug_assertions)]
                        eprintln!("[{id}] {}", text.trim_end());
                        on_line(text.trim_end_matches('\r'), true)
                    }
                    _ => err_open = false,
                },
                changed = stop.changed() => {
                    let requested = *stop.borrow();
                    if let Some(s) = requested {
                        if let Some(pid) = pid {
                            kill_tree(pid);
                        }
                        let _ = child.kill().await;
                        return Err(Outcome::Stopped(s));
                    }
                    if changed.is_err() {
                        // Sender gone: the job was dropped from the running table.
                        let _ = child.kill().await;
                        return Err(Outcome::Stopped(Stop::Cancel));
                    }
                }
            }
        }
        child.wait().await.map_err(|e| fail(e.to_string()))
    }

    fn stopped(stop: &watch::Receiver<Option<Stop>>) -> Result<(), Outcome> {
        match *stop.borrow() {
            Some(s) => Err(Outcome::Stopped(s)),
            None => Ok(()),
        }
    }

    /// Download one page's audio into `work` and convert it, without tagging.
    #[allow(clippy::too_many_arguments)]
    async fn fetch_audio(
        &self,
        job: &Job,
        url: &str,
        identity: Option<&TrackMeta>,
        work: &Path,
        ytdlp: &Path,
        ffmpeg: &Path,
        settings: &Settings,
        ogg: Ogg,
        stop: &mut watch::Receiver<Option<Stop>>,
    ) -> Result<(Value, PathBuf), Outcome> {
        let id = job.id.as_str();
        let fmt = yt_format(&job.format, &job.quality, ogg);
        let mut cmd = tools::command(ytdlp);
        cmd.args([
            "--no-playlist", "--newline", "--progress", "--color", "never", "--no-simulate",
            "--no-update", "--no-mtime", "--encoding", "utf-8",
            "--progress-template",
            "download:DP|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s",
            "--progress-template", "postprocess:PP|%(progress.status)s|%(progress.postprocessor)s",
            "--print",
            "before_dl:META|%(.{id,title,uploader,channel,artist,track,album,album_artist,release_year,release_date,upload_date,duration,thumbnail,webpage_url,genres,track_number,format_id})j",
            "--print", "after_move:FILE|%(filepath)s",
            "-f", fmt.selector, "-x", "--audio-format", fmt.audio_format,
            "--concurrent-fragments", "4", "--retries", "5", "--fragment-retries", "5", "--socket-timeout", "20",
            "-o", "%(id)s.%(ext)s",
        ]);
        cmd.arg("-P").arg(work).arg("--ffmpeg-location").arg(ffmpeg);
        if let Some(q) = &fmt.quality {
            cmd.args(["--audio-quality", q]);
        }
        if settings.sponsorblock {
            cmd.args(["--sponsorblock-remove", "music_offtopic"]);
        }
        // Kept until yt-dlp is done: a cookie file Dubplate wrote is deleted with it.
        let cookie_args = {
            let (s, scratch) = (settings.clone(), work.to_path_buf());
            tokio::task::spawn_blocking(move || cookies::prepare(&s, &scratch))
                .await
                .map_err(|e| fail(e.to_string()))?
                .map_err(fail)?
        };
        cmd.args(&cookie_args.args);
        if let Some(rt) = tools::js_runtime(&self.dirs.bin) {
            cmd.args(["--js-runtimes", &rt]);
        }
        cmd.arg("--").arg(url).env("PYTHONIOENCODING", "utf-8");

        let mut info: Option<Value> = None;
        let mut file: Option<PathBuf> = None;
        let mut last_error: Option<String> = None;
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        let num = |s: &str| s.trim().parse::<f64>().ok().filter(|n| n.is_finite());

        let status = self
            .run_process(id, cmd, stop, |line, _| {
                if let Some(json) = line.strip_prefix("META|") {
                    if let Ok(v) = serde_json::from_str::<Value>(json) {
                        // A track fetched from elsewhere keeps the name it was asked for by.
                        let guess = match identity {
                            Some(known) => meta::guess_from_source(&v).overlaid_with(known),
                            None => meta::guess_from_source(&v),
                        };
                        self.mutate(id, |j| {
                            j.title = guess.title.clone().unwrap_or_else(|| j.title.clone());
                            j.artist = guess.artist.clone().or(j.artist.take());
                            j.album = guess.album.clone();
                            j.duration = v.get("duration").and_then(|d| d.as_f64()).or(j.duration);
                            if let Some(t) = v.get("thumbnail").and_then(|t| t.as_str()) {
                                j.thumbnail = Some(t.to_string());
                            }
                            j.status = Status::Downloading;
                        });
                        info = Some(v);
                    }
                } else if let Some(rest) = line.strip_prefix("DP|") {
                    let p: Vec<&str> = rest.split('|').collect();
                    if p.len() < 5 {
                        return;
                    }
                    let downloaded = num(p[0]).unwrap_or(0.0);
                    let total = num(p[1]).or_else(|| num(p[2])).filter(|t| *t > 0.0);
                    let job = self.update(id, |j| {
                        j.status = Status::Downloading;
                        j.downloaded = downloaded as u64;
                        j.total = total.map(|t| t as u64);
                        j.speed = num(p[3]);
                        j.eta = num(p[4]).map(|e| e as u64);
                        if let Some(t) = total {
                            j.progress = ((downloaded / t).min(1.0) * 0.9) as f32;
                        }
                    });
                    if last_emit.elapsed() >= Duration::from_millis(140) {
                        last_emit = Instant::now();
                        if let Some(j) = job {
                            self.emit(&j);
                        }
                    }
                } else if let Some(rest) = line.strip_prefix("PP|") {
                    if rest.starts_with("started|") && !rest.ends_with("MoveFiles") {
                        self.mutate(id, |j| {
                            j.status = Status::Converting;
                            j.progress = j.progress.max(0.92);
                            j.speed = None;
                            j.eta = None;
                        });
                    }
                } else if let Some(path) = line.strip_prefix("FILE|") {
                    file = Some(PathBuf::from(path.trim()));
                } else if line.starts_with("ERROR") {
                    last_error = Some(friendly_error(line));
                }
            })
            .await?;

        if !status.success() {
            return Err(fail(last_error.unwrap_or_else(|| "The download failed. Updating yt-dlp under Settings → Tools often fixes this.".into())));
        }
        // What yt-dlp names the file it produces, and what it is saved as.
        let produced = if ogg == Ogg::Opus && job.format == "ogg" { "opus" } else { job.format.as_str() };
        let file = match file.filter(|f| f.is_file()) {
            Some(f) => f,
            None => std::fs::read_dir(work)
                .ok()
                .and_then(|d| {
                    d.filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .find(|p| p.extension().map(|x| x.to_string_lossy().to_lowercase()).as_deref() == Some(produced))
                })
                .ok_or_else(|| fail("The download finished but produced no audio file."))?,
        };
        Ok((info.unwrap_or(Value::Null), file))
    }

    /// Identify the track the job's page describes and find the same recording
    /// on YouTube. Records what it found on the job, so a retry goes straight there.
    async fn look_elsewhere(
        &self,
        job: &Job,
        reason: &'static str,
        settings: &Settings,
        stop: &mut watch::Receiver<Option<Stop>>,
    ) -> Result<Option<elsewhere::Found>, Outcome> {
        let id = job.id.as_str();
        self.mutate(id, |j| {
            j.status = Status::Fetching;
            j.progress = 0.0;
            j.downloaded = 0;
            j.total = None;
            j.speed = None;
            j.eta = None;
            j.workaround = Some(reason.to_string());
            j.note = Some(workaround_note(reason, 0).into());
        });
        // Long enough to read before the next stage replaces it.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        Self::stopped(stop)?;
        self.mutate(id, |j| j.note = Some(workaround_note(reason, 1).into()));
        let info = fetch_info(&self.dirs.bin, &job.source).await;
        Self::stopped(stop)?;
        let Ok(info) = info else { return Ok(None) };
        let found = elsewhere::find_on_youtube(&self.dirs.bin, &self.http, settings, &info).await;
        Self::stopped(stop)?;
        #[cfg(debug_assertions)]
        eprintln!(
            "[{id}] looked elsewhere for {:?}: {}",
            meta::guess_from_source(&info).title,
            found.as_ref().map(|f| format!("{} (score {:.2})", f.url, f.score)).unwrap_or_else(|| "no exact match".into())
        );
        if let Some(f) = &found {
            self.mutate(id, |j| {
                j.resolved = Some(f.url.clone());
                j.identity = Some(f.identity.clone());
                j.note = Some(workaround_note(reason, 2).into());
            });
        }
        Ok(found)
    }

    async fn run_download(&self, job: &Job, stop: &mut watch::Receiver<Option<Stop>>) -> Result<(), Outcome> {
        let settings = self.settings();
        let id = job.id.as_str();
        let missing = |tool: &str| fail(format!("{tool} is not installed. Open Settings → Tools to install it."));
        let (ytdlp, _) = tools::find("yt-dlp", &self.dirs.bin).ok_or_else(|| missing("yt-dlp"))?;
        let (ffmpeg, _) = tools::find("ffmpeg", &self.dirs.bin).ok_or_else(|| missing("ffmpeg"))?;
        let work = self.dirs.work.join(id);
        tokio::fs::create_dir_all(&work).await.map_err(|e| fail(format!("Cannot create a working folder: {e}")))?;

        let ogg = if job.format == "ogg" { ogg_mode(&ffmpeg).await } else { Ogg::Vorbis };
        if ogg == Ogg::Unavailable {
            return Err(fail(NO_OGG));
        }
        // Normally the page the user gave. A SoundCloud track that is DRM-protected,
        // or only offers a 30-second preview, is fetched from the same recording on
        // YouTube instead, found by `elsewhere`.
        let mut url = job.resolved.clone().unwrap_or_else(|| job.source.clone());
        let mut identity = job.identity.clone();
        let mut can_look_elsewhere = job.site == "soundcloud" && job.resolved.is_none();
        // Resuming a track that was already being fetched from elsewhere.
        if let (Some(_), Some(reason)) = (&job.resolved, job.workaround.as_deref()) {
            let note = workaround_note(if reason == "drm" { "drm" } else { "preview" }, 2);
            self.mutate(id, |j| j.note = Some(note.into()));
        }
        let mut spare: Vec<String> = Vec::new();
        let (info, file) = loop {
            let result = self
                .fetch_audio(job, &url, identity.as_ref(), &work, &ytdlp, &ffmpeg, &settings, ogg, stop)
                .await;
            let drm = matches!(&result, Err(Outcome::Failed(msg)) if msg.contains("DRM"));
            let preview = matches!(&result, Ok((info, _)) if is_preview(info));
            if can_look_elsewhere && (drm || preview) {
                can_look_elsewhere = false;
                let reason = if drm { "drm" } else { "preview" };
                match self.look_elsewhere(job, reason, &settings, stop).await? {
                    Some(found) => {
                        if let Ok((_, clip)) = &result {
                            let _ = tokio::fs::remove_file(clip).await;
                        }
                        url = found.url;
                        spare = found.others;
                        identity = Some(found.identity);
                        continue;
                    }
                    None if drm => {
                        return Err(fail("This track is DRM-protected on SoundCloud, and no exact copy of it could be found on YouTube."));
                    }
                    // Nothing better found: keep the preview, which is flagged below.
                    None => {
                        self.mutate(id, |j| j.workaround = None);
                        break result?;
                    }
                }
            }
            // YouTube occasionally refuses one particular upload; another exact copy will do.
            if matches!(&result, Err(Outcome::Failed(_))) && !spare.is_empty() {
                url = spare.remove(0);
                let next = url.clone();
                self.mutate(id, move |j| j.resolved = Some(next));
                continue;
            }
            break result?;
        };
        let ext = job.format.as_str();

        // ---- metadata
        self.mutate(id, |j| {
            j.status = Status::Tagging;
            j.progress = 0.96;
            j.speed = None;
            j.eta = None;
        });
        let base = match &identity {
            Some(known) => meta::guess_from_source(&info).overlaid_with(known),
            None => meta::guess_from_source(&info),
        };
        let duration = info.get("duration").and_then(|d| d.as_f64());
        let preview_only = is_preview(&info);
        let mut final_meta = base.clone();
        let mut matched: Option<(String, f32)> = None;
        let mut match_cover: Option<String> = None;
        if settings.auto_tag {
            if let Some(title) = base.title.clone() {
                let query = meta::Query { title, artist: base.artist.clone(), duration };
                let found = meta::search(&self.http, &settings, &query).await;
                Self::stopped(stop)?;
                if let Some(best) = meta::best_match(&found, settings.match_threshold) {
                    final_meta = base.overlaid_with(&best.meta);
                    matched = Some((best.source.clone(), best.score));
                    match_cover = best.cover_url.clone();
                }
            }
        }

        let mut cover: Option<Vec<u8>> = None;
        if settings.embed_cover {
            let thumb = info.get("thumbnail").and_then(|t| t.as_str()).map(|t| t.to_string());
            for (url, crop) in [(match_cover, false), (thumb, settings.square_cover)] {
                let Some(url) = url else { continue };
                if let Some(bytes) = meta::fetch_image(&self.http, &url).await {
                    if let Ok(Ok(jpeg)) = tokio::task::spawn_blocking(move || tags::prepare_cover(&bytes, crop, 1400)).await {
                        cover = Some(jpeg);
                        break;
                    }
                }
            }
        }
        Self::stopped(stop)?;

        let (tag_path, tag_meta, tag_cover) = (file.clone(), final_meta.clone(), cover.clone());
        let tagged = tokio::task::spawn_blocking(move || {
            let thumb = tag_cover.as_deref().and_then(|c| tags::data_url(c, 112));
            let change = tag_cover.map(tags::CoverChange::Set).unwrap_or(tags::CoverChange::Keep);
            (tags::write(&tag_path, &tag_meta, change, true), thumb)
        })
        .await
        .map_err(|e| fail(e.to_string()))?;
        let (tag_result, thumb) = tagged;

        // ---- move into place
        let out_dir = PathBuf::from(&settings.output_dir).join(render_folders(&settings.filename_template, &final_meta));
        tokio::fs::create_dir_all(&out_dir)
            .await
            .map_err(|e| fail(format!("Cannot create {}: {e}", out_dir.display())))?;
        let stem = render_filename(&settings.filename_template, &final_meta, &job.title);
        let dest = unique_path(&out_dir, &stem, ext, None);
        move_file(&file, &dest).await.map_err(|e| fail(format!("Could not save to {}: {e}", out_dir.display())))?;
        let size = tokio::fs::metadata(&dest).await.map(|m| m.len()).ok();

        self.mutate(id, |j| {
            j.title = final_meta.title.clone().unwrap_or_else(|| j.title.clone());
            j.artist = final_meta.artist.clone();
            j.album = final_meta.album.clone();
            if thumb.is_some() {
                j.thumbnail = thumb;
            }
            j.output = Some(dest.to_string_lossy().into_owned());
            j.size = size;
            j.matched = matched.as_ref().map(|m| m.0.clone());
            j.match_score = matched.as_ref().map(|m| m.1);
            j.error = tag_result
                .err()
                .map(|e| format!("Saved, but tags were not written: {e}"))
                .or_else(|| preview_only.then(|| "Preview only: the site offers just a short clip of this track without a paid account.".to_string()));
            j.status = Status::Done;
            j.note = None;
            j.progress = 1.0;
            j.finished = Some(now_ms());
        });
        Ok(())
    }

    async fn run_convert(&self, job: &Job, stop: &mut watch::Receiver<Option<Stop>>) -> Result<(), Outcome> {
        let id = job.id.as_str();
        let (ffmpeg, _) = tools::find("ffmpeg", &self.dirs.bin)
            .ok_or_else(|| fail("ffmpeg is not installed. Open Settings → Tools to install it."))?;
        let src = PathBuf::from(&job.source);
        if !src.is_file() {
            return Err(fail("The source file no longer exists."));
        }
        let read_src = src.clone();
        let (src_meta, src_cover) = tokio::task::spawn_blocking(move || tags::read_raw(&read_src))
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default();

        let dir = src.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "converted".into());
        let ext = job.format.as_str();
        let dest = unique_path(&dir, &stem, ext, None);
        let tmp = dir.join(format!(".{stem}.dubplate-{id}.{ext}"));

        let mut cmd = tools::command(&ffmpeg);
        cmd.args(["-hide_banner", "-nostdin", "-y", "-i"]).arg(&src);
        cmd.args(["-vn", "-map_metadata", "0"]);
        let ogg = if ext == "ogg" { ogg_mode(&ffmpeg).await } else { Ogg::Vorbis };
        if ogg == Ogg::Unavailable {
            return Err(fail(NO_OGG));
        }
        cmd.args(ffmpeg_codec_args(ext, &job.quality, ogg));
        cmd.args(["-progress", "pipe:1", "-nostats", "-loglevel", "error"]).arg(&tmp);

        self.mutate(id, |j| j.status = Status::Converting);
        let duration = job.duration.filter(|d| *d > 0.0);
        let mut last_error: Option<String> = None;
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        let result = self
            .run_process(id, cmd, stop, |line, is_err| {
                if is_err {
                    if !line.trim().is_empty() {
                        last_error = Some(line.trim().chars().take(300).collect());
                    }
                    return;
                }
                let micros = line
                    .strip_prefix("out_time_us=")
                    .or_else(|| line.strip_prefix("out_time_ms="))
                    .and_then(|v| v.trim().parse::<f64>().ok());
                if let (Some(us), Some(total)) = (micros, duration) {
                    let job = self.update(id, |j| j.progress = ((us / 1_000_000.0 / total).clamp(0.0, 1.0) * 0.97) as f32);
                    if last_emit.elapsed() >= Duration::from_millis(140) {
                        last_emit = Instant::now();
                        if let Some(j) = job {
                            self.emit(&j);
                        }
                    }
                }
            })
            .await;

        let status = match result {
            Ok(s) => s,
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(e);
            }
        };
        if !status.success() {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(fail(last_error.unwrap_or_else(|| "ffmpeg could not convert this file.".into())));
        }
        move_file(&tmp, &dest).await.map_err(|e| fail(format!("Could not save the converted file: {e}")))?;

        // ffmpeg carries text tags across; lofty makes sure the cover follows too.
        let (tag_path, tag_meta) = (dest.clone(), src_meta.clone());
        let (tag_result, thumb) = tokio::task::spawn_blocking(move || {
            let thumb = src_cover.as_deref().and_then(|c| tags::data_url(c, 112));
            let change = src_cover.map(tags::CoverChange::Set).unwrap_or(tags::CoverChange::Keep);
            (tags::write(&tag_path, &tag_meta, change, false), thumb)
        })
        .await
        .map_err(|e| fail(e.to_string()))?;
        let size = tokio::fs::metadata(&dest).await.map(|m| m.len()).ok();

        self.mutate(id, |j| {
            if thumb.is_some() {
                j.thumbnail = thumb;
            }
            j.output = Some(dest.to_string_lossy().into_owned());
            j.size = size;
            j.error = tag_result.err().map(|e| format!("Converted, but tags were not copied: {e}"));
            j.status = Status::Done;
            j.progress = 1.0;
            j.finished = Some(now_ms());
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_urls() {
        assert!(!is_collection("https://www.youtube.com/watch?v=abc&list=PL123"));
        assert!(is_collection("https://www.youtube.com/playlist?list=PL123"));
        assert!(is_collection("https://www.youtube.com/@someone/videos"));
        assert!(!is_collection("https://youtu.be/abc"));
        assert!(!is_collection("https://soundcloud.com/artist/track-name"));
        assert!(is_collection("https://soundcloud.com/artist/sets/an-ep"));
        assert!(is_collection("https://soundcloud.com/artist"));
        assert!(is_collection("https://soundcloud.com/artist/tracks"));
        assert_eq!(site_of("https://music.youtube.com/watch?v=x"), "youtube");
        assert_eq!(site_of("https://on.soundcloud.com/abc"), "soundcloud");
        assert_eq!(site_of("https://youtu.be/abc"), "youtube");
        assert_eq!(site_of("https://notyoutube.com/watch?v=x"), "web");
        assert_eq!(site_of("https://youtube.com.evil.example/x"), "web");
        assert_eq!(site_of("https://evil.example/?u=youtube.com"), "web");
        assert_eq!(site_of("https://youtube.com@evil.example/x"), "web");
        assert_eq!(normalize_url("youtu.be/abc").as_deref(), Some("https://youtu.be/abc"));
        assert_eq!(normalize_url("https://www.youtube.com/@someone").as_deref(), Some("https://www.youtube.com/@someone/videos"));
        assert_eq!(normalize_url("not a link"), None);
        assert!(is_transient(&friendly_error("ERROR: [soundcloud] Unable to download JSON metadata: Read timed out. (read timeout=20.0)")));
        assert!(!is_transient(&friendly_error("ERROR: [soundcloud] 1: This video is DRM protected")));
        assert!(is_transient(&friendly_error("ERROR: [youtube] M7amWll73Qg: Unable to download API page: HTTPSConnection(host='www.youtube.com', port=443): Failed to establish a new connection: [Errno 65] No route to host")));
        assert!(!is_transient(&friendly_error("ERROR: unable to download video data: HTTP Error 403: Forbidden")));
        assert_eq!(title_from_url("https://soundcloud.com/artist/world-on-fire-1"), "world on fire 1");
        assert_eq!(title_from_url("https://www.youtube.com/watch?v=abc"), "https://www.youtube.com/watch?v=abc");
        assert_eq!(title_from_url("https://api.soundcloud.com/tracks/soundcloud%3Atracks%3A1"), "https://api.soundcloud.com/tracks/soundcloud%3Atracks%3A1");
    }

    /// Hits the network: `cargo test live_search -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn live_search() {
        for site in ["youtube", "soundcloud"] {
            let hits = search_tracks(Path::new("/nonexistent"), "kevin macleod carefree", site).await.unwrap();
            assert!(hits.len() >= 3, "{site}: {} hits", hits.len());
            for h in &hits {
                assert_eq!(h.site, site);
                assert!(h.url.starts_with("https://") && !h.title.is_empty());
                assert!(!is_collection(&h.url), "{} would be treated as a playlist", h.url);
            }
            let h = &hits[0];
            println!("{site}: {} | {:?} | {:?} | {} | thumb={}", h.title, h.uploader, h.duration, h.url, h.thumbnail.is_some());
        }
        // A query that looks like an option must still be treated as text.
        assert!(search_tracks(Path::new("/nonexistent"), "--version", "youtube").await.is_ok());
    }

    #[test]
    fn builds_safe_filenames() {
        let meta = TrackMeta { title: Some("What? / Why: \"Now\"".into()), artist: Some("AC/DC".into()), ..Default::default() };
        assert_eq!(render_filename("{artist} - {title}", &meta, "x"), "AC DC - What Why Now");
        let no_artist = TrackMeta { title: Some("Solo".into()), ..Default::default() };
        assert_eq!(render_filename("{artist} - {title}", &no_artist, "x"), "Solo");
        assert_eq!(render_filename("{artist} - {title}", &TrackMeta::default(), "fall/back"), "fall back");

        let full = TrackMeta {
            title: Some("Archangel".into()),
            artist: Some("Burial".into()),
            album: Some("Untrue".into()),
            track: Some(2),
            ..Default::default()
        };
        let template = "{albumartist}/{album}/{track} {title}";
        assert_eq!(render_folders(template, &full), PathBuf::from("Burial").join("Untrue"));
        assert_eq!(render_filename(template, &full, "x"), "02 Archangel");
        // Missing tags drop that folder rather than leaving an empty or odd one.
        let single = TrackMeta { album: None, track: None, ..full.clone() };
        assert_eq!(render_folders(template, &single), PathBuf::from("Burial"));
        assert_eq!(render_filename(template, &single, "x"), "Archangel");
        // A template cannot climb out of the output folder.
        assert_eq!(render_folders("../../{artist}/{title}", &full), PathBuf::from("Burial"));
        assert_eq!(render_folders("{artist} - {title}", &full), PathBuf::new());
    }
}
