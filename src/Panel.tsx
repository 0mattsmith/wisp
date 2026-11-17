import { ArrowDownToLine, ClipboardPaste, ExternalLink, Pause, Play, Power } from "lucide-react";
import { useMemo, useState } from "react";
import { call, readClipboard } from "./api";
import { JobRow } from "./components/JobRow";
import { Logo, Wordmark } from "./components/Logo";
import { extractLinks } from "./format";
import { addLinks, toast, useJobs, useSettings, useToasts } from "./store";
import { FORMATS, isActive } from "./types";
import { saveSettings } from "./store";

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

  const submit = async (raw: string) => {
    const links = extractLinks(raw);
    if (!links.length) return toast("That is not a link", "error");
    if ((await addLinks(links, settings?.format, settings?.quality)) > 0) setText("");
  };

  return (
    <div className="panel">
      <header className="panel-head">
        <Logo size={24} />
        <Wordmark />
        <div className="spacer" />
        <button className="soft-btn" onClick={() => call("open_main")}>
          <ExternalLink size={13} /> Open app
        </button>
        {running ? (
          <button className="soft-btn" onClick={() => call("pause_all")}>
            <Pause size={13} /> Pause all
          </button>
        ) : paused ? (
          <button className="soft-btn" onClick={() => call("resume_all")}>
            <Play size={13} /> Resume
          </button>
        ) : null}
      </header>

      <div className="panel-add">
        <div className="url-box small">
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
          <button className="primary-btn square" title="Download" onClick={() => submit(text)}>
            <ArrowDownToLine size={16} />
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
            <p>No downloads yet.</p>
            <span>Paste a link to start one.</span>
          </div>
        ) : (
          shown.map((job) => <JobRow key={job.id} job={job} compact />)
        )}
      </div>

      <footer className="panel-foot">
        {list.length > shown.length ? (
          <button className="link-btn" onClick={() => call("open_main")}>
            Show {list.length - shown.length} more
          </button>
        ) : (
          <span />
        )}
        <button className="link-btn quiet" onClick={() => call("quit_app")} title="Quit Dubplate">
          <Power size={12} /> Quit
        </button>
      </footer>

      {toasts.length > 0 && <div className={`panel-toast ${toasts[toasts.length - 1].tone}`}>{toasts[toasts.length - 1].text}</div>}
    </div>
  );
}
