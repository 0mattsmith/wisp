import {
  CircleAlert,
  Clock,
  FolderOpen,
  Gauge,
  HardDrive,
  ListMusic,
  Music,
  Pause,
  Pencil,
  Play,
  RotateCcw,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  X,
} from "lucide-react";
import { call, reveal } from "../api";
import { bytes, clock, speed } from "../format";
import { isActive, type Job } from "../types";

const SITE: Record<string, string> = { youtube: "YouTube", soundcloud: "SoundCloud", web: "Web", file: "File" };

function stateLabel(job: Job): string {
  switch (job.status) {
    case "queued":
      return "Queued";
    case "fetching":
      return job.kind === "playlist" ? "Reading…" : job.note ? "Searching" : "Starting…";
    case "downloading":
      return `${Math.round((job.progress / 0.9) * 100)}%`;
    case "converting":
      return job.kind === "convert" ? `${Math.round(job.progress * 100)}%` : "Converting";
    case "tagging":
      return "Tagging";
    case "done":
      return "Done";
    case "failed":
      return "Failed";
    case "canceled":
      return "Canceled";
    case "paused":
      return "Paused";
  }
}

export function JobRow({
  job,
  compact = false,
  onEdit,
  onPlay,
  nowPlaying = false,
}: {
  job: Job;
  compact?: boolean;
  onEdit?: (job: Job) => void;
  /** Start (or pause) this track in the pop-out player. */
  onPlay?: (job: Job) => void;
  /** This is the track the player currently has loaded and playing. */
  nowPlaying?: boolean;
}) {
  const active = isActive(job);
  const waiting = job.status === "queued" || job.status === "paused";
  const showBar = active || waiting;
  // Stages without a measurable position get a moving bar instead of a stuck one.
  const indeterminate =
    job.status === "fetching" || job.status === "tagging" || (job.status === "converting" && job.kind !== "convert");
  const sub = [job.artist, job.album].filter(Boolean).join(" · ");

  return (
    <div className={`row is-${job.status}${compact ? " compact" : ""}`}>
      <div className="thumb">
        {job.thumbnail ? (
          <img src={job.thumbnail} alt="" loading="lazy" referrerPolicy="no-referrer" />
        ) : job.kind === "playlist" ? (
          <ListMusic size={compact ? 16 : 20} />
        ) : (
          <Music size={compact ? 16 : 20} />
        )}
      </div>

      <div className="row-main">
        <div className="row-top">
          <div className="row-title" title={job.title}>
            {job.title}
          </div>
          <div className="row-state">{stateLabel(job)}</div>
          <div className="row-actions">
            {(active || job.status === "queued") && job.kind !== "playlist" && (
              <button className="icon-btn" title="Pause" onClick={() => call("pause_job", { id: job.id })}>
                <Pause size={15} />
              </button>
            )}
            {job.status === "paused" && (
              <button className="icon-btn" title="Resume" onClick={() => call("resume_job", { id: job.id })}>
                <Play size={15} />
              </button>
            )}
            {(job.status === "failed" || job.status === "canceled") && (
              <button className="icon-btn" title="Try again" onClick={() => call("resume_job", { id: job.id })}>
                <RotateCcw size={15} />
              </button>
            )}
            {job.status === "done" && job.output && onPlay && (
              <button className={`icon-btn${nowPlaying ? " lit" : ""}`} title={nowPlaying ? "Pause" : "Play"} onClick={() => onPlay(job)}>
                {nowPlaying ? <Pause size={15} /> : <Play size={15} />}
              </button>
            )}
            {job.status === "done" && job.output && onEdit && (
              <button className="icon-btn" title="Edit tags" onClick={() => onEdit(job)}>
                <Pencil size={15} />
              </button>
            )}
            {job.status === "done" && job.output && (
              <button className="icon-btn" title="Show in folder" onClick={() => reveal(job.output!)}>
                <FolderOpen size={15} />
              </button>
            )}
            {active || waiting ? (
              <button className="icon-btn" title="Cancel" onClick={() => call("cancel_job", { id: job.id })}>
                <X size={15} />
              </button>
            ) : (
              <button
                className="icon-btn"
                title="Remove from list (the file is kept)"
                onClick={() => call("remove_job", { id: job.id })}
              >
                <X size={15} />
              </button>
            )}
          </div>
        </div>

        {!compact && sub && <div className="row-sub">{sub}</div>}

        {showBar && (
          <div className={`bar${indeterminate ? " indeterminate" : ""}`}>
            <span style={{ width: `${Math.max(2, job.progress * 100)}%` }} />
          </div>
        )}

        {job.note && (active || waiting) && (
          <div className={`row-note ${job.workaround ?? "drm"}`}>
            <ShieldAlert size={12} /> {job.note}
          </div>
        )}

        {job.error && job.status !== "done" ? (
          <div className="row-error">
            <CircleAlert size={13} /> {job.error}
          </div>
        ) : (
          <div className="chips">
            <span className="chip strong">{job.format.toUpperCase()}</span>
            {!compact && <span className="chip">{SITE[job.site] ?? job.site}</span>}
            {job.status === "downloading" && job.total ? (
              <span className="chip">
                <HardDrive size={11} /> {bytes(job.total)}
              </span>
            ) : null}
            {job.status === "done" && job.size ? (
              <span className="chip">
                <HardDrive size={11} /> {bytes(job.size)}
              </span>
            ) : null}
            {job.status === "downloading" && job.eta != null && (
              <span className="chip">
                <Clock size={11} /> {clock(job.eta)}
              </span>
            )}
            {job.status !== "downloading" && job.duration ? (
              <span className="chip">
                <Clock size={11} /> {clock(job.duration)}
              </span>
            ) : null}
            {job.status === "downloading" && job.speed ? (
              <span className="chip speed">
                <Gauge size={11} /> {speed(job.speed)}
              </span>
            ) : null}
            {job.status === "done" && job.workaround && (
              <span
                className={`chip workaround ${job.workaround}`}
                title={
                  job.workaround === "drm"
                    ? "This track is DRM-protected on SoundCloud, so the same recording was downloaded from an unprotected source."
                    : "SoundCloud only offers a preview of this track, so the full recording was downloaded from another source."
                }
              >
                <ShieldCheck size={11} /> {job.workaround === "drm" ? "Unprotected copy" : "Full track"}
              </span>
            )}
            {job.status === "done" && job.matched && (
              <span className="chip matched" title={`Tags matched with ${Math.round((job.matchScore ?? 0) * 100)}% confidence`}>
                <Sparkles size={11} /> {job.matched}
              </span>
            )}
            {job.status === "done" && job.error && (
              <span className="chip warn" title={job.error}>
                <CircleAlert size={11} /> {job.error.startsWith("Preview") ? "Preview only" : "Tags not written"}
              </span>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
