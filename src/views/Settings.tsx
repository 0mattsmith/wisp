import { Check, Download, FolderOpen, LoaderCircle, RefreshCw } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { call, isMac, loginStartEnabled, on, openExternal, pickFolder, setLoginStart } from "../api";
import { FormatPicker } from "../components/FormatPicker";
import { bytes } from "../format";
import { saveSettings, toast, useSettings } from "../store";
import type { Settings as SettingsT, ToolProgress, ToolStatus } from "../types";
import { appVersion, applyUpdate, checkForUpdate, useUpdate } from "../updater";

const NAMING = [
  { value: "{artist} - {title}", label: "Artist - Title" },
  { value: "{albumartist}/{artist} - {title}", label: "Artist folders" },
  { value: "{albumartist}/{album}/{title}", label: "Artist / Album folders" },
  { value: "{albumartist}/{album}/{track} {title}", label: "Artist / Album / numbered" },
];

function LoginStart() {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  useEffect(() => {
    loginStartEnabled()
      .then(setEnabled)
      .catch(() => setEnabled(false));
  }, []);
  return (
    <Row title="Open at login" hint={isMac ? "Starts quietly in the menu bar." : "Starts quietly in the system tray."}>
      <Toggle
        label="Open at login"
        value={enabled ?? false}
        onChange={async (on) => {
          setEnabled(on);
          try {
            await setLoginStart(on);
          } catch (e) {
            setEnabled(!on);
            toast(String(e), "error");
          }
        }}
      />
    </Row>
  );
}

function Row({ title, hint, children }: { title: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <div className="set-row">
      <div className="set-text">
        <strong>{title}</strong>
        {hint && <span>{hint}</span>}
      </div>
      <div className="set-control">{children}</div>
    </div>
  );
}

function Toggle({ value, onChange, label }: { value: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button role="switch" aria-checked={value} aria-label={label} className={`toggle${value ? " on" : ""}`} onClick={() => onChange(!value)}>
      <span />
    </button>
  );
}

/** A text field that saves when you leave it rather than on every keystroke. */
function TextSetting({ value, onSave, ...rest }: { value: string; onSave: (v: string) => void; placeholder?: string; type?: string }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <input
      className="input"
      value={draft}
      spellCheck={false}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => draft !== value && onSave(draft)}
      onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
      {...rest}
    />
  );
}

function Tools() {
  const [tools, setTools] = useState<ToolStatus[] | null>(null);
  const [latest, setLatest] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [progress, setProgress] = useState<ToolProgress | null>(null);

  const refresh = () => call<ToolStatus[]>("tools_status").then(setTools);

  useEffect(() => {
    refresh();
    call<string | null>("ytdlp_latest").then(setLatest);
    const off = [on<ToolProgress>("tool-progress", (p) => setProgress(p.done ? null : p)), on("tools-changed", refresh)];
    return () => off.forEach((o) => o.then((f) => f()));
  }, []);

  const install = async (name: string) => {
    setBusy(name);
    try {
      await call("install_tool", { name });
      toast(`${name} is ready`);
    } catch (e) {
      toast(String(e), "error");
    } finally {
      setBusy(null);
      setProgress(null);
      refresh();
    }
  };

  if (!tools) return <p className="muted">Checking…</p>;
  return (
    <>
      {tools.map((t) => {
        const outdated = t.name === "yt-dlp" && t.found && latest && t.version && t.version !== latest;
        const working = busy === t.name;
        return (
          <Row
            key={t.name}
            title={t.name}
            hint={
              t.found ? (
                <>
                  {t.version ?? "Unknown version"} · {t.managed ? "managed by Dubplate" : t.path}
                  {outdated && <em className="warn-text"> · {latest} is available</em>}
                </>
              ) : (
                <em className="warn-text">Not installed. Downloads cannot run without it.</em>
              )
            }
          >
            {working ? (
              <span className="muted inline">
                <LoaderCircle className="spin" size={14} />
                {progress?.name === t.name && progress.total
                  ? `${bytes(progress.downloaded)} of ${bytes(progress.total)}`
                  : "Working…"}
              </span>
            ) : !t.found || outdated ? (
              <button className="soft-btn accent" disabled={!t.installable || busy !== null} onClick={() => install(t.name)}>
                <Download size={14} /> {t.found ? "Update" : "Install"}
              </button>
            ) : (
              <span className="ok-text">
                <Check size={14} /> Ready
              </span>
            )}
          </Row>
        );
      })}
      <p className="set-note">
        Sites change often, and an out-of-date yt-dlp is the usual reason a download fails. A copy managed by Dubplate is kept current
        automatically; one installed by Homebrew or another package manager is updated there.
      </p>
    </>
  );
}

function Updates({ settings }: { settings: SettingsT }) {
  const update = useUpdate();
  const [version, setVersion] = useState("");
  useEffect(() => void appVersion().then(setVersion), []);

  const status =
    update.phase === "checking"
      ? "Checking…"
      : update.phase === "downloading"
        ? `Downloading ${update.version}… ${Math.round((update.progress ?? 0) * 100)}%`
        : update.phase === "ready"
          ? `Version ${update.version} is downloaded and ready.`
          : update.phase === "none"
            ? "You are on the latest version."
            : update.phase === "error"
              ? `Could not check for updates: ${update.error}`
              : `Version ${version}`;

  return (
    <>
      <Row title="Dubplate" hint={status}>
        {update.phase === "ready" ? (
          <button className="primary-btn small" onClick={applyUpdate}>
            Restart to update
          </button>
        ) : (
          <button className="soft-btn" disabled={update.phase === "checking" || update.phase === "downloading"} onClick={() => checkForUpdate(false)}>
            <RefreshCw size={14} /> Check now
          </button>
        )}
      </Row>
      <Row title="Update automatically" hint="New versions download quietly in the background. Nothing restarts until you say so.">
        <Toggle label="Update automatically" value={settings.autoUpdate} onChange={(autoUpdate) => saveSettings({ autoUpdate })} />
      </Row>
    </>
  );
}

export function Settings() {
  const s = useSettings();
  if (!s) return null;

  return (
    <div className="view settings">
      <header className="page-head">
        <div>
          <h1>Settings</h1>
        </div>
      </header>

      <section className="card">
        <h3>Downloads</h3>
        <Row title="Save to" hint={s.outputDir}>
          <button
            className="soft-btn"
            onClick={async () => {
              const dir = await pickFolder(s.outputDir);
              if (dir) saveSettings({ outputDir: dir });
            }}
          >
            <FolderOpen size={14} /> Choose
          </button>
        </Row>
        <Row title="Organise as" hint="How files are named, and whether they are sorted into folders.">
          <select
            className="select"
            value={NAMING.some((n) => n.value === s.filenameTemplate) ? s.filenameTemplate : "custom"}
            onChange={(e) => e.target.value !== "custom" && saveSettings({ filenameTemplate: e.target.value })}
          >
            {NAMING.map((n) => (
              <option key={n.value} value={n.value}>
                {n.label}
              </option>
            ))}
            <option value="custom">Custom</option>
          </select>
        </Row>
        <Row
          title="Naming pattern"
          hint="Use {artist}, {albumartist}, {title}, {album}, {year} and {track}. A / starts a new folder; folders with no matching tag are skipped."
        >
          <TextSetting value={s.filenameTemplate} onSave={(filenameTemplate) => saveSettings({ filenameTemplate })} />
        </Row>
        <Row title="Format and quality" hint="The default for new downloads and conversions.">
          <FormatPicker
            format={s.format}
            quality={s.quality}
            onFormat={(format) => saveSettings({ format })}
            onQuality={(quality) => saveSettings({ quality })}
          />
        </Row>
        <Row title="Downloads at once" hint="More is faster for playlists, but sites may slow you down above 4.">
          <select className="select" value={s.maxConcurrent} onChange={(e) => saveSettings({ maxConcurrent: Number(e.target.value) })}>
            {[1, 2, 3, 4, 5, 6, 8].map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </Row>
        <Row title="Trim non-music sections" hint="Cuts intros, outros and talking from YouTube music videos, using SponsorBlock.">
          <Toggle label="Trim non-music sections" value={s.sponsorblock} onChange={(sponsorblock) => saveSettings({ sponsorblock })} />
        </Row>
        <Row title="Use cookies from" hint="For age-restricted or members-only items you can already play in that browser.">
          <select className="select" value={s.cookiesBrowser} onChange={(e) => saveSettings({ cookiesBrowser: e.target.value })}>
            <option value="">Off</option>
            {["chrome", "firefox", "safari", "edge", "brave", "opera", "vivaldi"].map((b) => (
              <option key={b} value={b}>
                {b[0].toUpperCase() + b.slice(1)}
              </option>
            ))}
          </select>
        </Row>
      </section>

      <section className="card">
        <h3>Tags and artwork</h3>
        <Row title="Tag automatically" hint="Looks each track up and fills in artist, album, year, track number, genre and cover.">
          <Toggle label="Tag automatically" value={s.autoTag} onChange={(autoTag) => saveSettings({ autoTag })} />
        </Row>
        <Row
          title="Match confidence"
          hint={`Only apply a match that scores ${Math.round(s.matchThreshold * 100)}% or better. Anything less keeps the tags from the source page.`}
        >
          <input
            type="range"
            min={0.6}
            max={0.95}
            step={0.01}
            value={s.matchThreshold}
            onChange={(e) => saveSettings({ matchThreshold: Number(e.target.value) })}
          />
        </Row>
        <Row title="Embed cover art" hint="From the matched release, or the video thumbnail when nothing matches.">
          <Toggle label="Embed cover art" value={s.embedCover} onChange={(embedCover) => saveSettings({ embedCover })} />
        </Row>
        <Row title="Crop thumbnails square" hint="Trims widescreen video thumbnails so they sit properly in music players.">
          <Toggle label="Crop thumbnails square" value={s.squareCover} onChange={(squareCover) => saveSettings({ squareCover })} />
        </Row>
        <Row title="Sources" hint="All are free. Discogs needs a personal token.">
          <div className="source-toggles">
            {(
              [
                ["useItunes", "iTunes"],
                ["useDeezer", "Deezer"],
                ["useMusicbrainz", "MusicBrainz"],
                ["useDiscogs", "Discogs"],
              ] as const
            ).map(([key, label]) => (
              <button key={key} className={`pill-toggle${s[key] ? " on" : ""}`} aria-pressed={s[key]} onClick={() => saveSettings({ [key]: !s[key] })}>
                {label}
              </button>
            ))}
          </div>
        </Row>
        {s.useDiscogs && (
          <Row
            title="Discogs token"
            hint={
              <>
                Create one free under{" "}
                <a onClick={() => openExternal("https://www.discogs.com/settings/developers")}>Discogs → Settings → Developers</a>. Stored on this
                computer only.
              </>
            }
          >
            <TextSetting type="password" value={s.discogsToken} placeholder="Paste token" onSave={(discogsToken) => saveSettings({ discogsToken: discogsToken.trim() })} />
          </Row>
        )}
      </section>

      <section className="card">
        <h3>App</h3>
        <Row
          title={isMac ? "Keep running in the menu bar" : "Keep running in the system tray"}
          hint="Closing the window leaves downloads running. Click the icon for the quick panel."
        >
          <Toggle label="Keep running in the background" value={s.keepInTray} onChange={(keepInTray) => saveSettings({ keepInTray })} />
        </Row>
        {isMac && (
          <Row title="Hide the Dock icon" hint="Live in the menu bar only.">
            <Toggle label="Hide the Dock icon" value={s.hideDockIcon} onChange={(hideDockIcon) => saveSettings({ hideDockIcon })} />
          </Row>
        )}
        <LoginStart />
        <Row title="Notify when downloads finish">
          <Toggle label="Notify when downloads finish" value={s.notify} onChange={(notify) => saveSettings({ notify })} />
        </Row>
        <Row title="Spot copied links" hint="Offers to download a YouTube or SoundCloud link you have just copied.">
          <Toggle label="Spot copied links" value={s.clipboardWatch} onChange={(clipboardWatch) => saveSettings({ clipboardWatch })} />
        </Row>
      </section>

      <section className="card">
        <h3>Tools</h3>
        <Tools />
      </section>

      <section className="card">
        <h3>Updates</h3>
        <Updates settings={s} />
      </section>
    </div>
  );
}
