//! Reading and writing tags and cover art for every supported format
//! (ID3v2 for MP3/WAV/AAC, MP4 atoms for M4A, Vorbis comments for FLAC/OGG).

use crate::meta::TrackMeta;
use base64::Engine;
use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::file::TaggedFile;
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::Tag;
use serde::Serialize;
use std::path::Path;

/// Open a file, identifying its format from the contents rather than the
/// extension (an `.ogg` can hold Vorbis or Opus).
fn open(path: &Path) -> Result<TaggedFile, lofty::error::LoftyError> {
    Probe::open(path)?.guess_file_type()?.read()
}

pub enum CoverChange {
    Keep,
    Remove,
    /// JPEG bytes, already sized by `prepare_cover`.
    Set(Vec<u8>),
}

#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileTags {
    pub meta: TrackMeta,
    /// Preview-sized cover as a data URL.
    pub cover: Option<String>,
    pub duration: Option<f64>,
    pub bitrate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub format: String,
    pub size: u64,
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn meta_from_tag(tag: &Tag) -> TrackMeta {
    TrackMeta {
        title: non_empty(tag.title().map(|c| c.into_owned())),
        artist: non_empty(tag.artist().map(|c| c.into_owned())),
        album: non_empty(tag.album().map(|c| c.into_owned())),
        album_artist: non_empty(tag.get_string(&ItemKey::AlbumArtist).map(|s| s.to_string())),
        year: tag.year(),
        track: tag.track(),
        track_total: tag.track_total(),
        disc: tag.disk(),
        genre: non_empty(tag.genre().map(|c| c.into_owned())),
        comment: non_empty(tag.comment().map(|c| c.into_owned())),
    }
}

fn front_cover(tag: &Tag) -> Option<&Picture> {
    tag.pictures()
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| tag.pictures().first())
}

/// Tags plus the raw embedded cover, for copying between files.
pub fn read_raw(path: &Path) -> Result<(TrackMeta, Option<Vec<u8>>), String> {
    let file = open(path).map_err(|e| format!("Could not read tags: {e}"))?;
    let tag = file.primary_tag().or_else(|| file.first_tag());
    Ok(match tag {
        Some(t) => (meta_from_tag(t), front_cover(t).map(|p| p.data().to_vec())),
        None => (TrackMeta::default(), None),
    })
}

pub fn read(path: &Path) -> Result<FileTags, String> {
    let file = open(path).map_err(|e| format!("Could not read this file: {e}"))?;
    let props = file.properties();
    let tag = file.primary_tag().or_else(|| file.first_tag());
    let duration = props.duration().as_secs_f64();
    Ok(FileTags {
        meta: tag.map(meta_from_tag).unwrap_or_default(),
        cover: tag.and_then(front_cover).and_then(|p| data_url(p.data(), 420)),
        duration: (duration > 0.0).then_some(duration),
        bitrate: props.audio_bitrate(),
        sample_rate: props.sample_rate(),
        format: path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default(),
        size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
    })
}

/// Write tags to a file. With `fresh`, whatever the file already carries is
/// discarded first: used for new downloads, where the only existing tags are
/// leftovers from the source stream (container brands, encoder notes).
pub fn write(path: &Path, meta: &TrackMeta, cover: CoverChange, fresh: bool) -> Result<(), String> {
    let mut file = open(path).map_err(|e| format!("Could not open for tagging: {e}"))?;
    let tag_type = file.primary_tag_type();
    if fresh {
        file.clear();
    }
    if file.primary_tag().is_none() {
        file.insert_tag(Tag::new(tag_type));
    }
    let tag = file.primary_tag_mut().ok_or("This format cannot hold tags")?;

    macro_rules! text {
        ($value:expr, $set:ident, $remove:ident) => {
            match non_empty($value.clone()) {
                Some(v) => tag.$set(v),
                None => tag.$remove(),
            }
        };
    }
    macro_rules! number {
        ($value:expr, $set:ident, $remove:ident) => {
            match $value {
                Some(v) if v > 0 => tag.$set(v),
                _ => tag.$remove(),
            }
        };
    }
    text!(meta.title, set_title, remove_title);
    text!(meta.artist, set_artist, remove_artist);
    text!(meta.album, set_album, remove_album);
    text!(meta.genre, set_genre, remove_genre);
    text!(meta.comment, set_comment, remove_comment);
    number!(meta.year, set_year, remove_year);
    number!(meta.track, set_track, remove_track);
    number!(meta.track_total, set_track_total, remove_track_total);
    number!(meta.disc, set_disk, remove_disk);
    match non_empty(meta.album_artist.clone()) {
        Some(v) => {
            tag.insert_text(ItemKey::AlbumArtist, v);
        }
        None => tag.remove_key(&ItemKey::AlbumArtist),
    }

    match cover {
        CoverChange::Keep => {}
        CoverChange::Remove => {
            while !tag.pictures().is_empty() {
                tag.remove_picture(0);
            }
        }
        CoverChange::Set(bytes) => {
            while !tag.pictures().is_empty() {
                tag.remove_picture(0);
            }
            let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { MimeType::Png } else { MimeType::Jpeg };
            tag.push_picture(Picture::new_unchecked(PictureType::CoverFront, Some(mime), None, bytes));
        }
    }

    file.save_to_path(path, WriteOptions::default())
        .map_err(|e| format!("Could not save tags: {e}"))
}

/// Decode any supported image, optionally centre-crop to a square, cap its
/// size and re-encode as JPEG for embedding.
pub fn prepare_cover(bytes: &[u8], square: bool, max: u32) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("Unsupported image: {e}"))?;
    let (w, h) = (img.width(), img.height());
    let img = if square && w != h {
        let side = w.min(h);
        img.crop_imm((w - side) / 2, (h - side) / 2, side, side)
    } else {
        img
    };
    let img = if img.width().max(img.height()) > max {
        img.resize(max, max, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    encode_jpeg(&img, 90)
}

fn encode_jpeg(img: &image::DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    img.to_rgb8().write_with_encoder(encoder).map_err(|e| e.to_string())?;
    Ok(out)
}

/// A small square JPEG as a data URL, used for list thumbnails and previews.
pub fn data_url(bytes: &[u8], size: u32) -> Option<String> {
    let img = image::load_from_memory(bytes).ok()?;
    let thumb = img.resize_to_fill(size, size, image::imageops::FilterType::Triangle);
    let jpeg = encode_jpeg(&thumb, 78).ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trips tags and artwork through every output format. Needs sample
    /// files, so it only runs when pointed at a folder of them:
    /// `DUBPLATE_TAG_SAMPLES=/path cargo test tag_roundtrip -- --ignored`
    #[test]
    #[ignore]
    fn tag_roundtrip() {
        let dir = std::env::var("DUBPLATE_TAG_SAMPLES").expect("set DUBPLATE_TAG_SAMPLES");
        let cover = {
            let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(640, 360, image::Rgb([200, 60, 160])));
            let mut png = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
            prepare_cover(&png, true, 1400).unwrap()
        };
        let meta = TrackMeta {
            title: Some("Tïtle / Test".into()),
            artist: Some("Artist & Co".into()),
            album: Some("Album".into()),
            album_artist: Some("Album Artist".into()),
            year: Some(2021),
            track: Some(4),
            track_total: Some(12),
            disc: Some(1),
            genre: Some("Electronic".into()),
            comment: None,
        };
        let mut checked = 0;
        for ext in crate::settings::FORMATS {
            let path = Path::new(&dir).join(format!("sample.{ext}"));
            write(&path, &meta, CoverChange::Set(cover.clone()), true).unwrap_or_else(|e| panic!("{ext}: {e}"));
            let back = read(&path).unwrap_or_else(|e| panic!("{ext}: {e}"));
            assert_eq!(back.meta, meta, "{ext} tags");
            assert!(back.cover.is_some(), "{ext} cover");
            let (_, raw) = read_raw(&path).unwrap();
            let img = image::load_from_memory(&raw.unwrap()).unwrap();
            assert_eq!((img.width(), img.height()), (360, 360), "{ext} cover is cropped square");

            // Editing: change one field, drop the artwork, keep the rest.
            let edited = TrackMeta { title: Some("Renamed".into()), genre: None, ..meta.clone() };
            write(&path, &edited, CoverChange::Remove, false).unwrap();
            let back = read(&path).unwrap();
            assert_eq!(back.meta, edited, "{ext} edited tags");
            assert!(back.cover.is_none(), "{ext} cover removed");
            checked += 1;
        }
        assert_eq!(checked, 6);
    }
}
