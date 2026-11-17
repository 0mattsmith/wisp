import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { devLog, windowLabel } from "./api";
import { App } from "./App";
import { Panel } from "./Panel";
import { Player } from "./Player";
import { startStore } from "./store";
import "./styles.css";

const isPanel = windowLabel === "panel";
const isPlayer = windowLabel === "player";
// Both small windows are see-through around their rounded card.
document.documentElement.classList.toggle("is-panel", isPanel || isPlayer);

// Script errors are otherwise invisible in a desktop window.
window.addEventListener("error", (e) => devLog(`error: ${e.message} (${e.filename}:${e.lineno})`));
window.addEventListener("unhandledrejection", (e) => devLog(`unhandled: ${String(e.reason)}`));

devLog("window script started");
startStore();
createRoot(document.getElementById("root")!).render(<StrictMode>{isPlayer ? <Player /> : isPanel ? <Panel /> : <App />}</StrictMode>);
