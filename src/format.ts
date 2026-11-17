export function bytes(n: number | null | undefined): string {
  if (!n || n <= 0) return "";
  const units = ["B", "KB", "MB", "GB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i === 0 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

export function speed(n: number | null | undefined): string {
  const b = bytes(n);
  return b ? `${b}/s` : "";
}

export function clock(seconds: number | null | undefined): string {
  if (seconds == null || !isFinite(seconds) || seconds < 0) return "";
  const s = Math.round(seconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

export const SUPPORTED_LINK = /^(https?:\/\/)?([\w-]+\.)*(youtube\.com|youtu\.be|soundcloud\.com)\/\S+$/i;

/** Pull every link out of pasted text (one per line, or space separated). */
export function extractLinks(text: string): string[] {
  return text
    .split(/\s+/)
    .map((t) => t.trim())
    .filter((t) => /^https?:\/\//i.test(t) || SUPPORTED_LINK.test(t));
}

export function fileName(path: string | null): string {
  if (!path) return "";
  return path.split(/[\\/]/).pop() ?? path;
}
