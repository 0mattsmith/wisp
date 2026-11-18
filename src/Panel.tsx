import { AppWindow, ArrowDown, ClipboardPaste, LibraryBig, ListMusic, Pause, Play, Power, Search, Settings as SettingsIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { call, readClipboard } from "./api";
import { ThemeButton } from "./components/Chrome";
import { JobRow } from "./components/JobRow";
import { Logo, Wordmark } from "./components/Logo";
import { extractLinks } from "./format";
import { showPlayer } from "./playerControl";
import { addLinks, saveSettings, toast, useJobs, useSettings, useToasts } from "./store";
import { FORMATS, isActive } from "./types";

/** The compact window that drops down from the menu bar / tray icon. */
export function Panel() {
  const jobs = useJobs();
  const settings = useSettings();
  const toasts = useToasts();
  const [text, setText] = useState("");

  const list = useMemo(() => jobs.filter((j) => !j.cleared && j.kind !== "import").slice().reverse(), [jobs]);
  const shown = list.slice(0, 6);
  const running = list.some((j) => isActive(j) || j.status === "queued");
  const paused = list.some((j) => j.status === "paused");
  const looksLikeSearch = text.trim() !== "" && extractLinks(text).length === 0;

  const submit = async (raw: string) => {
    const links = extractLinks(raw);
    if (!links.length) {
      // Searching needs room for results: hand it to the main window.
      if (raw.trim()) {
        await call("open_main", { view: "downloads" });
        return toast("Search from the Downloads page in the main window");
      }
      return toast("That is not a link", "error");
    }
    if ((await addLinks(links, settings?.format, settings?.quality)) > 0) setText("");
  };

  return (
    <div className="panel">
      <header className="panel-head">
        <Logo size={22} />
        <Wordmark />
        <div className="spacer" />
        <ThemeButton className="round-btn" />
        <button className="round-btn" title="Settings" aria-label="Settings" onClick={() => call("open_main", { view: "settings" })}>
          <SettingsIcon size={15} />
        </button>
        <button className="round-btn" title="Open Wisp" aria-label="Open Wisp" onClick={() => call("open_main")}>
          <AppWindow size={15} />
        </button>
      </header>

      <div className="panel-add">
        <div className="composer pill">
          <input
            value={text}
            placeholder="Paste a YouTube or SoundCloud link"
            spellCheck={false}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && submit(text)}
          />
          <button
            className="ghost-btn"
            title="Download the link on the clipboard"
            onClick={async () => {
              const clip = await readClipboard();
              if (clip) submit(clip);
            }}
          >
            <ClipboardPaste size={15} />
          </button>
          <button className="send-btn" title={looksLikeSearch ? "Search" : "Download"} onClick={() => submit(text)}>
            {looksLikeSearch ? <Search size={15} strokeWidth={2.4} /> : <ArrowDown size={16} strokeWidth={2.6} />}
          </button>
        </div>
        {settings && (
          <div className="segmented tiny">
            {FORMATS.map((f) => (
              <button key={f} className={settings.format === f ? "on" : ""} onClick={() => saveSettings({ format: f })}>
                {f.toUpperCase()}
              </button>
            ))}
          </div>
        )}
      </div>

      <div className="panel-list">
        {shown.length === 0 ? (
          <div className="empty small">
            <p>No downloads yet</p>
            <span>Paste a link to start one.</span>
          </div>
        ) : (
          shown.map((job) => <JobRow key={job.id} job={job} compact />)
        )}
        {list.length > shown.length && (
          <button className="link-btn more" onClick={() => call("open_main", { view: "downloads" })}>
            Show {list.length - shown.length} more
          </button>
        )}
      </div>

      <footer className="panel-dock">
        <div className="dock">
          {running ? (
            <button className="round-btn" title="Pause all" aria-label="Pause all" onClick={() => call("pause_all")}>
              <Pause size={15} />
            </button>
          ) : (
            <button className="round-btn" title="Resume all" aria-label="Resume all" disabled={!paused} onClick={() => call("resume_all")}>
              <Play size={15} />
            </button>
          )}
          <button className="round-btn" title="Show the player" aria-label="Show the player" onClick={showPlayer}>
            <ListMusic size={15} />
          </button>
          <button className="round-btn" title="Library" aria-label="Library" onClick={() => call("open_main", { view: "library" })}>
            <LibraryBig size={15} />
          </button>
          <span className="dock-grip" aria-hidden="true" />
          <button className="round-btn" title="Quit Wisp" aria-label="Quit Wisp" onClick={() => call("quit_app")}>
            <Power size={15} />
          </button>
        </div>
      </footer>

      {toasts.length > 0 && <div className={`panel-toast ${toasts[toasts.length - 1].tone}`}>{toasts[toasts.length - 1].text}</div>}
    </div>
  );
}
