import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { emit, emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

/** False when the UI is opened in a plain browser, where sample data stands in for the backend. */
export const inTauri = "__TAURI_INTERNALS__" in window;

export const windowLabel = inTauri
  ? getCurrentWindow().label
  : location.hash === "#panel"
    ? "panel"
    : location.hash === "#player"
      ? "player"
      : "main";

export const isMac = navigator.userAgent.includes("Mac");

export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) return invoke<T>(cmd, args);
  const mock = await import("./mock");
  return mock.mockCall(cmd, args) as T;
}

export async function on<T>(event: string, cb: (payload: T) => void): Promise<() => void> {
  if (inTauri) return listen<T>(event, (e) => cb(e.payload));
  const mock = await import("./mock");
  return mock.mockListen(event, cb as (p: unknown) => void);
}

export async function reveal(path: string) {
  if (!inTauri) return;
  const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
  await revealItemInDir(path);
}

export async function openExternal(url: string) {
  if (!inTauri) return void window.open(url, "_blank");
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}

export async function openPath(path: string) {
  if (!inTauri) return;
  const { openPath } = await import("@tauri-apps/plugin-opener");
  await openPath(path);
}

export async function readClipboard(): Promise<string> {
  try {
    if (inTauri) {
      const { readText } = await import("@tauri-apps/plugin-clipboard-manager");
      return (await readText()) ?? "";
    }
    return "";
  } catch {
    return "";
  }
}

export async function pickFolder(current?: string): Promise<string | null> {
  if (!inTauri) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({ directory: true, defaultPath: current });
  return typeof picked === "string" ? picked : null;
}

export async function pickFiles(kind: "audio" | "image"): Promise<string[]> {
  if (!inTauri) return [];
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    multiple: kind === "audio",
    filters:
      kind === "audio"
        ? [{ name: "Audio", extensions: ["mp3", "flac", "wav", "m4a", "aac", "ogg", "opus", "aiff", "aif", "wma", "mp4", "webm"] }]
        : [{ name: "Images", extensions: ["jpg", "jpeg", "png", "webp"] }],
  });
  if (!picked) return [];
  return Array.isArray(picked) ? picked : [picked];
}

/** Subscribe to files dropped on the window. `over` reports hover state. */
export async function onFileDrop(
  onDrop: (paths: string[]) => void,
  onHover: (over: boolean) => void,
): Promise<() => void> {
  if (!inTauri) return () => {};
  const { getCurrentWebview } = await import("@tauri-apps/api/webview");
  return getCurrentWebview().onDragDropEvent((e) => {
    if (e.payload.type === "enter" || e.payload.type === "over") onHover(true);
    else if (e.payload.type === "leave") onHover(false);
    else if (e.payload.type === "drop") {
      onHover(false);
      onDrop(e.payload.paths);
    }
  });
}

/** Whether the app is registered to open when the user logs in. */
export async function loginStartEnabled(): Promise<boolean> {
  if (!inTauri) return false;
  const { isEnabled } = await import("@tauri-apps/plugin-autostart");
  return isEnabled();
}

export async function setLoginStart(on: boolean): Promise<void> {
  if (!inTauri) return;
  const { enable, disable } = await import("@tauri-apps/plugin-autostart");
  await (on ? enable() : disable());
}

/** Send an event to every window, or to one window by its label. */
export async function send(event: string, payload: unknown, target?: string): Promise<void> {
  if (!inTauri) return;
  await (target ? emitTo(target, event, payload) : emit(event, payload));
}

/** A URL the page can load a local file from (the file must first be allowed by the backend). */
export function fileSrc(path: string): string {
  return inTauri ? convertFileSrc(path) : "";
}

export async function startDrag(): Promise<void> {
  if (inTauri) await getCurrentWindow().startDragging();
}

let devBuild: Promise<boolean> | null = null;

/** In development builds, print a line in the terminal that launched the app. */
export function devLog(message: string) {
  if (!inTauri) return;
  devBuild ??= invoke<boolean>("dev_enabled").catch(() => false);
  devBuild.then((dev) => dev && invoke("dev_log", { message }).catch(() => {}));
}

export async function copyText(text: string): Promise<void> {
  if (inTauri) {
    const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
    await writeText(text);
  } else {
    await navigator.clipboard.writeText(text);
  }
}
