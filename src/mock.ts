// Sample data used only when the UI is opened in a plain browser (npm run dev)
// rather than inside the app. It lets the interface be designed and checked
// without the Rust backend.
import type { Candidate, FileTags, Job, Settings, ToolStatus } from "./types";

const art = (a: string, b: string) =>
  "data:image/svg+xml," +
  encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 96 96"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs><rect width="96" height="96" fill="url(#g)"/><circle cx="48" cy="48" r="22" fill="none" stroke="rgba(255,255,255,.55)" stroke-width="3"/><circle cx="48" cy="48" r="5" fill="rgba(255,255,255,.7)"/></svg>`,
  );

const base: Job = {
  id: "",
  kind: "download",
  source: "",
  site: "youtube",
  title: "",
  artist: null,
  album: null,
  thumbnail: null,
  duration: null,
  format: "mp3",
  quality: "best",
  status: "queued",
  progress: 0,
  downloaded: 0,
  total: null,
  speed: null,
  eta: null,
  error: null,
  output: null,
  size: null,
  matched: null,
  matchScore: null,
  cleared: false,
  attempts: 0,
  workaround: null,
  note: null,
  created: Date.now(),
  finished: null,
};

let jobs: Job[] = [
  { ...base, id: "7", title: "Glass Harbour", artist: "Lowtide Assembly", album: "Slow Signals", thumbnail: art("#0ea5e9", "#1e3a8a"), duration: 301, format: "flac", status: "done", progress: 1, output: "/Users/you/Music/Dubplate/Lowtide Assembly - Glass Harbour.flac", size: 38_400_000, matched: "MusicBrainz", matchScore: 0.93, cleared: true },
  { ...base, id: "6", title: "Northern Line (Night Edit)", artist: "Marlowe Kite", thumbnail: art("#f97316", "#7c2d12"), site: "soundcloud", status: "failed", error: "Not available: this track was removed by the uploader." },
  { ...base, id: "5", title: "Paper Lanterns", artist: "Ines Vauquelin", album: "Small Hours", thumbnail: art("#f056c8", "#4c1d95"), duration: 214, status: "done", progress: 1, output: "/Users/you/Music/Dubplate/Ines Vauquelin - Paper Lanterns.mp3", size: 8_612_000, matched: "iTunes", matchScore: 0.97 },
  { ...base, id: "4", title: "Halcyon Drift", artist: "The Quiet Arcade", thumbnail: art("#22c55e", "#064e3b"), duration: 262, status: "paused", progress: 0.41 },
  { ...base, id: "9", title: "World On Fire (Re-Mastered)", artist: "The Royal Concept", thumbnail: art("#f43f5e", "#4c0519"), duration: 200, site: "soundcloud", status: "fetching", workaround: "drm", note: "Finding an unprotected copy…" },
  { ...base, id: "10", title: "D-D-Dance", artist: "The Royal Concept", thumbnail: art("#38bdf8", "#0c4a6e"), duration: 222, site: "soundcloud", status: "downloading", progress: 0.4, downloaded: 3_100_000, total: 8_000_000, speed: 2_400_000, eta: 2, workaround: "preview", note: "Downloading the full track" },
  { ...base, id: "8", title: "Goldrushed (Re-Mastered)", artist: "The Royal Concept", album: "Goldrushed", thumbnail: art("#fb923c", "#7c2d12"), duration: 226, site: "soundcloud", status: "done", progress: 1, workaround: "drm", output: "/Users/you/Music/Dubplate/The Royal Concept - Goldrushed.mp3", size: 9_100_000, matched: "iTunes", matchScore: 0.98 },
  { ...base, id: "3", title: "Saltwater Radio", artist: "Odette & The Tides", thumbnail: art("#eab308", "#713f12"), duration: 187, site: "soundcloud", format: "m4a", status: "tagging", progress: 0.96 },
  { ...base, id: "2", title: "Afterimage (feat. Juno Reyes)", artist: "Kestrel Park", thumbnail: art("#8b5cf6", "#1e1b4b"), duration: 233, status: "downloading", progress: 0.52, downloaded: 4_300_000, total: 7_480_000, speed: 2_950_000, eta: 2 },
  { ...base, id: "1", title: "Sodium Lights", artist: "Marlowe Kite", thumbnail: art("#3b82f6", "#0f172a"), duration: 248, status: "queued" },
];

const settings: Settings = {
  outputDir: "/Users/you/Music/Dubplate",
  format: "mp3",
  quality: "best",
  maxConcurrent: 3,
  filenameTemplate: "{artist} - {title}",
  autoTag: true,
  matchThreshold: 0.78,
  embedCover: true,
  squareCover: true,
  useMusicbrainz: true,
  useItunes: true,
  useDeezer: true,
  useDiscogs: true,
  discogsToken: "",
  sponsorblock: false,
  cookiesBrowser: "",
  notify: true,
  keepInTray: true,
  hideDockIcon: false,
  autoUpdate: true,
  clipboardWatch: true,
};

const listeners = new Map<string, Set<(p: unknown) => void>>();
const emit = (event: string, payload: unknown) => listeners.get(event)?.forEach((l) => l(payload));

export function mockListen(event: string, cb: (p: unknown) => void) {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event)!.add(cb);
  return () => void listeners.get(event)!.delete(cb);
}

const patch = (id: string, change: Partial<Job>) => {
  jobs = jobs.map((j) => (j.id === id ? { ...j, ...change } : j));
  emit("job-update", jobs.find((j) => j.id === id));
};

export function mockCall(cmd: string, args: Record<string, unknown> = {}): unknown {
  const id = args.id as string;
  switch (cmd) {
    case "list_jobs":
      return jobs;
    case "get_settings":
      return settings;
    case "set_settings":
      Object.assign(settings, args.settings);
      return { ...settings };
    case "add_urls": {
      const urls = args.urls as string[];
      urls.forEach((u, i) => {
        const job: Job = { ...base, id: `n${Date.now()}${i}`, source: u, title: u, format: (args.format as string) ?? settings.format, created: Date.now() };
        jobs = [...jobs, job];
        emit("job-update", job);
      });
      return urls.length;
    }
    case "pause_job":
      return patch(id, { status: "paused", speed: null, eta: null });
    case "resume_job":
      return patch(id, { status: "queued", error: null });
    case "cancel_job":
      return patch(id, { status: "canceled" });
    case "remove_job":
      jobs = jobs.filter((j) => j.id !== id);
      return emit("job-removed", id);
    case "clear_finished":
      jobs.filter((j) => j.status === "done").forEach((j) => patch(j.id, { cleared: true }));
      jobs.filter((j) => ["failed", "canceled"].includes(j.status)).forEach((j) => mockCall("remove_job", { id: j.id }));
      return;
    case "tools_status":
      return [
        { name: "yt-dlp", found: true, managed: true, path: "/app/bin/yt-dlp", version: "2026.10.26", installable: true },
        { name: "ffmpeg", found: true, managed: false, path: "/opt/homebrew/bin/ffmpeg", version: "8.1.1", installable: true },
        { name: "ffprobe", found: true, managed: false, path: "/opt/homebrew/bin/ffprobe", version: "8.1.1", installable: true },
      ] satisfies ToolStatus[];
    case "search_tracks": {
      const site = args.site as string;
      const names = [
        ["Paper Lanterns", "Ines Vauquelin", 214, "#f056c8", "#4c1d95"],
        ["Paper Lanterns (Live at Halden Rooms)", "Ines Vauquelin", 251, "#fb7185", "#831843"],
        ["Paper Lanterns - Kestrel Park Remix", "Kestrel Park", 322, "#8b5cf6", "#1e1b4b"],
        ["Paper Lanterns (Acoustic)", "Odette & The Tides", 198, "#eab308", "#713f12"],
      ] as const;
      return names.map(([title, uploader, duration, a, b], i) => ({
        url: `https://${site === "soundcloud" ? "soundcloud.com/demo" : "www.youtube.com/watch?v=demo"}${i}`,
        site,
        title,
        uploader,
        duration,
        thumbnail: art(a, b),
      }));
    }
    case "bookmarklet":
      return "javascript:(()=>{location.href='dubplate://add?url='+encodeURIComponent(location.href)})()";
    case "player_pending":
      return null;
    case "ytdlp_latest":
      return "2026.10.26";
    case "read_tags":
      return {
        meta: { title: "Paper Lanterns", artist: "Ines Vauquelin", album: "Small Hours", albumArtist: "Ines Vauquelin", year: 2024, track: 3, trackTotal: 11, disc: 1, genre: "Indie Pop", comment: null },
        cover: art("#f056c8", "#4c1d95"),
        duration: 214,
        bitrate: 320,
        sampleRate: 44100,
        format: "mp3",
        size: 8_612_000,
      } satisfies FileTags;
    case "search_metadata": {
      const mk = (source: string, score: number, album: string, year: number, a: string, b: string): Candidate => ({
        source,
        score,
        duration: 214,
        link: null,
        coverUrl: art(a, b),
        thumbUrl: art(a, b),
        meta: { title: "Paper Lanterns", artist: "Ines Vauquelin", album, albumArtist: "Ines Vauquelin", year, track: 3, trackTotal: 11, disc: 1, genre: "Indie Pop", comment: null },
      });
      return [
        mk("iTunes", 0.97, "Small Hours", 2024, "#f056c8", "#4c1d95"),
        mk("Deezer", 0.95, "Small Hours", 2024, "#f056c8", "#4c1d95"),
        mk("MusicBrainz", 0.9, "Small Hours (Deluxe)", 2025, "#fb7185", "#831843"),
        mk("Discogs", 0.71, "Paper Lanterns EP", 2023, "#a78bfa", "#312e81"),
      ];
    }
    default:
      return undefined;
  }
}
