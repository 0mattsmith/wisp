# Wisp

Download, convert and tag music from YouTube and SoundCloud. A light, native-feeling desktop app for macOS (Apple silicon and Intel), Windows and Linux, built with Rust and Tauri. Light and dark themes, a menu bar quick panel, and a pop-out player.

*Wisp was called Dubplate before v0.1.8. Its internal app ID (`app.dubplate.desktop`) and `dubplate://` links are kept so existing installs, libraries and bookmarks carry on working.*

## What it does

- **Search** by song name on YouTube or SoundCloud, or paste a link.
- **Download** single tracks, playlists, sets and channels with yt-dlp, several at a time. Network timeouts are retried automatically.
- **Organise** files with a naming pattern such as `{albumartist}/{album}/{track} {title}`; a `/` starts a folder.
- **Never stuck with a clip:** when a SoundCloud track is DRM-protected or only offers a 30-second preview, Wisp identifies the exact recording (artist, title, version and length, cross-checked with the music databases) and downloads that recording from YouTube instead. Nothing is downloaded unless the match is exact.
- **Convert** to MP3, FLAC, WAV, M4A, AAC or OGG, or choose **None** to keep the original: SoundCloud audio exactly as served, and YouTube's video (preferring H.264 MP4 and M4A audio, which play on any Mac) with ffmpeg. Where the source already holds the target codec (for example M4A from YouTube) the audio is copied rather than re-encoded.
- **Tag** automatically from iTunes, Deezer, MusicBrainz (with Cover Art Archive) and Discogs, including cover art. Every file can be re-tagged by hand in the editor.
- **Convert and tag files you already have** by dropping them on the window.
- **Send from your browser** with a "Send to Wisp" bookmark button, or (macOS) from the Share menu via a Shortcut. Both open a `wisp://add?url=…` link, which only accepts YouTube and SoundCloud pages.
- **Signed-in downloads** for age-restricted or members-only items, using cookies from Chrome, Safari, Firefox, Edge, Brave, Opera, Vivaldi, Chromium, Helium, or any other browser picked under "Custom…". On macOS, Chromium-family browsers that yt-dlp cannot read (Helium, Arc, Thorium…) are unlocked through their own keychain entry, and only YouTube, Google and SoundCloud cookies are passed on, in a private file deleted after each download.
- **Play** anything in the Library in a pop-out mini player that floats above other windows.
- **Menu bar / tray** quick panel, optional start at login (straight into the menu bar), and background updates.

## Run it

```bash
npm install
```

```bash
npm run tauri dev
```

yt-dlp and ffmpeg are found on your PATH (Homebrew, winget, apt and so on). If they are missing, install them from Settings → Tools inside the app.

`npm run dev` on its own opens the interface in a browser with sample data, which is handy for design work.

To try things without touching your real library, point a development build at a scratch folder:

```bash
WISP_DEV_DIR=/tmp/wisp-dev npm run tauri dev
```

## Build installers

```bash
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/dubplate-updater.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" npm run tauri build
```

## Releases and updates

```bash
./push "What changed"
```

commits everything and pushes it to GitHub. Add `--bump` to release it as well:

```bash
./push "What changed" --bump
```

That raises the version (0.1.0 → 0.1.1; use `--bump minor`, `--bump major` or `--bump 1.2.3` for other steps), tags it, and pushes the tag. GitHub then builds installers for every platform and publishes the release once all of them succeed, with your message as the release notes. Installed copies check `latest.json` on that release, download the new version in the background, and swap it in when the user clicks **Restart to update**. Add `--dry-run` to either command to see what it would do.

Releases are signed with the key in `~/.tauri/dubplate-updater.key`, stored in the repo as the `TAURI_SIGNING_PRIVATE_KEY` secret. Keep that file safe: installed copies reject updates signed with any other key.

Notes per platform:

- **macOS**: updates work without an Apple Developer account, but an unsigned app shows a Gatekeeper warning on first launch. To sign and notarise, add the `APPLE_*` repository secrets and uncomment the matching lines in `.github/workflows/release.yml`.
- **Windows**: the installer runs in passive mode after the click, then the app reopens.
- **Linux**: in-app updates apply to the AppImage. `.deb` and `.rpm` installs update through the package.

## Layout

| Path | What lives there |
| --- | --- |
| `src-tauri/src/jobs.rs` | Queue, yt-dlp and ffmpeg pipelines, library |
| `src-tauri/src/meta.rs` | Title parsing, metadata providers, match scoring |
| `src-tauri/src/tags.rs` | Tag and cover art reading/writing |
| `src-tauri/src/tools.rs` | Finding, installing and updating yt-dlp / ffmpeg |
| `src-tauri/src/tray.rs` | Menu bar icon and quick panel |
| `src/` | React interface (`Panel.tsx` is the menu bar panel, `Player.tsx` the pop-out player) |
| `branding/` | Logo and menu bar icon sources |
