//! Finding the same recording on YouTube when the page the user gave cannot
//! supply it: SoundCloud tracks that are DRM-protected, or that only offer a
//! 30-second preview without a subscription.
//!
//! "The same recording" is strict. A candidate must have the same title and
//! artist, the same version (no remix for an original, no live take for a
//! studio one, no slowed or sped-up edit) and, wherever both lengths are known,
//! the same length to within a few seconds. Anything less is not downloaded.

use crate::jobs::{self, SearchHit};
use crate::meta::{self, TrackMeta};
use crate::settings::Settings;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

/// Words that mark a different version of a song. Each must appear the same
/// number of times in the candidate's title as in the original's.
const VERSION_WORDS: &[&str] = &[
    "remix", "rmx", "live", "cover", "acoustic", "slowed", "sped up", "speed up", "reverb", "nightcore", "8d",
    "karaoke", "instrumental", "extended", "vip", "bootleg", "mashup", "flip", "rework", "orchestral", "lofi",
    "lo-fi", "loop", "hour", "hours", "reaction", "tutorial", "lesson", "full album", "acapella", "a cappella",
    "remake", "demo", "unplugged", "piano version", "dub",
];

/// What the original page says the track is.
#[derive(Clone, Debug)]
pub struct Target {
    pub meta: TrackMeta,
    pub duration: Option<f64>,
}

/// A YouTube upload being considered.
#[derive(Clone, Debug, Default)]
pub struct Candidate {
    pub url: String,
    pub title: String,
    pub uploader: Option<String>,
    /// Track and artist fields, when YouTube Music provides them.
    pub track: Option<String>,
    pub artist: Option<String>,
    pub duration: Option<f64>,
    /// Listed as a song on YouTube Music: an official audio upload.
    pub official: bool,
}

#[derive(Clone, Debug)]
pub struct Found {
    pub url: String,
    /// Other uploads that matched just as strictly, best first, in case YouTube
    /// refuses to serve the first one.
    pub others: Vec<String>,
    pub score: f32,
    /// The track as identified, for tagging the download.
    pub identity: TrackMeta,
}

fn count_word(haystack: &str, word: &str) -> usize {
    let hay = format!(" {} ", haystack.to_lowercase().replace(|c: char| !c.is_alphanumeric() && c != '-', " "));
    let needle = format!(" {word} ");
    hay.matches(&needle).count()
}

fn remove_ignoring_case(text: &str, name: &str) -> String {
    if name.trim().is_empty() {
        return text.to_string();
    }
    let lower = text.to_lowercase();
    let needle = name.to_lowercase();
    // Lower-casing can change byte lengths outside ASCII; only then fall back to an exact match.
    if lower.len() != text.len() {
        return text.replace(name, " ");
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = 0;
    for (at, _) in lower.match_indices(&needle) {
        if at >= rest {
            out.push_str(&text[rest..at]);
            out.push(' ');
            rest = at + needle.len();
        }
    }
    out.push_str(&text[rest..]);
    out
}

fn versions(text: &str) -> Vec<usize> {
    VERSION_WORDS.iter().map(|w| count_word(text, w)).collect()
}

/// How sure we are that `c` is the exact recording `t` describes, from 0 to 1.
/// Zero means it is ruled out.
pub fn score(t: &Target, c: &Candidate) -> f32 {
    let Some(want_title) = t.meta.title.as_deref() else { return 0.0 };

    // Read the candidate's artist and title the way a downloaded page would be read.
    let parsed = meta::guess_from_source(&json!({
        "title": c.title,
        "uploader": c.uploader,
        "track": c.track,
        "artist": c.artist,
    }));
    let have_title = parsed.title.as_deref().unwrap_or(&c.title);

    // A different version of the song is a different track, however close the name.
    let mut want_text = want_title.to_string();
    let mut have_text = c.track.clone().unwrap_or_else(|| c.title.clone());
    // Words in the artist's own name ("Live", "Dub") do not mark a version.
    for name in [t.meta.artist.as_deref(), parsed.artist.as_deref(), c.uploader.as_deref()].into_iter().flatten() {
        want_text = remove_ignoring_case(&want_text, name);
        have_text = remove_ignoring_case(&have_text, name);
    }
    if versions(&want_text) != versions(&have_text) {
        return 0.0;
    }

    // The same recording runs the same length; music videos with intros do not.
    let duration = match (t.duration, c.duration) {
        (Some(a), Some(b)) if a > 0.0 && b > 0.0 => {
            let tolerance = (a * 0.02).max(3.0);
            let delta = (a - b).abs();
            if delta > tolerance {
                return 0.0;
            }
            Some(1.0 - (delta / tolerance) as f32 * 0.3)
        }
        _ => None,
    };

    // Titles must match nearly letter for letter once decorations such as
    // "(Official Audio)", "(Re-Mastered)" or "feat. …" are set aside: "World On
    // Fire" is not "Set The World On Fire".
    let title = strsim::normalized_levenshtein(&meta::normalize(want_title), &meta::normalize(have_title)) as f32;
    if title < 0.9 {
        return 0.0;
    }
    let artist = match (t.meta.artist.as_deref(), parsed.artist.as_deref()) {
        (Some(want), Some(have)) => meta::similarity(want, have)
            .max(meta::similarity(meta::first_artist(want), meta::first_artist(have)))
            // The uploader is the artist's own channel.
            .max(c.uploader.as_deref().map(|u| meta::similarity(want, u)).unwrap_or(0.0)),
        (Some(_), None) => 0.0,
        (None, _) => 0.6,
    };
    if t.meta.artist.is_some() && artist < 0.75 {
        return 0.0;
    }

    let mut total = title * 0.45 + artist * 0.35 + duration.unwrap_or(0.6) * 0.2;
    if c.official {
        total += 0.05;
    }
    // Without lengths to compare, only an official upload with a near-perfect name will do.
    if duration.is_none() && !(c.official && total >= 0.9) {
        return 0.0;
    }
    total.min(1.0)
}

/// The YouTube video id, so the same upload found by two searches counts once.
fn video_id(url: &str) -> Option<String> {
    let u = tauri::Url::parse(url).ok()?;
    if u.host_str()?.ends_with("youtu.be") {
        return u.path_segments()?.next().map(String::from);
    }
    u.query_pairs().find(|(k, _)| k == "v").map(|(_, v)| v.into_owned())
}

/// Search words for a track: "(Re-Mastered)", "[HD]" and similar brackets only
/// muddy a search, but a bracket naming a version ("(Skream Remix)") stays.
fn search_terms(m: &TrackMeta) -> Option<String> {
    static BRACKETS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let brackets = BRACKETS.get_or_init(|| regex::Regex::new(r"\s*[\(\[][^\)\]]*[\)\]]").unwrap());
    let raw = meta::clean_title(m.title.as_deref()?);
    let title = brackets
        .replace_all(&raw, |c: &regex::Captures| {
            if versions(&c[0]).iter().any(|n| *n > 0) { c[0].to_string() } else { String::new() }
        })
        .trim()
        .to_string();
    let title = if title.is_empty() { raw } else { title };
    Some(match m.artist.as_deref() {
        Some(a) => format!("{} {title}", meta::first_artist(a)),
        None => title,
    })
}

/// Work out exactly which track the original page is, then look for that
/// recording on YouTube. Returns `None` unless a match is certain enough.
pub async fn find_on_youtube(bin: &Path, http: &reqwest::Client, settings: &Settings, source: &Value) -> Option<Found> {
    let page = meta::guess_from_source(source);
    page.title.as_ref()?;
    // A preview-only page reports the clip's length (30 seconds), not the track's.
    let preview = source.get("format_id").and_then(|f| f.as_str()).is_some_and(|f| f.contains("preview"));
    let page_duration = source.get("duration").and_then(|d| d.as_f64()).filter(|_| !preview);

    // The music databases often know the official artist and title better than an
    // upload's own description ("Artist - Track (Re-Mastered)" by a label account),
    // and they know the full length of a track the page only previews.
    let query = meta::Query { title: page.title.clone().unwrap_or_default(), artist: page.artist.clone(), duration: page_duration };
    let listing = meta::best_match(&meta::search(http, settings, &query).await, 0.85);
    let official = listing.as_ref().map(|c| page.overlaid_with(&c.meta));
    let duration = page_duration.or_else(|| listing.as_ref().and_then(|c| c.duration));

    let mut targets = vec![Target { meta: page.clone(), duration }];
    if let Some(o) = &official {
        if o.title != page.title || o.artist != page.artist {
            targets.push(Target { meta: o.clone(), duration });
        }
    }

    // Several searches at once: plain and "official audio" for each name the track
    // goes by, plus YouTube Music's song list, which holds the official uploads.
    let mut specs: Vec<(String, bool)> = Vec::new();
    for t in &targets {
        if let Some(terms) = search_terms(&t.meta) {
            specs.push((format!("ytsearch12:{terms}"), false));
            specs.push((format!("ytsearch8:{terms} official audio"), false));
        }
    }
    if let Some(terms) = targets.last().and_then(|t| search_terms(&t.meta)) {
        let q: String = tauri::Url::parse_with_params("https://music.youtube.com/search", &[("q", terms.as_str())])
            .map(|u| u.to_string())
            .unwrap_or_default();
        if !q.is_empty() {
            specs.push((format!("{q}#songs"), true));
        }
    }
    specs.dedup();
    // One track is looked up at a time: a playlist full of protected tracks would
    // otherwise fire dozens of searches at once, and YouTube starts refusing them.
    static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _turn = ONE_AT_A_TIME.lock().await;
    let search = |spec: String| async move {
        match jobs::search_spec(bin, &spec).await {
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                jobs::search_spec(bin, &spec).await
            }
            ok => ok,
        }
    };
    let results = futures::future::join_all(specs.iter().map(|(spec, _)| search(spec.clone()))).await;
    #[cfg(debug_assertions)]
    for ((spec, _), r) in specs.iter().zip(&results) {
        eprintln!("[elsewhere] {spec} -> {}", r.as_ref().map(|h| format!("{} results", h.len())).unwrap_or_else(|e| format!("error: {e}")));
    }

    let mut found: HashMap<String, Candidate> = HashMap::new();
    for ((_, official), hits) in specs.iter().zip(results) {
        for hit in hits.unwrap_or_default() {
            let SearchHit { url, title, uploader, duration, .. } = hit;
            let Some(id) = video_id(&url) else { continue };
            let entry = found.entry(id.clone()).or_insert_with(|| Candidate {
                url: format!("https://www.youtube.com/watch?v={id}"),
                title: title.clone(),
                ..Default::default()
            });
            entry.official |= *official;
            if entry.uploader.is_none() {
                entry.uploader = uploader;
            }
            if entry.duration.is_none() {
                entry.duration = duration;
            }
        }
    }

    // Search listings can leave out the length or the channel (YouTube Music's song
    // list always does). Look up in full every result whose title fits, so a good
    // match is never passed over for missing details.
    let fits = |c: &Candidate| {
        let title = meta::guess_from_source(&json!({ "title": c.title, "uploader": c.uploader })).title.unwrap_or_default();
        targets.iter().any(|t| {
            let want = meta::normalize(t.meta.title.as_deref().unwrap_or_default());
            strsim::normalized_levenshtein(&want, &meta::normalize(&title)) >= 0.9
                || strsim::normalized_levenshtein(&want, &meta::normalize(&c.title)) >= 0.9
        })
    };
    let mut details: Vec<String> = found
        .iter()
        .filter(|(_, c)| (c.duration.is_none() || c.uploader.is_none()) && fits(c))
        .map(|(id, _)| id.clone())
        .collect();
    details.sort_by_key(|id| !found[id].official);
    details.truncate(6);
    let infos = futures::future::join_all(details.iter().map(|id| {
        let url = format!("https://www.youtube.com/watch?v={id}");
        async move {
            match jobs::fetch_info(bin, &url).await {
                Ok(info) => Some(info),
                Err(_) => jobs::fetch_info(bin, &url).await.ok(),
            }
        }
    }))
    .await;
    let text = |v: &Value, k: &str| v.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(String::from);
    for (id, info) in details.iter().zip(infos) {
        let (Some(info), Some(c)) = (info, found.get_mut(id)) else { continue };
        c.duration = c.duration.or_else(|| info.get("duration").and_then(|d| d.as_f64()));
        c.uploader = c.uploader.take().or_else(|| text(&info, "uploader")).or_else(|| text(&info, "channel"));
        c.track = text(&info, "track");
        c.artist = text(&info, "artist").or_else(|| {
            info.get("artists").and_then(|a| a.as_array()).and_then(|a| a.first()).and_then(|a| a.as_str()).map(String::from)
        });
    }

    #[cfg(debug_assertions)]
    {
        for t in &targets {
            eprintln!("[elsewhere] target {:?} / {:?} / {:?}s", t.meta.artist, t.meta.title, t.duration);
        }
        for c in found.values() {
            let s = targets.iter().map(|t| score(t, c)).fold(0.0f32, f32::max);
            eprintln!("[elsewhere]   {s:.2} {:?} by {:?} {:?}s official={}", c.title, c.uploader, c.duration, c.official);
        }
    }
    let mut matches: Vec<(f32, Candidate)> = found
        .into_values()
        .map(|c| {
            let s = targets.iter().map(|t| score(t, &c)).fold(0.0f32, f32::max);
            (s, c)
        })
        .filter(|(s, _)| *s >= 0.85)
        .collect();
    // Best first; between equals, an official upload first.
    matches.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(b.1.official.cmp(&a.1.official)));
    let mut urls = matches.iter().map(|(_, c)| c.url.clone());
    let url = urls.next()?;
    Some(Found { url, others: urls.take(3).collect(), score: matches[0].0, identity: official.unwrap_or(page) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> Target {
        Target {
            meta: TrackMeta {
                title: Some("World On Fire (Re-Mastered)".into()),
                artist: Some("The Royal Concept".into()),
                ..Default::default()
            },
            duration: Some(199.889),
        }
    }

    fn cand(title: &str, uploader: &str, duration: f64) -> Candidate {
        Candidate { url: "u".into(), title: title.into(), uploader: Some(uploader.into()), duration: Some(duration), ..Default::default() }
    }

    // The real search results for this track, November 2026.
    #[test]
    fn picks_the_exact_recording() {
        let t = target();
        assert!(score(&t, &cand("World On Fire", "The Royal Concept", 200.0)) >= 0.9);
        // Re-uploads with the same audio are acceptable too.
        assert!(score(&t, &cand("The Royal Concept - World On Fire (Official Audio)", "Some Fan", 201.0)) >= 0.85);
    }

    #[test]
    fn rejects_near_misses() {
        let t = target();
        // Different length: a different edit or a video with an intro.
        assert_eq!(score(&t, &cand("The Royal Concept - World On Fire", "HearIndie", 229.0)), 0.0);
        // A live version, even at the same length.
        assert_eq!(score(&t, &cand("The Royal Concept - World On Fire (Live In The Basement)", "The Royal Concept", 199.0)), 0.0);
        // Same title, different song.
        assert_eq!(score(&t, &cand("The Royal Concept - Set The World On Fire", "ayyuktan", 200.0)), 0.0);
        assert_eq!(score(&t, &cand("World On Fire", "Sarah McLachlan", 200.0)), 0.0);
        assert_eq!(score(&t, &cand("World On Fire (Slowed + Reverb)", "The Royal Concept", 200.0)), 0.0);
        assert_eq!(score(&t, &cand("World On Fire (Nightcore)", "The Royal Concept", 199.0)), 0.0);
    }

    #[test]
    fn remixes_must_match_exactly() {
        let t = Target {
            meta: TrackMeta { title: Some("Tune (Skream Remix)".into()), artist: Some("La Roux".into()), ..Default::default() },
            duration: Some(300.0),
        };
        assert!(score(&t, &cand("La Roux - Tune (Skream Remix)", "Skream", 301.0)) >= 0.85);
        assert_eq!(score(&t, &cand("La Roux - Tune (Original Mix)", "La Roux", 300.0)), 0.0);
        assert_eq!(score(&t, &cand("La Roux - Tune", "La Roux", 300.0)), 0.0);
        assert_eq!(score(&t, &cand("La Roux - Tune (Other Remix)", "La Roux", 300.0)), 0.0);
    }

    #[test]
    fn artist_names_do_not_count_as_versions() {
        let t = Target {
            meta: TrackMeta { title: Some("Selection".into()), artist: Some("Live".into()), ..Default::default() },
            duration: Some(200.0),
        };
        assert!(score(&t, &cand("Live - Selection", "LIVE", 200.0)) >= 0.85);
    }

    #[test]
    fn unknown_length_needs_an_official_upload() {
        let t = target();
        let mut c = cand("World On Fire", "The Royal Concept", 0.0);
        c.duration = None;
        assert_eq!(score(&t, &c), 0.0);
        c.official = true;
        assert!(score(&t, &c) >= 0.85);
    }

    #[test]
    fn searches_without_decorations() {
        let m = |t: &str| TrackMeta { title: Some(t.into()), artist: Some("The Royal Concept".into()), ..Default::default() };
        assert_eq!(search_terms(&m("World On Fire (Re-Mastered)")).as_deref(), Some("The Royal Concept World On Fire"));
        assert_eq!(search_terms(&m("Tune (Skream Remix) [HD]")).as_deref(), Some("The Royal Concept Tune (Skream Remix)"));
    }

    #[test]
    fn reads_video_ids() {
        assert_eq!(video_id("https://www.youtube.com/watch?v=XGOdJwEdNJs").as_deref(), Some("XGOdJwEdNJs"));
        assert_eq!(video_id("https://music.youtube.com/watch?v=XGOdJwEdNJs&list=x").as_deref(), Some("XGOdJwEdNJs"));
        assert_eq!(video_id("https://youtu.be/XGOdJwEdNJs").as_deref(), Some("XGOdJwEdNJs"));
    }
}
