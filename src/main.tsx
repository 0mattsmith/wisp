import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { devLog, inTauri, isMac, windowLabel } from "./api";
import { App } from "./App";
import { Panel } from "./Panel";
import { Player } from "./Player";
import { startStore } from "./store";
import { startTheme } from "./theme";
import "./styles.css";

const isPanel = windowLabel === "panel";
const isPlayer = windowLabel === "player";
// Both small windows are see-through around their rounded card.
document.documentElement.classList.toggle("is-panel", isPanel || isPlayer);
// On macOS the sidebar and the menu bar panel sit on real system glass.
document.documentElement.classList.toggle("vibrant", inTauri && isMac && !isPlayer);

// Script errors are otherwise invisible in a desktop window.
window.addEventListener("error", (e) => devLog(`error: ${e.message} (${e.filename}:${e.lineno})`));
window.addEventListener("unhandledrejection", (e) => devLog(`unhandled: ${String(e.reason)}`));

devLog("window script started");
// The theme follows the settings as soon as they arrive, and every change after.
startTheme();
startStore();
createRoot(document.getElementById("root")!).render(<StrictMode>{isPlayer ? <Player /> : isPanel ? <Panel /> : <App />}</StrictMode>);
