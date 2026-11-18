use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// "none" keeps what the site serves: SoundCloud's audio as it is, YouTube's video.
pub const FORMATS: [&str; 7] = ["mp3", "flac", "wav", "m4a", "aac", "ogg", "none"];
pub const ORIGINAL: &str = "none";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub output_dir: String,
    /// mp3 | flac | wav | m4a | aac | ogg | none (no conversion)
    pub format: String,
    /// best | 320 | 256 | 192 | 128 (ignored for lossless formats)
    pub quality: String,
    pub max_concurrent: usize,
    /// Tokens: {artist} {title} {album} {year} {track}
    pub filename_template: String,
    pub auto_tag: bool,
    /// 0..1, how sure a match must be before it is applied automatically
    pub match_threshold: f32,
    pub embed_cover: bool,
    pub square_cover: bool,
    pub use_musicbrainz: bool,
    pub use_itunes: bool,
    pub use_deezer: bool,
    pub use_discogs: bool,
    pub discogs_token: String,
    pub sponsorblock: bool,
    /// Browser to borrow cookies from for age/region restricted items: "" for
    /// off, a browser yt-dlp knows ("chrome"…), "helium", or "custom".
    pub cookies_browser: String,
    /// The browser picked under "Custom…".
    pub cookies_custom: Option<crate::cookies::CustomBrowser>,
    pub notify: bool,
    pub keep_in_tray: bool,
    pub hide_dock_icon: bool,
    pub auto_update: bool,
    pub clipboard_watch: bool,
    /// "system", "light" or "dark"
    pub appearance: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_dir: String::new(),
            format: "mp3".into(),
            quality: "best".into(),
            max_concurrent: 3,
            filename_template: "{artist} - {title}".into(),
            auto_tag: true,
            match_threshold: 0.78,
            embed_cover: true,
            square_cover: true,
            use_musicbrainz: true,
            use_itunes: true,
            use_deezer: true,
            use_discogs: true,
            discogs_token: String::new(),
            sponsorblock: false,
            cookies_browser: String::new(),
            cookies_custom: None,
            notify: true,
            keep_in_tray: !cfg!(target_os = "linux"),
            hide_dock_icon: false,
            auto_update: true,
            clipboard_watch: true,
            appearance: "system".into(),
        }
    }
}

impl Settings {
    pub fn load(file: &Path, default_output: PathBuf) -> Self {
        let mut s: Settings = std::fs::read(file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if s.output_dir.trim().is_empty() {
            s.output_dir = default_output.to_string_lossy().into_owned();
        }
        s.sanitize();
        s
    }

    pub fn sanitize(&mut self) {
        if !FORMATS.contains(&self.format.as_str()) {
            self.format = "mp3".into();
        }
        if !["best", "320", "256", "192", "128"].contains(&self.quality.as_str()) {
            self.quality = "best".into();
        }
        self.max_concurrent = self.max_concurrent.clamp(1, 8);
        if !["system", "light", "dark"].contains(&self.appearance.as_str()) {
            self.appearance = "system".into();
        }
        let cookies = self.cookies_browser.as_str();
        if !(cookies.is_empty() || cookies == "helium" || cookies == "custom" || crate::cookies::BUILT_IN.contains(&cookies)) {
            self.cookies_browser.clear();
        }
        self.match_threshold = self.match_threshold.clamp(0.5, 0.98);
        if self.filename_template.trim().is_empty() {
            self.filename_template = "{artist} - {title}".into();
        }
    }

    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap_or_default())?;
        std::fs::rename(tmp, file)
    }
}
