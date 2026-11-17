import { ImagePlus, LoaderCircle, Music, Search, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";
import { call, openExternal, pickFiles } from "../api";
import { bytes, clock, fileName } from "../format";
import { toast } from "../store";
import type { Candidate, FileTags, Job, TrackMeta } from "../types";

type Cover = { kind: "keep" } | { kind: "remove" } | { kind: "url"; value: string } | { kind: "file"; value: string };

const TEXT_FIELDS: { key: keyof TrackMeta; label: string; wide?: boolean }[] = [
  { key: "title", label: "Title", wide: true },
  { key: "artist", label: "Artist", wide: true },
  { key: "album", label: "Album" },
  { key: "albumArtist", label: "Album artist" },
  { key: "genre", label: "Genre" },
  { key: "year", label: "Year" },
  { key: "track", label: "Track" },
  { key: "disc", label: "Disc" },
];
const NUMERIC: (keyof TrackMeta)[] = ["year", "track", "disc"];

type Form = Record<string, string>;

function toForm(meta: TrackMeta): Form {
  const out: Form = {};
  for (const { key } of TEXT_FIELDS) out[key] = meta[key] == null ? "" : String(meta[key]);
  out.comment = meta.comment ?? "";
  return out;
}

function toMeta(form: Form, original: TrackMeta): TrackMeta {
  const text = (k: string) => form[k]?.trim() || null;
  const num = (k: string) => {
    const n = parseInt(form[k], 10);
    return Number.isFinite(n) && n > 0 ? n : null;
  };
  return {
    title: text("title"),
    artist: text("artist"),
    album: text("album"),
    albumArtist: text("albumArtist"),
    genre: text("genre"),
    comment: text("comment"),
    year: num("year"),
    track: num("track"),
    disc: num("disc"),
    // Keep the total only while the track number it belongs to is still there.
    trackTotal: num("track") ? original.trackTotal : null,
  };
}

export function TagEditor({ job, onClose }: { job: Job; onClose: () => void }) {
  const path = job.output!;
  const [tags, setTags] = useState<FileTags | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [form, setForm] = useState<Form>({});
  const [cover, setCover] = useState<Cover>({ kind: "keep" });
  const [rename, setRename] = useState(false);
  const [saving, setSaving] = useState(false);
  const [searching, setSearching] = useState(false);
  const [results, setResults] = useState<Candidate[] | null>(null);
  const [q, setQ] = useState({ title: "", artist: "" });

  const search = async (title: string, artist: string, duration: number | null) => {
    if (!title.trim()) return;
    setSearching(true);
    try {
      setResults(await call<Candidate[]>("search_metadata", { query: { title, artist: artist.trim() || null, duration } }));
    } catch (e) {
      toast(String(e), "error");
    } finally {
      setSearching(false);
    }
  };

  useEffect(() => {
    call<FileTags>("read_tags", { path })
      .then((t) => {
        setTags(t);
        setForm(toForm(t.meta));
        const title = t.meta.title ?? job.title;
        const artist = t.meta.artist ?? job.artist ?? "";
        setQ({ title, artist });
        search(title, artist, t.duration);
      })
      .catch((e) => setLoadError(String(e)));
    // Loads once for the file this editor was opened on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const apply = (c: Candidate) => {
    setForm((f) => {
      const next = { ...f };
      for (const { key } of TEXT_FIELDS) {
        const v = c.meta[key];
        if (v != null && v !== "") next[key] = String(v);
      }
      return next;
    });
    if (c.coverUrl) setCover({ kind: "url", value: c.coverUrl });
    if (tags && c.meta.trackTotal) setTags({ ...tags, meta: { ...tags.meta, trackTotal: c.meta.trackTotal } });
  };

  const save = async () => {
    if (!tags) return;
    setSaving(true);
    try {
      await call("write_tags", { jobId: job.id, path, meta: toMeta(form, tags.meta), cover, rename });
      toast("Tags saved");
      onClose();
    } catch (e) {
      toast(String(e), "error");
    } finally {
      setSaving(false);
    }
  };

  const preview = cover.kind === "url" ? cover.value : cover.kind === "keep" ? tags?.cover : null;

  return (
    <div className="modal-back" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal" role="dialog" aria-label="Edit tags">
        <header className="modal-head">
          <div>
            <h2>Edit tags</h2>
            <p title={path}>
              {fileName(path)}
              {tags && (
                <>
                  {" · "}
                  {[tags.format.toUpperCase(), tags.bitrate ? `${tags.bitrate} kbps` : "", clock(tags.duration), bytes(tags.size)]
                    .filter(Boolean)
                    .join(" · ")}
                </>
              )}
            </p>
          </div>
          <button className="icon-btn" onClick={onClose} title="Close">
            <X size={17} />
          </button>
        </header>

        {loadError ? (
          <div className="empty">
            <p>{loadError}</p>
          </div>
        ) : !tags ? (
          <div className="empty">
            <LoaderCircle className="spin" size={22} />
          </div>
        ) : (
          <div className="modal-body">
            <div className="editor">
              <div className="cover-col">
                <div className="cover">
                  {preview ? (
                    <img src={preview} alt="Cover art" referrerPolicy="no-referrer" />
                  ) : cover.kind === "file" ? (
                    <span className="cover-note">{fileName(cover.value)}</span>
                  ) : (
                    <Music size={34} />
                  )}
                </div>
                <div className="cover-actions">
                  <button
                    className="soft-btn"
                    onClick={async () => {
                      const [file] = await pickFiles("image");
                      if (file) setCover({ kind: "file", value: file });
                    }}
                  >
                    <ImagePlus size={14} /> Choose image
                  </button>
                  <button className="icon-btn" title="Remove artwork" onClick={() => setCover({ kind: "remove" })}>
                    <Trash2 size={15} />
                  </button>
                </div>
              </div>

              <div className="fields">
                {TEXT_FIELDS.map(({ key, label, wide }) => (
                  <label key={key} className={wide ? "wide" : ""}>
                    <span>{label}</span>
                    <input
                      value={form[key] ?? ""}
                      inputMode={NUMERIC.includes(key) ? "numeric" : undefined}
                      onChange={(e) => setForm({ ...form, [key]: e.target.value })}
                    />
                  </label>
                ))}
                <label className="wide">
                  <span>Comment</span>
                  <input value={form.comment ?? ""} onChange={(e) => setForm({ ...form, comment: e.target.value })} />
                </label>
              </div>
            </div>

            <div className="matches">
              <h3>Find a match</h3>
              <form
                className="match-search"
                onSubmit={(e) => {
                  e.preventDefault();
                  search(q.title, q.artist, tags.duration);
                }}
              >
                <input value={q.title} onChange={(e) => setQ({ ...q, title: e.target.value })} placeholder="Title" />
                <input value={q.artist} onChange={(e) => setQ({ ...q, artist: e.target.value })} placeholder="Artist" />
                <button className="soft-btn" type="submit" disabled={searching}>
                  {searching ? <LoaderCircle className="spin" size={14} /> : <Search size={14} />}
                </button>
              </form>

              <div className="match-list">
                {searching && !results && <p className="muted">Searching MusicBrainz, iTunes, Deezer and Discogs…</p>}
                {results?.length === 0 && <p className="muted">No matches. Try a simpler title or check the artist spelling.</p>}
                {results?.map((c, i) => (
                  <button key={i} className="match" onClick={() => apply(c)} title="Use these tags">
                    <div className="match-art">
                      {c.thumbUrl ? (
                        <img
                          src={c.thumbUrl}
                          alt=""
                          loading="lazy"
                          referrerPolicy="no-referrer"
                          onError={(e) => (e.currentTarget.style.visibility = "hidden")}
                        />
                      ) : (
                        <Music size={16} />
                      )}
                    </div>
                    <div className="match-text">
                      <strong>{c.meta.title}</strong>
                      <span>{[c.meta.artist, c.meta.album, c.meta.year].filter(Boolean).join(" · ")}</span>
                    </div>
                    <div className="match-side">
                      <span className={`score ${c.score >= 0.85 ? "high" : c.score >= 0.65 ? "mid" : "low"}`}>{Math.round(c.score * 100)}%</span>
                      <span
                        className="source"
                        onClick={(e) => {
                          if (c.link) {
                            e.stopPropagation();
                            openExternal(c.link);
                          }
                        }}
                      >
                        {c.source}
                      </span>
                    </div>
                  </button>
                ))}
              </div>
            </div>
          </div>
        )}

        <footer className="modal-foot">
          <label className="check">
            <input type="checkbox" checked={rename} onChange={(e) => setRename(e.target.checked)} />
            Rename the file to match
          </label>
          <div className="spacer" />
          <button className="soft-btn" onClick={onClose}>
            Cancel
          </button>
          <button className="primary-btn small" onClick={save} disabled={!tags || saving}>
            {saving ? "Saving…" : "Save tags"}
          </button>
        </footer>
      </div>
    </div>
  );
}
