import { AppWindow, ArrowDown, ChevronDown, ClipboardPaste, LibraryBig, ListMusic, Pause, Play, Power, Search, Settings as SettingsIcon } from "lucide-react";
import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { call, devLog, readClipboard } from "./api";
import { ThemeButton } from "./components/Chrome";
import { JobRow } from "./components/JobRow";
import { Logo, Wordmark } from "./components/Logo";
import { extractLinks } from "./format";
import { showPlayer } from "./playerControl";
import { addLinks, saveSettings, toast, useJobs, useSettings, useToasts } from "./store";
import { FORMATS, isActive } from "./types";

const savedShowFinished = () => {
  try {
    return localStorage.getItem("panel-show-finished") === "1";
  } catch {
    return false;
  }
};

/** The compact window that drops down from the menu bar / tray icon. */
export function Panel() {
  const jobs = useJobs();
  const settings = useSettings();
  const toasts = useToasts();
  const [text, setText] = useState("");

  const list = useMemo(() => jobs.filter((j) => !j.cleared && j.kind !== "import").slice().reverse(), [jobs]);
  // In progress (and anything needing attention) up top; finished tracks tucked away below.
  const current = list.filter((j) => j.status !== "done");
  const finished = list.filter((j) => j.status === "done").slice(0, 60);
  const [showFinished, setShowFinished] = useState(savedShowFinished);
  const running = list.some((j) => isActive(j) || j.status === "queued");
  const paused = list.some((j) => j.status === "paused");

  // Size the window to what is showing, up to its original height; past that, the list scrolls.
  const panelRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const panel = panelRef.current;
    const listEl = listRef.current;
    const content = contentRef.current;
    if (!panel || !listEl || !content) return;
    // Timers rather than animation frames: frames are paused while the panel is
    // hidden, and it is hidden most of the time.
    let timer = 0;
    let last = 0;
    const fit = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        const style = getComputedStyle(listEl);
        const padding = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);
        const chrome = panel.offsetHeight - listEl.clientHeight;
        // Exact (fractional) height, plus a pixel spare so rounding never adds a scrollbar.
        const needed = Math.ceil(chrome + padding + content.getBoundingClientRect().height + 1);
        if (needed === last) return;
        last = needed;
        devLog(`panel wants ${needed}px`);
        call("fit_panel", { height: needed }).catch(() => {});
      }, 30);
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(content);
    // Measure again whenever the panel is shown.
    const shown = () => {
      last = 0;
      fit();
    };
    window.addEventListener("focus", shown);
    document.addEventListener("visibilitychange", shown);
    return () => {
      observer.disconnect();
      window.clearTimeout(timer);
      window.removeEventListener("focus", shown);
      document.removeEventListener("visibilitychange", shown);
    };
  }, [showFinished]);

  const toggleFinished = () => {
    const next = !showFinished;
    setShowFinished(next);
    try {
      localStorage.setItem("panel-show-finished", next ? "1" : "0");
    } catch {
      /* the choice just won't be remembered */
    }
  };
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
    <div className="panel" ref={panelRef}>
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

      <div className="panel-list" ref={listRef}>
        <div className="panel-list-inner" ref={contentRef}>
          {current.map((job) => (
            <JobRow key={job.id} job={job} compact />
          ))}

          {finished.length > 0 && (
            <button
              className={`finished-toggle${showFinished ? " open" : ""}`}
              aria-expanded={showFinished}
              title={showFinished ? "Hide finished downloads" : "Show finished downloads"}
              onClick={toggleFinished}
            >
              <ChevronDown size={13} />
              <span>
                Downloaded <em>{finished.length}</em>
              </span>
              <ChevronDown size={13} />
            </button>
          )}
          {showFinished && finished.map((job) => <JobRow key={job.id} job={job} compact />)}
        </div>
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
