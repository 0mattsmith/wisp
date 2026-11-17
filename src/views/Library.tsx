import { FolderOpen, Pause, Pencil, Play, Plus, Repeat, Search, Sparkles, X } from "lucide-react";
import { useMemo, useState } from "react";
import { call, pickFiles, reveal } from "../api";
import { bytes, clock, fileName } from "../format";
import { playTracks, togglePlayback, usePlayerState } from "../playerControl";
import { toast, useJobs, useSettings } from "../store";
import { FORMATS, type Job } from "../types";
import { Music } from "lucide-react";

export function Library({ onEdit }: { onEdit: (job: Job) => void }) {
  const jobs = useJobs();
  const settings = useSettings();
  const [query, setQuery] = useState("");
  const [menu, setMenu] = useState<string | null>(null);
  const player = usePlayerState();

  const items = useMemo(() => {
    const q = query.trim().toLowerCase();
    return jobs
      .filter((j) => j.status === "done" && j.output)
      .filter((j) => !q || [j.title, j.artist, j.album, j.output].some((v) => v?.toLowerCase().includes(q)))
      .sort((a, b) => (b.finished ?? b.created) - (a.finished ?? a.created));
  }, [jobs, query]);

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

  return (
    <div className="view">
      <header className="page-head">
        <div>
          <h1>Library</h1>
          <p className="hero-sub">Everything you have downloaded, converted or added. Edit tags and artwork at any time.</p>
        </div>
        <button className="soft-btn" onClick={importFiles}>
          <Plus size={14} /> Add files
        </button>
      </header>

      <div className="search-box">
        <Search size={15} />
        <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search title, artist or album" />
        {items.length > 0 && <span className="count">{items.length} tracks</span>}
      </div>

      {items.length === 0 ? (
        <div className="empty">
          <Music size={22} />
          <p>{query ? "No tracks match that search." : "Your library is empty."}</p>
          <span>{query ? "Try a different title or artist." : "Finished downloads appear here. You can also drop in files you already have."}</span>
        </div>
      ) : (
        <div className="rows">
          {items.map((job) => (
            <div key={job.id} className={`row lib-row${player.id === job.id ? " is-current" : ""}`}>
              <button className="thumb playable" title={player.id === job.id && player.playing ? "Pause" : "Play"} onClick={() => play(job)}>
                {job.thumbnail ? <img src={job.thumbnail} alt="" loading="lazy" /> : <Music size={20} />}
                <span className="thumb-play">{player.id === job.id && player.playing ? <Pause size={18} fill="currentColor" /> : <Play size={18} fill="currentColor" />}</span>
              </button>
              <div className="row-main">
                <div className="row-top">
                  <div className="row-title" title={job.output ?? ""}>
                    {job.title}
                  </div>
                  <div className="row-actions always">
                    <button
                      className={`icon-btn${player.id === job.id && player.playing ? " lit" : ""}`}
                      title={player.id === job.id && player.playing ? "Pause" : "Play"}
                      onClick={() => play(job)}
                    >
                      {player.id === job.id && player.playing ? <Pause size={15} /> : <Play size={15} />}
                    </button>
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
                  </div>
                </div>
                <div className="row-sub">{[job.artist, job.album].filter(Boolean).join(" · ") || fileName(job.output)}</div>
                <div className="chips">
                  <span className="chip strong">{job.format.toUpperCase()}</span>
                  {job.size ? <span className="chip">{bytes(job.size)}</span> : null}
                  {job.duration ? <span className="chip">{clock(job.duration)}</span> : null}
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
