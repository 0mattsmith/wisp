import { FolderOpen, LayoutGrid, List, Music, Pause, Pencil, Play, Plus, Repeat, Search, ShieldCheck, Sparkles, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { call, pickFiles, reveal } from "../api";
import { PageHeader, ToolButton } from "../components/Chrome";
import { bytes, clock, fileName } from "../format";
import { playTracks, togglePlayback, usePlayerState } from "../playerControl";
import { toast, useJobs, useSettings } from "../store";
import { FORMATS, type Job } from "../types";

type Layout = "grid" | "list";

const savedLayout = (): Layout => {
  try {
    return localStorage.getItem("library-layout") === "list" ? "list" : "grid";
  } catch {
    return "grid";
  }
};

export function Library({ onEdit }: { onEdit: (job: Job) => void }) {
  const jobs = useJobs();
  const settings = useSettings();
  const [query, setQuery] = useState("");
  const [menu, setMenu] = useState<string | null>(null);
  const [layout, setLayout] = useState<Layout>(savedLayout);
  const player = usePlayerState();

  useEffect(() => {
    try {
      localStorage.setItem("library-layout", layout);
    } catch {
      /* the choice just won't be remembered */
    }
  }, [layout]);

  const items = useMemo(() => {
    const q = query.trim().toLowerCase();
    return jobs
      .filter((j) => j.status === "done" && j.output)
      .filter((j) => !q || [j.title, j.artist, j.album, j.output].some((v) => v?.toLowerCase().includes(q)))
      .sort((a, b) => (b.finished ?? b.created) - (a.finished ?? a.created));
  }, [jobs, query]);

  const playing = (job: Job) => player.id === job.id && player.playing;

  const play = (job: Job) => {
    if (player.id === job.id) return void togglePlayback();
    // Whatever is listed (after any search) becomes the queue, starting here.
    const ids = items.map((i) => i.id);
    playTracks(ids, ids.indexOf(job.id));
  };

  const importFiles = async () => {
    const paths = await pickFiles("audio");
    if (!paths.length) return;
    const added = await call<number>("add_files", { paths, convert: false });
    toast(added ? `Added ${added} file${added === 1 ? "" : "s"}` : "Those files are already here");
  };

  const convert = async (job: Job, format: string) => {
    setMenu(null);
    await call("add_files", { paths: [job.output], convert: true, format, quality: settings?.quality });
    toast(`Converting to ${format.toUpperCase()} — see Downloads`);
  };

  const actions = (job: Job) => (
    <>
      <button className="icon-btn" title="Edit tags and artwork" onClick={() => onEdit(job)}>
        <Pencil size={15} />
      </button>
      <div className="menu-wrap">
        <button className="icon-btn" title="Convert to another format" onClick={() => setMenu(menu === job.id ? null : job.id)}>
          <Repeat size={15} />
        </button>
        {menu === job.id && (
          <div className="menu" onMouseLeave={() => setMenu(null)}>
            {FORMATS.filter((f) => f !== job.format).map((f) => (
              <button key={f} onClick={() => convert(job, f)}>
                Convert to {f.toUpperCase()}
              </button>
            ))}
          </div>
        )}
      </div>
      <button className="icon-btn" title="Show in folder" onClick={() => reveal(job.output!)}>
        <FolderOpen size={15} />
      </button>
      <button className="icon-btn" title="Remove from Library (the file is kept)" onClick={() => call("remove_job", { id: job.id })}>
        <X size={15} />
      </button>
    </>
  );

  return (
    <div className="view">
      <PageHeader overline="Music" title="Library">
        <label className="search-field">
          <Search size={14} />
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search" />
          {query && (
            <button className="clear" title="Clear search" onClick={() => setQuery("")}>
              <X size={12} />
            </button>
          )}
        </label>
        <div className="segmented icons" role="radiogroup" aria-label="Layout">
          <button role="radio" aria-checked={layout === "grid"} className={layout === "grid" ? "on" : ""} title="Grid" onClick={() => setLayout("grid")}>
            <LayoutGrid size={15} />
          </button>
          <button role="radio" aria-checked={layout === "list"} className={layout === "list" ? "on" : ""} title="List" onClick={() => setLayout("list")}>
            <List size={15} />
          </button>
        </div>
        <ToolButton label="Add audio files you already have" onClick={importFiles}>
          <Plus size={17} />
        </ToolButton>
      </PageHeader>

      {items.length === 0 ? (
        <div className="empty">
          <Music size={22} />
          <p>{query ? "No tracks match that search" : "Your library is empty"}</p>
          <span>{query ? "Try a different title or artist." : "Finished downloads appear here. You can also drop in files you already have."}</span>
        </div>
      ) : layout === "grid" ? (
        <div className="grid">
          {items.map((job) => (
            <div key={job.id} className={`tile${player.id === job.id ? " is-current" : ""}`}>
              <button className="tile-art" title={playing(job) ? "Pause" : "Play"} onClick={() => play(job)}>
                {job.thumbnail ? <img src={job.thumbnail} alt="" loading="lazy" /> : <Music size={30} />}
                <span className="tile-play">{playing(job) ? <Pause size={20} fill="currentColor" /> : <Play size={20} fill="currentColor" />}</span>
                {job.workaround && (
                  <span className={`tile-badge ${job.workaround}`} title={job.workaround === "drm" ? "Unprotected copy" : "Full track"}>
                    <ShieldCheck size={11} />
                  </span>
                )}
              </button>
              <div className="tile-text">
                <strong title={job.title}>{job.title}</strong>
                <span>{job.artist ?? fileName(job.output)}</span>
              </div>
              <div className="tile-actions">{actions(job)}</div>
            </div>
          ))}
        </div>
      ) : (
        <div className="rows">
          {items.map((job) => (
            <div key={job.id} className={`row lib-row${player.id === job.id ? " is-current" : ""}`}>
              <button className="thumb playable" title={playing(job) ? "Pause" : "Play"} onClick={() => play(job)}>
                {job.thumbnail ? <img src={job.thumbnail} alt="" loading="lazy" /> : <Music size={20} />}
                <span className="thumb-play">{playing(job) ? <Pause size={18} fill="currentColor" /> : <Play size={18} fill="currentColor" />}</span>
              </button>
              <div className="row-main">
                <div className="row-top">
                  <div className="row-title" title={job.output ?? ""}>
                    {job.title}
                  </div>
                  <div className="row-actions always">{actions(job)}</div>
                </div>
                <div className="row-sub">{[job.artist, job.album].filter(Boolean).join(" · ") || fileName(job.output)}</div>
                <div className="chips">
                  <span className="chip strong">{job.format.toUpperCase()}</span>
                  {job.size ? <span className="chip">{bytes(job.size)}</span> : null}
                  {job.duration ? <span className="chip">{clock(job.duration)}</span> : null}
                  {job.workaround && (
                    <span className={`chip workaround ${job.workaround}`}>
                      <ShieldCheck size={11} /> {job.workaround === "drm" ? "Unprotected copy" : "Full track"}
                    </span>
                  )}
                  {job.matched && (
                    <span className="chip matched">
                      <Sparkles size={11} /> {job.matched}
                    </span>
                  )}
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
