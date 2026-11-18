import { useSyncExternalStore } from "react";
import { call, on } from "./api";
import type { Job, Settings } from "./types";

type Listener = () => void;

function createStore<T>(initial: T) {
  let value = initial;
  const listeners = new Set<Listener>();
  return {
    get: () => value,
    set(next: T) {
      value = next;
      listeners.forEach((l) => l());
    },
    subscribe(l: Listener) {
      listeners.add(l);
      return () => listeners.delete(l);
    },
  };
}

const jobs = createStore<Job[]>([]);
const settings = createStore<Settings | null>(null);
const toasts = createStore<{ id: number; text: string; tone: "info" | "error" }[]>([]);
/** Why the settings could not be loaded, so a page can say so instead of staying blank. */
const loadError = createStore<string | null>(null);

let started = false;

/** Load state from the backend and keep it in sync. Safe to call twice. */
export async function startStore() {
  if (started) return;
  started = true;
  await on<Job>("job-update", (job) => {
    const list = jobs.get();
    const i = list.findIndex((j) => j.id === job.id);
    jobs.set(i === -1 ? [...list, job] : list.map((j, n) => (n === i ? job : j)));
  });
  await on<string>("job-removed", (id) => jobs.set(jobs.get().filter((j) => j.id !== id)));
  await on<Settings>("settings-changed", (s) => settings.set(s));
  // Loaded separately, so a problem with one never leaves the other empty.
  call<Job[]>("list_jobs")
    .then((list) => jobs.set(list))
    .catch((e) => toast(`Could not load your downloads: ${e}`, "error"));
  await loadSettings();
}

/** Load (or retry loading) the settings from the app. */
export async function loadSettings() {
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      settings.set(await call<Settings>("get_settings"));
      loadError.set(null);
      return;
    } catch (e) {
      loadError.set(String(e));
      await new Promise((r) => setTimeout(r, 500 * (attempt + 1)));
    }
  }
}

export const useLoadError = () => useSyncExternalStore(loadError.subscribe, loadError.get);
/** The current settings, outside React. */
export const currentSettings = () => settings.get();
export const onSettings = (cb: () => void) => settings.subscribe(cb);

export const useJobs = () => useSyncExternalStore(jobs.subscribe, jobs.get);
export const useSettings = () => useSyncExternalStore(settings.subscribe, settings.get);
export const useToasts = () => useSyncExternalStore(toasts.subscribe, toasts.get);

export async function saveSettings(patch: Partial<Settings>) {
  const current = settings.get();
  if (!current) return;
  const optimistic = { ...current, ...patch };
  settings.set(optimistic);
  try {
    settings.set(await call<Settings>("set_settings", { settings: optimistic }));
  } catch (e) {
    settings.set(current);
    toast(String(e), "error");
  }
}

let toastId = 0;
export function toast(text: string, tone: "info" | "error" = "info") {
  const id = ++toastId;
  toasts.set([...toasts.get(), { id, text, tone }]);
  setTimeout(() => toasts.set(toasts.get().filter((t) => t.id !== id)), tone === "error" ? 6000 : 3200);
}

/** Queue links for download. Returns how many were new. */
export async function addLinks(links: string[], format?: string, quality?: string): Promise<number> {
  if (links.length === 0) return 0;
  try {
    const added = await call<number>("add_urls", { urls: links, format, quality });
    if (added === 0) toast("Already in the queue");
    return added;
  } catch (e) {
    toast(String(e), "error");
    return 0;
  }
}
