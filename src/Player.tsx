import { ExternalLink, Music, Pause, Play, SkipBack, SkipForward, Volume2, VolumeX, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { call, devLog, fileSrc, inTauri, on, send, startDrag } from "./api";
import { clock } from "./format";
import type { PlayerCommand, PlayerState } from "./playerControl";
import { useJobs } from "./store";
import type { FileTags } from "./types";

const savedVolume = () => {
  const v = Number(localStorage.getItem("player-volume"));
  return Number.isFinite(v) && v > 0 && v <= 1 ? v : 0.9;
};

/** The pop-out mini player: artwork up top, three chunky controls beneath. */
export function Player() {
  const jobs = useJobs();
  const audio = useRef<HTMLAudioElement>(null);
  const [queue, setQueue] = useState<string[]>([]);
  const [index, setIndex] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolume] = useState(savedVolume);
  const [muted, setMuted] = useState(false);
  const [cover, setCover] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  const currentId = queue[index] ?? null;
  const track = useMemo(() => jobs.find((j) => j.id === currentId) ?? null, [jobs, currentId]);

  // Refs let the long-lived event listeners below see the latest values.
  const live = useRef({ queue, index, currentId, playing });
  live.current = { queue, index, currentId, playing };

  const report = useCallback((id: string | null, isPlaying: boolean) => {
    send("player-state", { id, playing: isPlaying } satisfies PlayerState);
  }, []);

  const step = useCallback((by: number) => {
    const { queue, index } = live.current;
    const el = audio.current;
    // "Previous" restarts the track first, as most players do.
    if (by < 0 && el && el.currentTime > 3) {
      el.currentTime = 0;
      return;
    }
    const next = index + by;
    if (next >= 0 && next < queue.length) setIndex(next);
    else if (el && by > 0) el.pause();
  }, []);

  const toggle = useCallback(() => {
    const el = audio.current;
    if (!el || !live.current.currentId) return;
    if (el.paused) el.play().catch(() => {});
    else el.pause();
  }, []);

  const close = useCallback(() => {
    audio.current?.pause();
    setQueue([]);
    setIndex(0);
    setCover(null);
    report(null, false);
    call("player_hide");
  }, [report]);

  // Commands from the main window, the menu bar panel and the tray.
  const lastSeq = useRef(0);
  useEffect(() => {
    const handle = (cmd: PlayerCommand) => {
      devLog(`command: ${cmd.type}`);
      if (cmd.type === "play") {
        // The same request can arrive twice (as an event and as the one held for us).
        if (cmd.seq <= lastSeq.current) return;
        lastSeq.current = cmd.seq;
        call("player_ack", { seq: cmd.seq });
        setProblem(null);
        if (cmd.muted) setMuted(true);
        setQueue(cmd.ids);
        setIndex(Math.min(Math.max(cmd.index, 0), Math.max(cmd.ids.length - 1, 0)));
      } else if (cmd.type === "toggle") toggle();
      else if (cmd.type === "volume") {
        setMuted(cmd.value <= 0);
        if (cmd.value > 0) setVolume(Math.min(cmd.value, 1));
      } else if (cmd.type === "query") report(live.current.currentId, live.current.playing);
    };
    const off = on<PlayerCommand>("player-command", handle);
    // Once listening, pick up any request sent before this window was ready,
    // and tell the other windows what the player is doing.
    off.then(async () => {
      const missed = await call<PlayerCommand | null>("player_pending");
      if (missed) handle(missed);
      report(live.current.currentId, live.current.playing);
    });
    return () => void off.then((f) => f());
  }, [toggle, report]);

  // Load and start whichever track is now current.
  useEffect(() => {
    const el = audio.current;
    if (!el || !currentId) return;
    let stale = false;
    setTime(0);
    setDuration(0);
    setCover(null);
    (async () => {
      try {
        const path = await call<string>("player_source", { id: currentId });
        if (stale) return;
        el.src = fileSrc(path);
        devLog(`loading ${el.src}`);
        await el.play();
        // The list thumbnail is tiny; fetch the full-size cover for the widget.
        const tags = await call<FileTags>("read_tags", { path });
        if (!stale) setCover(tags.cover);
      } catch (e) {
        if (stale) return;
        const blocked = e instanceof DOMException && e.name === "NotAllowedError";
        setProblem(blocked ? "Press play to start." : typeof e === "string" ? e : "This file could not be played.");
        devLog(`load failed: ${String(e)}`);
      }
    })();
    return () => {
      stale = true;
    };
  }, [currentId]);

  useEffect(() => {
    const el = audio.current;
    if (!el) return;
    el.volume = volume;
    el.muted = muted;
    localStorage.setItem("player-volume", String(volume));
  }, [volume, muted]);

  // Show the track on the system's Now Playing controls and answer the media keys.
  useEffect(() => {
    if (!("mediaSession" in navigator) || !track) return;
    navigator.mediaSession.metadata = new MediaMetadata({
      title: track.title,
      artist: track.artist ?? "",
      album: track.album ?? "",
      artwork: cover ? [{ src: cover }] : [],
    });
    navigator.mediaSession.setActionHandler("play", toggle);
    navigator.mediaSession.setActionHandler("pause", toggle);
    navigator.mediaSession.setActionHandler("previoustrack", () => step(-1));
    navigator.mediaSession.setActionHandler("nexttrack", () => step(1));
  }, [track, cover, toggle, step]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = audio.current;
      if (e.key === " ") {
        e.preventDefault();
        toggle();
      } else if (e.key === "ArrowRight" && el) el.currentTime = Math.min(el.currentTime + 5, el.duration || 0);
      else if (e.key === "ArrowLeft" && el) el.currentTime = Math.max(el.currentTime - 5, 0);
      else if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggle, close]);

  const seek = (e: React.PointerEvent<HTMLDivElement>) => {
    const el = audio.current;
    if (!el || !duration) return;
    const box = e.currentTarget.getBoundingClientRect();
    el.currentTime = Math.min(Math.max((e.clientX - box.left) / box.width, 0), 1) * duration;
    setTime(el.currentTime);
  };

  // Outside the app (design preview in a browser) show the first finished track.
  const shown = track ?? (inTauri ? null : (jobs.find((j) => j.status === "done") ?? null));
  const art = cover ?? shown?.thumbnail ?? null;
  const shownDuration = duration || shown?.duration || 0;
  const hasPrev = index > 0 || time > 3;
  const hasNext = index < queue.length - 1;

  return (
    <div className="player">
      <audio
        ref={audio}
        onPlay={() => {
          devLog(`playing ${live.current.currentId}`);
          setPlaying(true);
          setProblem(null);
          report(live.current.currentId, true);
        }}
        onPause={() => {
          setPlaying(false);
          report(live.current.currentId, false);
        }}
        onTimeUpdate={(e) => {
          const t = e.currentTarget.currentTime;
          // Development builds report progress so playback can be checked from a terminal.
          if (Math.floor(t) !== Math.floor(time)) devLog(`position ${t.toFixed(1)}s of ${e.currentTarget.duration.toFixed(1)}s`);
          setTime(t);
        }}
        onDurationChange={(e) => setDuration(Number.isFinite(e.currentTarget.duration) ? e.currentTarget.duration : 0)}
        onEnded={() => step(1)}
        onError={() => {
          if (!live.current.currentId) return;
          setProblem("This format cannot be played on this system.");
          devLog(`audio error code ${audio.current?.error?.code}: ${audio.current?.error?.message ?? ""}`);
        }}
      />

      <div
        className="player-art"
        onPointerDown={(e) => {
          // Drag the widget by its artwork, but not by the buttons sitting on it.
          if (e.button === 0 && !(e.target as HTMLElement).closest("button, input, .player-seek")) startDrag();
        }}
      >
        {art ? <img src={art} alt="" draggable={false} /> : <Music size={54} />}

        <div className="player-top">
          <button className="player-chip" title="Close player" onClick={close}>
            <X size={14} />
          </button>
          <div className="spacer" />
          <div className="player-volume">
            <button className="player-chip" title={muted ? "Unmute" : "Mute"} onClick={() => setMuted(!muted)}>
              {muted ? <VolumeX size={14} /> : <Volume2 size={14} />}
            </button>
            <input
              type="range"
              min={0}
              max={1}
              step={0.02}
              value={muted ? 0 : volume}
              aria-label="Volume"
              onChange={(e) => {
                setMuted(false);
                setVolume(Number(e.target.value));
              }}
            />
          </div>
          <button className="player-chip" title="Open Dubplate" onClick={() => call("open_main")}>
            <ExternalLink size={14} />
          </button>
        </div>

        <div className="player-info">
          <strong>{shown?.title ?? "Nothing playing"}</strong>
          <span>{problem ?? shown?.artist ?? (shown ? "" : "Pick a track in your Library")}</span>
          <div className="player-seek" onPointerDown={seek} onPointerMove={(e) => e.buttons === 1 && seek(e)}>
            <div className="player-seek-track">
              <span style={{ width: `${shownDuration ? Math.min((time / shownDuration) * 100, 100) : 0}%` }} />
            </div>
          </div>
          <div className="player-times">
            <span>{clock(time) || "0:00"}</span>
            <span>{clock(shownDuration) || "0:00"}</span>
          </div>
        </div>
      </div>

      <div className="player-controls">
        <button className="player-btn main" title={playing ? "Pause" : "Play"} onClick={toggle} disabled={!shown}>
          {playing ? <Pause size={22} fill="currentColor" strokeWidth={0} /> : <Play size={22} fill="currentColor" strokeWidth={0} />}
        </button>
        <button className="player-btn" title="Previous" onClick={() => step(-1)} disabled={!hasPrev}>
          <SkipBack size={18} fill="currentColor" />
        </button>
        <button className="player-btn" title="Next" onClick={() => step(1)} disabled={!hasNext}>
          <SkipForward size={18} fill="currentColor" />
        </button>
      </div>
    </div>
  );
}
