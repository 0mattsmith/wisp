import { FORMATS, formatHint, formatLabel, LOSSLESS, ORIGINAL, QUALITIES } from "../types";

/** Format pills plus a bitrate menu (hidden for lossless formats). */
export function FormatPicker({
  format,
  quality,
  onFormat,
  onQuality,
}: {
  format: string;
  quality: string;
  onFormat: (f: string) => void;
  onQuality: (q: string) => void;
}) {
  return (
    <div className="format-picker">
      <div className="segmented" role="radiogroup" aria-label="Output format">
        {FORMATS.map((f) => (
          <button key={f} role="radio" aria-checked={format === f} className={format === f ? "on" : ""} title={formatHint(f)} onClick={() => onFormat(f)}>
            {formatLabel(f)}
          </button>
        ))}
      </div>
      {format === ORIGINAL ? (
        <span className="lossless-note">Original quality</span>
      ) : LOSSLESS.includes(format) ? (
        <span className="lossless-note">Lossless</span>
      ) : (
        <select className="select" value={quality} onChange={(e) => onQuality(e.target.value)} aria-label="Quality">
          {QUALITIES.map((q) => (
            <option key={q.value} value={q.value}>
              {q.label}
            </option>
          ))}
        </select>
      )}
    </div>
  );
}
