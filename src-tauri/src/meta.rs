//! Track metadata: guessing it from the source page, looking it up on the free
//! music databases, and scoring how well each result matches.

use crate::settings::Settings;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Sent as the User-Agent. MusicBrainz and Discogs both ask for an app name,
/// version and a contact URL; point this at the project's home page.
pub const USER_AGENT: &str = concat!(
    "Dubplate/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/0mattsmith/dubplate )"
);

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackMeta {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<u32>,
    pub track: Option<u32>,
    pub track_total: Option<u32>,
    pub disc: Option<u32>,
    pub genre: Option<String>,
    pub comment: Option<String>,
}

impl TrackMeta {
    /// Fields set on `other` win; anything it lacks is kept from `self`.
    pub fn overlaid_with(&self, other: &TrackMeta) -> TrackMeta {
        TrackMeta {
            title: other.title.clone().or_else(|| self.title.clone()),
            artist: other.artist.clone().or_else(|| self.artist.clone()),
            album: other.album.clone().or_else(|| self.album.clone()),
            album_artist: other.album_artist.clone().or_else(|| self.album_artist.clone()),
            year: other.year.or(self.year),
            track: other.track.or(self.track),
            track_total: other.track_total.or(self.track_total),
            disc: other.disc.or(self.disc),
            genre: other.genre.clone().or_else(|| self.genre.clone()),
            comment: other.comment.clone().or_else(|| self.comment.clone()),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Candidate {
    pub source: String,
    pub meta: TrackMeta,
    pub cover_url: Option<String>,
    pub thumb_url: Option<String>,
    pub duration: Option<f64>,
    pub score: f32,
    pub link: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Query {
    pub title: String,
    pub artist: Option<String>,
    pub duration: Option<f64>,
}

// ---------------------------------------------------------------- guessing

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
}

fn junk_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\s*[\(\[\{][^\)\]\}]*\b(official|video|audio|lyrics?|visuali[sz]er|hd|hq|4k|m/?v|explicit|full\s+song|out\s+now|free\s+(download|dl)|premiere)\b[^\)\]\}]*[\)\]\}]",
        )
        .unwrap()
    })
}

/// Strip "(Official Video)", "[HD]" and similar decorations from a title.
pub fn clean_title(raw: &str) -> String {
    let t = junk_re().replace_all(raw, "");
    let t = t.split(" | ").next().unwrap_or(&t);
    t.trim().trim_matches(|c| c == '-' || c == '–' || c == '|').trim().to_string()
}

fn clean_channel(raw: &str) -> String {
    let mut c = raw.trim().to_string();
    for suffix in [" - Topic", "VEVO", " Official"] {
        if c.len() > suffix.len() + 1 && c.to_lowercase().ends_with(&suffix.to_lowercase()) {
            c.truncate(c.len() - suffix.len());
        }
    }
    c.trim().to_string()
}

/// Build starting tags from the yt-dlp info JSON for one item.
pub fn guess_from_source(info: &Value) -> TrackMeta {
    let raw_title = s(info, "title").unwrap_or_default();
    let channel = s(info, "uploader").or_else(|| s(info, "channel")).map(|c| clean_channel(&c));

    // YouTube Music and some SoundCloud uploads carry real track fields.
    let (mut title, mut artist) = (s(info, "track"), s(info, "artist"));

    if title.is_none() || artist.is_none() {
        let cleaned = clean_title(&raw_title);
        let near_channel = |text: &str| channel.as_deref().map(|c| similarity(c, text)).unwrap_or(0.0);
        let split = [" - ", " – ", " — ", " ~ "]
            .iter()
            .find_map(|sep| cleaned.split_once(sep))
            // "Artist: Title" is only trusted when the uploader is that artist,
            // since a colon is just as often part of the title itself.
            .or_else(|| cleaned.split_once(": ").filter(|(a, _)| near_channel(a) > 0.8))
            .map(|(a, t)| (a.trim().to_string(), t.trim().to_string()))
            .filter(|(a, t)| !a.is_empty() && !t.is_empty())
            // Some uploads are written "Title - Artist"; the channel name settles it.
            .map(|(a, t)| if near_channel(&t) > 0.8 && near_channel(&a) < 0.5 { (t, a) } else { (a, t) });
        match split {
            Some((a, t)) => {
                title = title.or(Some(t));
                artist = artist.or(Some(a));
            }
            None => {
                title = title.or(Some(cleaned).filter(|c| !c.is_empty()));
                artist = artist.or(channel.clone());
            }
        }
    }

    let year = info
        .get("release_year")
        .and_then(|y| y.as_u64())
        .map(|y| y as u32)
        .or_else(|| {
            s(info, "release_date")
                .or_else(|| s(info, "upload_date"))
                .and_then(|d| d.get(0..4).and_then(|y| y.parse().ok()))
        });

    let genre = info
        .get("genres")
        .and_then(|g| g.as_array())
        .and_then(|g| g.first())
        .and_then(|g| g.as_str())
        .map(|g| g.to_string())
        .or_else(|| s(info, "genre"));

    TrackMeta {
        title: title.or(Some(raw_title).filter(|t| !t.is_empty())),
        artist,
        album: s(info, "album"),
        album_artist: s(info, "album_artist"),
        year,
        track: info.get("track_number").and_then(|n| n.as_u64()).map(|n| n as u32),
        genre,
        ..Default::default()
    }
}

// ----------------------------------------------------------------- scoring

fn normalize(text: &str) -> String {
    static FEAT: OnceLock<Regex> = OnceLock::new();
    let feat = FEAT.get_or_init(|| {
        Regex::new(r"(?i)[\(\[][^\)\]]*\b(feat|ft|featuring|with|remaster(ed)?|version|edit|mono|stereo|deluxe|bonus)\b[^\)\]]*[\)\]]|\b(feat|ft|featuring)\b\.?.*$").unwrap()
    });
    let stripped = feat.replace_all(text, " ");
    let mut out = String::with_capacity(stripped.len());
    for c in stripped.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if c == '&' {
            out.push_str(" and ");
        } else if c != '\'' && c != '’' {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn similarity(a: &str, b: &str) -> f32 {
    let (na, nb) = (normalize(a), normalize(b));
    if na.is_empty() || nb.is_empty() {
        return 0.0;
    }
    if na == nb {
        return 1.0;
    }
    let lev = strsim::normalized_levenshtein(&na, &nb) as f32;
    let (ta, tb): (std::collections::HashSet<&str>, std::collections::HashSet<&str>) =
        (na.split(' ').collect(), nb.split(' ').collect());
    let inter = ta.intersection(&tb).count() as f32;
    let jaccard = inter / (ta.len() + tb.len()) .max(1) as f32 * 2.0 * 0.95;
    // One name wholly containing the other ("Queen" vs "Queen Official").
    let contained = if inter as usize == ta.len().min(tb.len()) { 0.86 } else { 0.0 };
    lev.max(jaccard).max(contained)
}

fn first_artist(a: &str) -> &str {
    a.split([',', '&', ';'])
        .next()
        .unwrap_or(a)
        .split(" x ")
        .next()
        .unwrap_or(a)
        .trim()
}

pub fn score(query: &Query, cand: &Candidate) -> f32 {
    let title = similarity(&query.title, cand.meta.title.as_deref().unwrap_or(""));
    let artist = match (&query.artist, &cand.meta.artist) {
        (Some(q), Some(c)) => similarity(q, c).max(similarity(first_artist(q), first_artist(c)) * 0.97),
        _ => 0.0,
    };
    let duration = match (query.duration, cand.duration) {
        (Some(q), Some(c)) if q > 0.0 && c > 0.0 => {
            let delta = (q - c).abs();
            Some(if delta <= 3.0 { 1.0 } else { (1.0 - (delta - 3.0) / 27.0).max(0.0) as f32 })
        }
        _ => None,
    };
    let mut total = match (query.artist.is_some(), duration) {
        (true, Some(d)) => title * 0.47 + artist * 0.38 + d * 0.15,
        (true, None) => title * 0.55 + artist * 0.45,
        // Without an artist to check against, never score high enough to auto-apply.
        (false, Some(d)) => (title * 0.8 + d * 0.2) * 0.75,
        (false, None) => title * 0.7,
    };
    // Discogs matches releases, not recordings, so its title is unverified.
    if cand.source == "Discogs" {
        total *= 0.9;
    }
    // A wrong title or artist should sink the match however good the rest is.
    if title < 0.5 || (query.artist.is_some() && artist < 0.4) {
        total *= 0.6;
    }
    total.clamp(0.0, 1.0)
}

// --------------------------------------------------------------- providers

async fn get_json(req: reqwest::RequestBuilder) -> Option<Value> {
    req.timeout(Duration::from_secs(9))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()
}

fn year_of(date: Option<&str>) -> Option<u32> {
    date.and_then(|d| d.get(0..4)).and_then(|y| y.parse().ok()).filter(|y| *y > 1000)
}

async fn musicbrainz(http: &reqwest::Client, q: &Query) -> Vec<Candidate> {
    // MusicBrainz allows one request per second per client.
    static GATE: OnceLock<tokio::sync::Mutex<Instant>> = OnceLock::new();
    let gate = GATE.get_or_init(|| tokio::sync::Mutex::new(Instant::now() - Duration::from_secs(2)));
    {
        let mut last = gate.lock().await;
        let wait = Duration::from_millis(1100).saturating_sub(last.elapsed());
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        *last = Instant::now();
    }

    let esc = |t: &str| t.replace('\\', " ").replace('"', " ");
    let mut lucene = format!("recording:\"{}\"", esc(&q.title));
    if let Some(a) = &q.artist {
        lucene.push_str(&format!(" AND artist:\"{}\"", esc(first_artist(a))));
    }
    let Some(json) = get_json(
        http.get("https://musicbrainz.org/ws/2/recording")
            .query(&[("query", lucene.as_str()), ("fmt", "json"), ("limit", "6")]),
    )
    .await
    else {
        return vec![];
    };

    let credit = |v: &Value| -> Option<String> {
        let parts = v.get("artist-credit")?.as_array()?;
        let mut out = String::new();
        for p in parts {
            out.push_str(p.get("name").and_then(|n| n.as_str()).unwrap_or(""));
            out.push_str(p.get("joinphrase").and_then(|n| n.as_str()).unwrap_or(""));
        }
        Some(out.trim().to_string()).filter(|o| !o.is_empty())
    };

    let mut out = Vec::new();
    for rec in json.get("recordings").and_then(|r| r.as_array()).into_iter().flatten() {
        let mut meta = TrackMeta { title: s(rec, "title"), artist: credit(rec), ..Default::default() };
        let (mut cover_url, mut thumb_url) = (None, None);

        // Prefer an official studio album, then the earliest release.
        let best = rec.get("releases").and_then(|r| r.as_array()).and_then(|rels| {
            rels.iter().min_by_key(|r| {
                let group = r.get("release-group");
                let official = r.get("status").and_then(|x| x.as_str()) == Some("Official");
                let album = group.and_then(|g| g.get("primary-type")).and_then(|x| x.as_str()) == Some("Album");
                let plain = group
                    .and_then(|g| g.get("secondary-types"))
                    .and_then(|x| x.as_array())
                    .map(|a| a.is_empty())
                    .unwrap_or(true);
                let date = r.get("date").and_then(|d| d.as_str()).filter(|d| !d.is_empty()).unwrap_or("9999").to_string();
                (!official, !plain, !album, date)
            })
        });
        if let Some(rel) = best {
            meta.album = s(rel, "title");
            meta.year = year_of(rel.get("date").and_then(|d| d.as_str()));
            meta.album_artist = credit(rel).or_else(|| meta.artist.clone());
            if let Some(medium) = rel.get("media").and_then(|m| m.as_array()).and_then(|m| m.first()) {
                meta.disc = medium.get("position").and_then(|p| p.as_u64()).map(|p| p as u32);
                meta.track_total = medium.get("track-count").and_then(|p| p.as_u64()).map(|p| p as u32);
                meta.track = medium
                    .get("track")
                    .and_then(|t| t.as_array())
                    .and_then(|t| t.first())
                    .and_then(|t| t.get("number"))
                    .and_then(|n| n.as_str())
                    .and_then(|n| n.parse().ok());
            }
            if let Some(id) = s(rel, "id") {
                cover_url = Some(format!("https://coverartarchive.org/release/{id}/front-500"));
                thumb_url = Some(format!("https://coverartarchive.org/release/{id}/front-250"));
            }
        }
        meta.genre = rec
            .get("tags")
            .and_then(|t| t.as_array())
            .and_then(|tags| tags.iter().max_by_key(|t| t.get("count").and_then(|c| c.as_i64()).unwrap_or(0)))
            .and_then(|t| s(t, "name"))
            .map(|g| title_case(&g));

        out.push(Candidate {
            source: "MusicBrainz".into(),
            meta,
            cover_url,
            thumb_url,
            duration: rec.get("length").and_then(|l| l.as_f64()).map(|l| l / 1000.0),
            link: s(rec, "id").map(|id| format!("https://musicbrainz.org/recording/{id}")),
            score: 0.0,
        });
    }
    out
}

fn title_case(text: &str) -> String {
    text.split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

async fn itunes(http: &reqwest::Client, q: &Query) -> Vec<Candidate> {
    let term = format!("{} {}", q.artist.as_deref().map(first_artist).unwrap_or(""), q.title);
    let Some(json) = get_json(http.get("https://itunes.apple.com/search").query(&[
        ("term", term.trim()),
        ("media", "music"),
        ("entity", "song"),
        ("limit", "6"),
    ]))
    .await
    else {
        return vec![];
    };
    let num = |v: &Value, k: &str| v.get(k).and_then(|n| n.as_u64()).map(|n| n as u32);
    json.get("results")
        .and_then(|r| r.as_array())
        .into_iter()
        .flatten()
        .map(|r| {
            let art = s(r, "artworkUrl100");
            Candidate {
                source: "iTunes".into(),
                meta: TrackMeta {
                    title: s(r, "trackName"),
                    artist: s(r, "artistName"),
                    album: s(r, "collectionName").map(|a| a.trim_end_matches(" - Single").to_string()),
                    album_artist: s(r, "collectionArtistName").or_else(|| s(r, "artistName")),
                    year: year_of(r.get("releaseDate").and_then(|d| d.as_str())),
                    track: num(r, "trackNumber"),
                    track_total: num(r, "trackCount"),
                    disc: num(r, "discNumber"),
                    genre: s(r, "primaryGenreName"),
                    comment: None,
                },
                cover_url: art.as_ref().map(|a| a.replace("100x100bb", "1000x1000bb")),
                thumb_url: art.as_ref().map(|a| a.replace("100x100bb", "200x200bb")),
                duration: r.get("trackTimeMillis").and_then(|l| l.as_f64()).map(|l| l / 1000.0),
                link: s(r, "trackViewUrl"),
                score: 0.0,
            }
        })
        .collect()
}

async fn deezer(http: &reqwest::Client, q: &Query) -> Vec<Candidate> {
    let esc = |t: &str| t.replace('"', " ");
    let query = match &q.artist {
        Some(a) => format!("artist:\"{}\" track:\"{}\"", esc(first_artist(a)), esc(&q.title)),
        None => format!("track:\"{}\"", esc(&q.title)),
    };
    let Some(json) = get_json(
        http.get("https://api.deezer.com/search").query(&[("q", query.as_str()), ("limit", "6")]),
    )
    .await
    else {
        return vec![];
    };
    let mut out: Vec<(Option<u64>, Candidate)> = json
        .get("data")
        .and_then(|r| r.as_array())
        .into_iter()
        .flatten()
        .map(|r| {
            let album = r.get("album").cloned().unwrap_or(Value::Null);
            let artist = r.get("artist").and_then(|a| s(a, "name"));
            (
                r.get("id").and_then(|i| i.as_u64()),
                Candidate {
                    source: "Deezer".into(),
                    meta: TrackMeta {
                        title: s(r, "title"),
                        artist: artist.clone(),
                        album: s(&album, "title"),
                        album_artist: artist,
                        ..Default::default()
                    },
                    cover_url: s(&album, "cover_xl").or_else(|| s(&album, "cover_big")),
                    thumb_url: s(&album, "cover_medium"),
                    duration: r.get("duration").and_then(|d| d.as_f64()),
                    link: s(r, "link"),
                    score: 0.0,
                },
            )
        })
        .collect();

    // The search result has no year or track number; the track endpoint does.
    let details = futures::future::join_all(out.iter().take(2).map(|(id, _)| {
        let id = *id;
        async move {
            match id {
                Some(id) => get_json(http.get(format!("https://api.deezer.com/track/{id}"))).await,
                None => None,
            }
        }
    }))
    .await;
    for ((_, cand), detail) in out.iter_mut().zip(details) {
        if let Some(d) = detail {
            cand.meta.year = year_of(d.get("release_date").and_then(|x| x.as_str()));
            cand.meta.track = d.get("track_position").and_then(|n| n.as_u64()).map(|n| n as u32);
            cand.meta.disc = d.get("disk_number").and_then(|n| n.as_u64()).map(|n| n as u32);
        }
    }
    out.into_iter().map(|(_, c)| c).collect()
}

async fn discogs(http: &reqwest::Client, q: &Query, token: &str) -> Vec<Candidate> {
    let mut params = vec![("type", "release"), ("per_page", "5"), ("track", q.title.as_str())];
    let artist = q.artist.as_deref().map(first_artist);
    if let Some(a) = artist {
        params.push(("artist", a));
    }
    let Some(json) = get_json(
        http.get("https://api.discogs.com/database/search")
            .query(&params)
            .header("Authorization", format!("Discogs token={token}")),
    )
    .await
    else {
        return vec![];
    };
    static DISAMBIG: OnceLock<Regex> = OnceLock::new();
    let disambig = DISAMBIG.get_or_init(|| Regex::new(r"\s*\(\d+\)$|\*$").unwrap());
    json.get("results")
        .and_then(|r| r.as_array())
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let full = s(r, "title")?;
            let (art, album) = full.split_once(" - ")?;
            let art = disambig.replace_all(art.trim(), "").to_string();
            let first = |k: &str| {
                r.get(k).and_then(|g| g.as_array()).and_then(|g| g.first()).and_then(|g| g.as_str()).map(|g| g.to_string())
            };
            Some(Candidate {
                source: "Discogs".into(),
                meta: TrackMeta {
                    title: Some(q.title.clone()),
                    artist: Some(art.clone()),
                    album: Some(album.trim().to_string()),
                    album_artist: Some(art),
                    year: r
                        .get("year")
                        .and_then(|y| y.as_u64().map(|y| y as u32).or_else(|| y.as_str().and_then(|y| y.parse().ok()))),
                    genre: first("style").or_else(|| first("genre")),
                    ..Default::default()
                },
                cover_url: s(r, "cover_image"),
                thumb_url: s(r, "thumb"),
                duration: None,
                link: s(r, "uri").map(|u| format!("https://www.discogs.com{u}")),
                score: 0.0,
            })
        })
        .collect()
}

/// Query every enabled provider at once and return the results best-first.
pub async fn search(http: &reqwest::Client, settings: &Settings, q: &Query) -> Vec<Candidate> {
    if q.title.trim().is_empty() {
        return vec![];
    }
    let token = settings.discogs_token.trim();
    let (mb, it, dz, dc) = tokio::join!(
        async { if settings.use_musicbrainz { musicbrainz(http, q).await } else { vec![] } },
        async { if settings.use_itunes { itunes(http, q).await } else { vec![] } },
        async { if settings.use_deezer { deezer(http, q).await } else { vec![] } },
        async { if settings.use_discogs && !token.is_empty() { discogs(http, q, token).await } else { vec![] } },
    );
    // Interleaved in this order so that ties favour the richer sources.
    let mut all: Vec<Candidate> = it.into_iter().chain(dz).chain(mb).chain(dc).collect();
    for c in &mut all {
        c.score = score(q, c);
    }
    all.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    all
}

/// The match to apply automatically, with gaps filled from other confident
/// results for the same album.
pub fn best_match(cands: &[Candidate], threshold: f32) -> Option<Candidate> {
    let mut best = cands.first().filter(|c| c.score >= threshold)?.clone();
    for other in cands.iter().skip(1).filter(|c| c.score >= threshold) {
        let same_album = match (&best.meta.album, &other.meta.album) {
            (Some(a), Some(b)) => similarity(a, b) > 0.85,
            _ => false,
        };
        if best.meta.genre.is_none() {
            best.meta.genre = other.meta.genre.clone();
        }
        if same_album {
            best.meta.year = best.meta.year.or(other.meta.year);
            best.meta.track = best.meta.track.or(other.meta.track);
            best.meta.track_total = best.meta.track_total.or(other.meta.track_total);
            best.meta.disc = best.meta.disc.or(other.meta.disc);
            if best.cover_url.is_none() {
                best.cover_url = other.cover_url.clone();
            }
        }
    }
    Some(best)
}

pub async fn fetch_image(http: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    let resp = http
        .get(url)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let bytes = resp.bytes().await.ok()?;
    (bytes.len() > 256 && bytes.len() < 20 * 1024 * 1024).then(|| bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cleans_decorated_titles() {
        assert_eq!(clean_title("Daft Punk - Get Lucky (Official Audio) ft. Pharrell"), "Daft Punk - Get Lucky ft. Pharrell");
        assert_eq!(clean_title("Song Name [Official Music Video] [4K]"), "Song Name");
        assert_eq!(clean_title("Track (Radio Edit)"), "Track (Radio Edit)");
        assert_eq!(clean_title("Tune (Skream Remix)"), "Tune (Skream Remix)");
    }

    #[test]
    fn guesses_artist_and_title() {
        let m = guess_from_source(&json!({"title": "Burial - Archangel (Official Video)", "uploader": "Hyperdub", "upload_date": "20071105"}));
        assert_eq!(m.artist.as_deref(), Some("Burial"));
        assert_eq!(m.title.as_deref(), Some("Archangel"));
        assert_eq!(m.year, Some(2007));

        let m = guess_from_source(&json!({"title": "Archangel", "uploader": "Burial - Topic", "track": "Archangel", "artist": "Burial", "album": "Untrue", "release_year": 2007}));
        assert_eq!(m.album.as_deref(), Some("Untrue"));
        assert_eq!(m.artist.as_deref(), Some("Burial"));

        let m = guess_from_source(&json!({"title": "Kevin MacLeod: Monkeys Spinning Monkeys", "uploader": "Kevin MacLeod"}));
        assert_eq!(m.artist.as_deref(), Some("Kevin MacLeod"));
        assert_eq!(m.title.as_deref(), Some("Monkeys Spinning Monkeys"));

        let m = guess_from_source(&json!({"title": "Star Wars: Main Theme", "uploader": "Film Scores"}));
        assert_eq!(m.title.as_deref(), Some("Star Wars: Main Theme"));

        let m = guess_from_source(&json!({"title": "Archangel - Burial", "uploader": "Burial"}));
        assert_eq!(m.artist.as_deref(), Some("Burial"));
        assert_eq!(m.title.as_deref(), Some("Archangel"));

        let m = guess_from_source(&json!({"title": "Me at the zoo", "uploader": "jawed"}));
        assert_eq!(m.artist.as_deref(), Some("jawed"));
        assert_eq!(m.title.as_deref(), Some("Me at the zoo"));
    }

    #[test]
    fn scores_matches_sensibly() {
        let q = Query { title: "Archangel".into(), artist: Some("Burial".into()), duration: Some(238.0) };
        let good = Candidate {
            meta: TrackMeta { title: Some("Archangel".into()), artist: Some("Burial".into()), ..Default::default() },
            duration: Some(239.0),
            ..Default::default()
        };
        let wrong_artist = Candidate {
            meta: TrackMeta { title: Some("Archangel".into()), artist: Some("Two Steps From Hell".into()), ..Default::default() },
            duration: Some(160.0),
            ..Default::default()
        };
        assert!(score(&q, &good) > 0.95);
        assert!(score(&q, &wrong_artist) < 0.5);
        assert!(similarity("Get Lucky (feat. Pharrell Williams)", "Get Lucky") > 0.95);
        assert!(similarity("Queen Official", "Queen") > 0.8);
    }
}
