// Light, dark, or following the system, for every window. The page colours
// come from CSS (data-theme on <html>); the native parts of the window (the
// macOS sidebar glass, the menu bar panel's frost, title bar controls) are told
// too, so they never disagree with the page.
import { useSyncExternalStore } from "react";
import { inTauri } from "./api";
import { currentSettings, onSettings } from "./store";

export type Appearance = "system" | "light" | "dark";

const media = window.matchMedia("(prefers-color-scheme: dark)");
const listeners = new Set<() => void>();
let resolved: "light" | "dark" = media.matches ? "dark" : "light";

function apply() {
  const choice = (currentSettings()?.appearance ?? "system") as Appearance;
  const next = choice === "system" ? (media.matches ? "dark" : "light") : choice;
  const root = document.documentElement;
  root.dataset.theme = next;
  root.style.colorScheme = next;
  if (next !== resolved) {
    resolved = next;
    listeners.forEach((l) => l());
  }
  if (inTauri) {
    import("@tauri-apps/api/window").then(({ getCurrentWindow }) =>
      getCurrentWindow()
        .setTheme(choice === "system" ? null : choice)
        .catch(() => {}),
    );
  }
}

let started = false;
export function startTheme() {
  apply();
  if (started) return;
  started = true;
  onSettings(apply);
  media.addEventListener("change", apply);
}

/** The theme actually showing, light or dark. */
export const useResolvedTheme = () =>
  useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => resolved,
  );

/** System → Light → Dark → System, for the quick toggle buttons. */
export function nextAppearance(current: Appearance): Appearance {
  return current === "system" ? "light" : current === "light" ? "dark" : "system";
}
