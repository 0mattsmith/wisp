import { ArrowDown, ArrowDownToLine, Check, ClipboardPaste, FileAudio, LoaderCircle, Music, Pause, Play, Search, Trash2, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { call, pickFiles, readClipboard } from "../api";
import { NotLoaded, PageHeader, ToolButton } from "../components/Chrome";
import { FormatPicker } from "../components/FormatPicker";
import { JobRow } from "../components/JobRow";
import { clock, extractLinks, SUPPORTED_LINK } from "../format";
import { addLinks, saveSettings, toast, useJobs, useSettings } from "../store";
import { playTracks, togglePlayback, usePlayerState } from "../playerControl";
import { isActive, type Job, type SearchHit } from "../types";

interface SearchState {
  query: string;
  site: "youtube" | "soundcloud";
  loading: boolean;
  hits: SearchHit[];
  error: string | null;
  /** Links already sent to the queue from these results. */
  added: string[];
}

export function Downloads({ onEdit }: { onEdit: (job: Job) => void }) {
  const jobs = useJobs();
  const settings = useSettings();
  const player = usePlayerState();
  const [text, setText] = useState("");
  const [suggestion, setSuggestion] = useState<string | null>(null);
  const [search, setSearch] = useState<SearchState | null>(null);
  const searchRun = useRef(0);
  const seenClip = useRef("");
  const input = useRef<HTMLTextAreaElement>(null);

  const queue = useMemo(() => jobs.filter((j) => !j.cleared && j.kind !== "import").slice().reverse(), [jobs]);
  const activeCount = queue.filter((j) => isActive(j) || j.status === "queued").length;
  const pausedCount = queue.filter((j) => j.status === "paused").length;
  const finishedCount = queue.filter((j) => ["done", "failed", "canceled"].includes(j.status)).length;

  // Offer a supported link that is sitting on the clipboard, without adding it uninvited.
  useEffect(() => {
    if (!settings?.clipboardWatch) return;
    const look = async () => {
      const clip = (await readClipboard()).trim();
      if (clip && clip !== seenClip.current && SUPPORTED_LINK.test(clip)) {
        seenClip.current = clip;
        setSuggestion(clip);
      }
    };
    look();
    window.addEventListener("focus", look);
    return () => window.removeEventListener("focus", look);
  }, [settings?.clipboardWatch]);

  if (!settings) {
    return (
      <div className="view">
        <PageHeader overline="Music" title="Downloads" />
        <NotLoaded />
      </div>
    );
  }

  const runSearch = async (query: string, site: SearchState["site"]) => {
    const run = ++searchRun.current;
    setSearch({ query, site, loading: true, hits: [], error: null, added: [] });
    try {
      const hits = await call<SearchHit[]>("search_tracks", { query, site });
      // Ignore the answer to a search that has since been replaced or closed.
      if (run === searchRun.current) setSearch({ query, site, loading: false, hits, error: null, added: [] });
    } catch (e) {
      if (run === searchRun.current) setSearch({ query, site, loading: false, hits: [], error: String(e), added: [] });
    }
  };

  const closeSearch = () => {
    searchRun.current++;
    setSearch(null);
  };

  const submit = async (raw = text) => {
    const links = extractLinks(raw);
    if (links.length === 0) {
      // Not a link: treat it as the name of a song to look for.
      if (raw.trim()) return runSearch(raw.trim(), search?.site ?? "youtube");
      toast("Paste a link or type a song name first", "error");
      return;
    }
    closeSearch();
    const added = await addLinks(links, settings.format, settings.quality);
    if (added > 0) {
      setText("");
      setSuggestion(null);
    }
  };

  const convertFiles = async () => {
    const paths = await pickFiles("audio");
    if (paths.length) await call("add_files", { paths, convert: true, format: settings.format, quality: settings.quality });
  };

  const looksLikeSearch = text.trim() !== "" && extractLinks(text).length === 0;

  return (
    <div className="view">
      <PageHeader overline="Music" title="Downloads">
        <ToolButton label="Convert audio files from your computer" onClick={convertFiles}>
          <FileAudio size={16} />
        </ToolButton>
        {activeCount > 0 && (
          <ToolButton label="Pause all" onClick={() => call("pause_all")}>
            <Pause size={16} />
          </ToolButton>
        )}
        {pausedCount > 0 && (
          <ToolButton label="Resume all" onClick={() => call("resume_all")}>
            <Play size={16} />
          </ToolButton>
        )}
        {finishedCount > 0 && (
          <ToolButton label="Clear finished (they stay in your Library)" onClick={() => call("clear_finished")}>
            <Trash2 size={16} />
          </ToolButton>
        )}
      </PageHeader>

      <section className="composer-card">
        <div className="composer">
          <textarea
            ref={input}
            rows={1}
            value={text}
            placeholder="Paste a YouTube or SoundCloud link, or search for a song…"
            spellCheck={false}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                submit();
              }
            }}
            onPaste={(e) => {
              // Several links pasted at once go straight to the queue.
              const pasted = e.clipboardData.getData("text");
              if (extractLinks(pasted).length > 1 && !text.trim()) {
                e.preventDefault();
                submit(pasted);
              }
            }}
          />
          <button
            className="ghost-btn"
            title="Paste from clipboard"
            onClick={async () => {
              const clip = await readClipboard();
              if (clip) setText(clip.trim());
              input.current?.focus();
            }}
          >
            <ClipboardPaste size={16} />
          </button>
          <button className="send-btn" title={looksLikeSearch ? "Search" : "Download"} onClick={() => submit()}>
            {looksLikeSearch ? <Search size={17} strokeWidth={2.4} /> : <ArrowDown size={18} strokeWidth={2.6} />}
          </button>
        </div>
        <div className="composer-foot">
          <FormatPicker
            format={settings.format}
            quality={settings.quality}
            onFormat={(format) => saveSettings({ format })}
            onQuality={(quality) => saveSettings({ quality })}
          />
          {suggestion && !text && (
            <button className="suggestion" onClick={() => submit(suggestion)}>
              <ClipboardPaste size={13} />
              <span>Download copied link</span>
              <em>{suggestion.replace(/^https?:\/\/(www\.)?/, "")}</em>
            </button>
          )}
        </div>

        {search && (
          <div className="results">
            <header>
              <span className="results-title">Results for “{search.query}”</span>
              <div className="segmented tiny">
                {(["youtube", "soundcloud"] as const).map((site) => (
                  <button key={site} className={search.site === site ? "on" : ""} onClick={() => search.site !== site && runSearch(search.query, site)}>
                    {site === "youtube" ? "YouTube" : "SoundCloud"}
                  </button>
                ))}
              </div>
              <button className="icon-btn" title="Close results" onClick={closeSearch}>
                <X size={15} />
              </button>
            </header>
            {search.loading ? (
              <p className="results-note">
                <LoaderCircle className="spin" size={15} /> Searching…
              </p>
            ) : search.error ? (
              <p className="results-note error">{search.error}</p>
            ) : search.hits.length === 0 ? (
              <p className="results-note">Nothing found. Try adding the artist's name.</p>
            ) : (
              <div className="results-list">
                {search.hits.map((hit) => {
                  const done = search.added.includes(hit.url);
                  return (
                    <div key={hit.url} className="result">
                      <div className="thumb small">
                        {hit.thumbnail ? <img src={hit.thumbnail} alt="" loading="lazy" referrerPolicy="no-referrer" /> : <Music size={16} />}
                      </div>
                      <div className="result-text">
                        <strong title={hit.title}>{hit.title}</strong>
                        <span>{[hit.uploader, clock(hit.duration)].filter(Boolean).join(" · ")}</span>
                      </div>
                      <button
                        className={`soft-btn${done ? "" : " accent"}`}
                        disabled={done}
                        onClick={async () => {
                          await addLinks([hit.url], settings.format, settings.quality);
                          setSearch((s) => (s ? { ...s, added: [...s.added, hit.url] } : s));
                        }}
                      >
                        {done ? (
                          <>
                            <Check size={14} /> Added
                          </>
                        ) : (
                          <>
                            <ArrowDownToLine size={14} /> Download
                          </>
                        )}
                      </button>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        )}
      </section>

      <section className="queue">
        <h2 className="group-title">
          Queue
          {activeCount > 0 && <span className="count">{activeCount} in progress</span>}
        </h2>

        {queue.length === 0 ? (
          <div className="empty">
            <ArrowDownToLine size={22} />
            <p>Nothing queued yet</p>
            <span>Paste a link above, or drop audio files here to convert them.</span>
          </div>
        ) : (
          <div className="rows">
            {queue.map((job) => (
              <JobRow
                key={job.id}
                job={job}
                onEdit={onEdit}
                nowPlaying={player.id === job.id && player.playing}
                onPlay={(j) => {
                  if (player.id === j.id) return void togglePlayback();
                  // The rest of the finished tracks in the queue follow on.
                  const ids = queue.filter((q) => q.status === "done" && q.output).map((q) => q.id);
                  playTracks(ids, ids.indexOf(j.id));
                }}
              />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
