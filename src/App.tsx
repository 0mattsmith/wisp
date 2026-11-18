import { ArrowDownToLine, LibraryBig, Music, Pause, Play, RefreshCw, Settings as SettingsIcon, TriangleAlert, Upload } from "lucide-react";
import { useEffect, useState } from "react";
import { call, isMac, on, onFileDrop } from "./api";
import { ThemeButton } from "./components/Chrome";
import { Logo, Wordmark } from "./components/Logo";
import { TagEditor } from "./components/TagEditor";
import { showPlayer, togglePlayback, usePlayerState } from "./playerControl";
import { toast, useJobs, useSettings, useToasts } from "./store";
import { bytes, speed } from "./format";
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

  // The menu bar panel and tray menu can open a particular page.
  useEffect(() => {
    const off = on<string>("navigate", (to) => {
      if (to === "downloads" || to === "library" || to === "settings") setView(to);
    });
    return () => void off.then((f) => f());
  }, []);

  const format = settings?.format;
  // Dropped files: convert them from Downloads, or just add them from Library.
  useEffect(() => {
    const off = onFileDrop(async (paths) => {
      // With "None" chosen there is nothing to convert to: dropped files just join the Library.
      const convert = view !== "library" && format !== "none";
      const added = await call<number>("add_files", { paths, convert });
      if (!added) toast("No supported audio files in that drop", "error");
      else if (!convert) toast(`Added ${added} file${added === 1 ? "" : "s"}`);
    }, setDropping);
    return () => void off.then((f) => f());
  }, [view, format]);

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

  const item = (id: View, label: string, Icon: typeof ArrowDownToLine, badge?: number, live = false) => (
    <button key={id} className={`side-item${view === id ? " on" : ""}`} onClick={() => setView(id)}>
      <Icon size={16} />
      <span>{label}</span>
      {badge ? <em className={live ? "live" : ""}>{badge}</em> : null}
    </button>
  );

  // The Finder-style line at the foot of the window.
  const downloading = jobs.filter((j) => j.status === "downloading");
  const totalSpeed = downloading.reduce((sum, j) => sum + (j.speed ?? 0), 0);
  const librarySize = jobs.filter((j) => j.status === "done" && j.output).reduce((sum, j) => sum + (j.size ?? 0), 0);
  const status =
    view === "library"
      ? `${libraryCount} track${libraryCount === 1 ? "" : "s"}${librarySize ? ` · ${bytes(librarySize)}` : ""}`
      : view === "downloads"
        ? activeCount
          ? `${activeCount} in progress${totalSpeed ? ` · ${speed(totalSpeed)}` : ""}`
          : "Nothing downloading"
        : "Wisp";

  return (
    <div className={`app${isMac ? " mac" : ""}`}>
      <aside className="sidebar" data-tauri-drag-region>
        <div className="side-top" data-tauri-drag-region />
        <div className="brand" data-tauri-drag-region>
          <Logo size={24} />
          <Wordmark />
        </div>
        <nav>
          <div className="side-section">Music</div>
          {item("downloads", "Downloads", ArrowDownToLine, activeCount || undefined, true)}
          {item("library", "Library", LibraryBig, libraryCount || undefined)}
          <div className="side-section">Wisp</div>
          {item("settings", "Settings", SettingsIcon)}
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
        <div className="side-foot">
          <span className="tagline">Download · Convert · Tag</span>
          <ThemeButton className="icon-btn" />
        </div>
      </aside>

      <main className="content">
        <div className="scroll">
          {view === "downloads" && <Downloads onEdit={setEditing} />}
          {view === "library" && <Library onEdit={setEditing} />}
          {view === "settings" && <Settings />}
        </div>
        <footer className="statusbar">{status}</footer>
      </main>

      {dropping && (
        <div className="drop-overlay">
          <div>
            <Upload size={28} />
            <p>
              {view === "library" || settings?.format === "none"
                ? "Drop to add to your Library"
                : `Drop to convert to ${settings?.format.toUpperCase() ?? "MP3"}`}
            </p>
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
