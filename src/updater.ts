import { useSyncExternalStore } from "react";
import { inTauri } from "./api";

export interface UpdateState {
  phase: "idle" | "checking" | "none" | "downloading" | "ready" | "error";
  version?: string;
  progress?: number;
  error?: string;
}

let state: UpdateState = { phase: "idle" };
const listeners = new Set<() => void>();
const set = (next: UpdateState) => {
  state = next;
  listeners.forEach((l) => l());
};

// The downloaded update, waiting to be installed.
let pending: { install: () => Promise<void> } | null = null;

export const useUpdate = () =>
  useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => state,
  );

/**
 * Look for a new version and, if there is one, download it in the background.
 * Nothing is installed until `applyUpdate` runs, so active downloads are never interrupted.
 */
export async function checkForUpdate(silent: boolean) {
  if (state.phase === "checking" || state.phase === "downloading" || state.phase === "ready") return;
  if (!inTauri) return set({ phase: "none" });
  set({ phase: "checking" });
  try {
    const { check } = await import("@tauri-apps/plugin-updater");
    const update = await check();
    if (!update) return set({ phase: silent ? "idle" : "none" });
    let total = 0;
    let got = 0;
    set({ phase: "downloading", version: update.version, progress: 0 });
    await update.download((e) => {
      if (e.event === "Started") total = e.data.contentLength ?? 0;
      else if (e.event === "Progress") {
        got += e.data.chunkLength;
        set({ phase: "downloading", version: update.version, progress: total ? got / total : 0 });
      }
    });
    pending = update;
    set({ phase: "ready", version: update.version });
  } catch (e) {
    set(silent ? { phase: "idle" } : { phase: "error", error: String(e) });
  }
}

/** Swap in the downloaded version and restart. */
export async function applyUpdate() {
  if (!pending) return;
  try {
    await pending.install();
    const { relaunch } = await import("@tauri-apps/plugin-process");
    await relaunch();
  } catch (e) {
    set({ phase: "error", error: String(e) });
  }
}

export async function appVersion(): Promise<string> {
  if (!inTauri) return "0.1.0";
  const { getVersion } = await import("@tauri-apps/api/app");
  return getVersion();
}
