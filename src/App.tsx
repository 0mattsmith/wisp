import { ArrowDownToLine, Library as LibraryIcon, Music, Pause, Play, RefreshCw, Settings as SettingsIcon, TriangleAlert, Upload } from "lucide-react";
import { useEffect, useState } from "react";
import { call, isMac, on, onFileDrop } from "./api";
import { Logo, Wordmark } from "./components/Logo";
import { TagEditor } from "./components/TagEditor";
import { showPlayer, togglePlayback, usePlayerState } from "./playerControl";
import { toast, useJobs, useSettings, useToasts } from "./store";
import { isActive, type Job, type ToolStatus } from "./types";
import { applyUpdate, checkForUpdate, useUpdate } from "./updater";
import { Downloads } from "./views/Downloads";
import { Library } from "./views/Library";
import { Settings } from "./views/Settings";

type View = "downloads" | "library" | "settings";

export function App() {
  const jobs = useJobs();
  const settings = useSettings();
  const toasts = useToasts();
  const update = useUpdate();
  const player = usePlayerState();
  const playingJob = player.id ? jobs.find((j) => j.id === player.id) : undefined;
  const [view, setView] = useState<View>("downloads");
  const [editing, setEditing] = useState<Job | null>(null);
  const [dropping, setDropping] = useState(false);
  const [missingTools, setMissingTools] = useState<string[]>([]);
  const [ytdlpStale, setYtdlpStale] = useState<string | null>(null);
  const [updatingYtdlp, setUpdatingYtdlp] = useState(false);

  const activeCount = jobs.filter((j) => isActive(j) || j.status === "queued").length;
  const libraryCount = jobs.filter((j) => j.status === "done" && j.output).length;

  // Dropped files: convert them from Downloads, or just add them from Library.
  useEffect(() => {
    const off = onFileDrop(async (paths) => {
      const convert = view !== "library";
      const added = await call<number>("add_files", { paths, convert });
      if (!added) toast("No supported audio files in that drop", "error");
      else if (!convert) toast(`Added ${added} file${added === 1 ? "" : "s"}`);
    }, setDropping);
    return () => void off.then((f) => f());
  }, [view]);

  useEffect(() => {
    const check = async () => {
      const tools = await call<ToolStatus[]>("tools_status");
      setMissingTools(tools.filter((x) => !x.found).map((x) => x.name));
      // An old yt-dlp is the usual cause of failed downloads, so say so up front.
      const ytdlp = tools.find((x) => x.name === "yt-dlp");
      const latest = ytdlp?.version ? await call<string | null>("ytdlp_latest") : null;
      setYtdlpStale(latest && ytdlp?.version && ytdlp.version < latest ? latest : null);
    };
    check();
    const off = on("tools-changed", check);
    return () => void off.then((f) => f());
  }, []);

  // Quietly look for a new version at launch and every six hours after.
  const autoUpdate = settings?.autoUpdate;
  useEffect(() => {
    if (!autoUpdate) return;
    checkForUpdate(true);
    const timer = setInterval(() => checkForUpdate(true), 6 * 60 * 60 * 1000);
    return () => clearInterval(timer);
  }, [autoUpdate]);

  const nav: { id: View; label: string; icon: typeof ArrowDownToLine; badge?: number }[] = [
    { id: "downloads", label: "Downloads", icon: ArrowDownToLine, badge: activeCount || undefined },
    { id: "library", label: "Library", icon: LibraryIcon, badge: libraryCount || undefined },
    { id: "settings", label: "Settings", icon: SettingsIcon },
  ];

  return (
    <div className={`app${isMac ? " mac" : ""}`}>
      <aside className="sidebar" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          <Logo size={30} />
          <Wordmark />
        </div>
        <nav>
          {nav.map(({ id, label, icon: Icon, badge }) => (
            <button key={id} className={view === id ? "on" : ""} onClick={() => setView(id)}>
              <Icon size={17} />
              <span>{label}</span>
              {badge ? <em className={id === "downloads" ? "live" : ""}>{badge}</em> : null}
            </button>
          ))}
        </nav>
        <div className="spacer" data-tauri-drag-region />

        {missingTools.length > 0 && (
          <button className="side-note warn" onClick={() => setView("settings")}>
            <TriangleAlert size={15} />
            <span>
              <strong>Setup needed</strong>
              {missingTools.join(", ")} missing. Install in Settings.
            </span>
          </button>
        )}
        {ytdlpStale && missingTools.length === 0 && (
          <button
            className="side-note warn"
            disabled={updatingYtdlp}
            onClick={async () => {
              setUpdatingYtdlp(true);
              try {
                await call("install_tool", { name: "yt-dlp" });
                toast("yt-dlp updated");
              } catch (e) {
                toast(String(e), "error");
              } finally {
                setUpdatingYtdlp(false);
              }
            }}
          >
            <TriangleAlert size={15} />
            <span>
              <strong>{updatingYtdlp ? "Updating yt-dlp…" : "yt-dlp is out of date"}</strong>
              {updatingYtdlp ? "This takes a few seconds." : `Some downloads will fail. Click to update to ${ytdlpStale}.`}
            </span>
          </button>
        )}
        {update.phase === "ready" && (
          <button className="side-note update" onClick={applyUpdate}>
            <RefreshCw size={15} />
            <span>
              <strong>Update ready</strong>
              Restart to get {update.version}
            </span>
          </button>
        )}
        {playingJob && (
          <div className="now-playing">
            <button className="now-art" title="Show the player" onClick={showPlayer}>
              {playingJob.thumbnail ? <img src={playingJob.thumbnail} alt="" /> : <Music size={16} />}
            </button>
            <button className="now-text" title="Show the player" onClick={showPlayer}>
              <strong>{playingJob.title}</strong>
              <span>{playingJob.artist ?? "Now playing"}</span>
            </button>
            <button className="icon-btn" title={player.playing ? "Pause" : "Play"} onClick={togglePlayback}>
              {player.playing ? <Pause size={15} fill="currentColor" /> : <Play size={15} fill="currentColor" />}
            </button>
          </div>
        )}
        <p className="tagline">Download · Convert · Tag</p>
      </aside>

      <main>
        <div className="titlebar" data-tauri-drag-region />
        <div className="scroll">
          {view === "downloads" && <Downloads onEdit={setEditing} />}
          {view === "library" && <Library onEdit={setEditing} />}
          {view === "settings" && <Settings />}
        </div>
      </main>

      {dropping && (
        <div className="drop-overlay">
          <div>
            <Upload size={28} />
            <p>{view === "library" ? "Drop to add to your Library" : `Drop to convert to ${settings?.format.toUpperCase() ?? "MP3"}`}</p>
          </div>
        </div>
      )}

      {editing && editing.output && <TagEditor job={editing} onClose={() => setEditing(null)} />}

      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.tone}`}>
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}
