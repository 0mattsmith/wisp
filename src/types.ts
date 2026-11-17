export type Status =
  | "queued"
  | "fetching"
  | "downloading"
  | "converting"
  | "tagging"
  | "done"
  | "failed"
  | "canceled"
  | "paused";

export interface Job {
  id: string;
  kind: "download" | "convert" | "import" | "playlist";
  source: string;
  site: "youtube" | "soundcloud" | "web" | "file";
  title: string;
  artist: string | null;
  album: string | null;
  thumbnail: string | null;
  duration: number | null;
  format: string;
  quality: string;
  status: Status;
  progress: number;
  downloaded: number;
  total: number | null;
  speed: number | null;
  eta: number | null;
  error: string | null;
  output: string | null;
  size: number | null;
  matched: string | null;
  matchScore: number | null;
  cleared: boolean;
  attempts: number;
  /** Why the track came from elsewhere: "drm" or "preview". */
  workaround: "drm" | "preview" | null;
  /** What is happening right now, shown under the progress bar. */
  note: string | null;
  created: number;
  finished: number | null;
}

export interface Settings {
  outputDir: string;
  format: string;
  quality: string;
  maxConcurrent: number;
  filenameTemplate: string;
  autoTag: boolean;
  matchThreshold: number;
  embedCover: boolean;
  squareCover: boolean;
  useMusicbrainz: boolean;
  useItunes: boolean;
  useDeezer: boolean;
  useDiscogs: boolean;
  discogsToken: string;
  sponsorblock: boolean;
  cookiesBrowser: string;
  notify: boolean;
  keepInTray: boolean;
  hideDockIcon: boolean;
  autoUpdate: boolean;
  clipboardWatch: boolean;
}

export interface TrackMeta {
  title: string | null;
  artist: string | null;
  album: string | null;
  albumArtist: string | null;
  year: number | null;
  track: number | null;
  trackTotal: number | null;
  disc: number | null;
  genre: string | null;
  comment: string | null;
}

export interface Candidate {
  source: string;
  meta: TrackMeta;
  coverUrl: string | null;
  thumbUrl: string | null;
  duration: number | null;
  score: number;
  link: string | null;
}

export interface SearchHit {
  url: string;
  site: string;
  title: string;
  uploader: string | null;
  duration: number | null;
  thumbnail: string | null;
}

export interface FileTags {
  meta: TrackMeta;
  cover: string | null;
  duration: number | null;
  bitrate: number | null;
  sampleRate: number | null;
  format: string;
  size: number;
}

export interface ToolStatus {
  name: string;
  found: boolean;
  managed: boolean;
  path: string | null;
  version: string | null;
  installable: boolean;
}

export interface ToolProgress {
  name: string;
  downloaded: number;
  total: number | null;
  done: boolean;
}

export const FORMATS = ["mp3", "flac", "wav", "m4a", "aac", "ogg"] as const;
export const LOSSLESS = ["flac", "wav"];
export const QUALITIES = [
  { value: "best", label: "Best" },
  { value: "320", label: "320 kbps" },
  { value: "256", label: "256 kbps" },
  { value: "192", label: "192 kbps" },
  { value: "128", label: "128 kbps" },
];

export const ACTIVE: Status[] = ["fetching", "downloading", "converting", "tagging"];
export const isActive = (j: Job) => ACTIVE.includes(j.status);
