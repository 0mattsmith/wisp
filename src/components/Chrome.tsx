import { LoaderCircle, Monitor, Moon, RefreshCw, Sun, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { loadSettings, saveSettings, useLoadError, useSettings } from "../store";
import { nextAppearance, type Appearance } from "../theme";

/** A Finder-style page heading: a small line above a large title, actions on the right. */
export function PageHeader({ overline, title, children }: { overline: string; title: string; children?: ReactNode }) {
  return (
    <header className="page-header" data-tauri-drag-region>
      <div className="page-heading" data-tauri-drag-region>
        <span className="overline" data-tauri-drag-region>
          {overline}
        </span>
        <h1 data-tauri-drag-region>{title}</h1>
      </div>
      {children && <div className="page-actions">{children}</div>}
    </header>
  );
}

/** A round toolbar button with a tooltip. */
export function ToolButton({
  label,
  onClick,
  children,
  active = false,
  disabled = false,
}: {
  label: string;
  onClick: () => void;
  children: ReactNode;
  active?: boolean;
  disabled?: boolean;
}) {
  return (
    <button className={`tool-btn${active ? " on" : ""}`} title={label} aria-label={label} onClick={onClick} disabled={disabled}>
      {children}
    </button>
  );
}

const APPEARANCE_LABEL: Record<Appearance, string> = { system: "Match system", light: "Light", dark: "Dark" };

/** Cycles System → Light → Dark. */
export function ThemeButton({ className = "tool-btn" }: { className?: string }) {
  const settings = useSettings();
  const current = (settings?.appearance ?? "system") as Appearance;
  const next = nextAppearance(current);
  const Icon = current === "light" ? Sun : current === "dark" ? Moon : Monitor;
  return (
    <button
      className={className}
      title={`Appearance: ${APPEARANCE_LABEL[current]} (click for ${APPEARANCE_LABEL[next]})`}
      aria-label={`Appearance: ${APPEARANCE_LABEL[current]}`}
      disabled={!settings}
      onClick={() => saveSettings({ appearance: next })}
    >
      <Icon size={15} />
    </button>
  );
}

/** Shown instead of a blank page while settings load, or if they cannot. */
export function NotLoaded() {
  const error = useLoadError();
  if (!error) {
    return (
      <div className="empty quiet">
        <LoaderCircle className="spin" size={20} />
      </div>
    );
  }
  return (
    <div className="empty">
      <TriangleAlert size={22} />
      <p>Wisp could not load its settings.</p>
      <span className="selectable">{error}</span>
      <button className="soft-btn" onClick={() => loadSettings()}>
        <RefreshCw size={14} /> Try again
      </button>
    </div>
  );
}
