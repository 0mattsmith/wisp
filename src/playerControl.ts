// How the main window and menu bar panel talk to the pop-out player.
// The player window owns the audio; everything else sends it commands and
// listens for what it is doing.
import { useSyncExternalStore } from "react";
import { call, on, send } from "./api";

export type PlayerCommand =
  | { type: "play"; ids: string[]; index: number; seq: number; muted?: boolean }
  | { type: "toggle" }
  | { type: "volume"; value: number }
  | { type: "query" };

export interface PlayerState {
  /** The job id of the track that is loaded, if any. */
  id: string | null;
  playing: boolean;
}

let state: PlayerState = { id: null, playing: false };
const listeners = new Set<() => void>();
let listening = false;

function ensureListening() {
  if (listening) return;
  listening = true;
  on<PlayerState>("player-state", (next) => {
    state = next;
    listeners.forEach((l) => l());
  });
  // Ask the player what it is doing, in case it was already going when this window opened.
  send("player-command", { type: "query" } satisfies PlayerCommand, "player");
}

export function usePlayerState(): PlayerState {
  return useSyncExternalStore(
    (l) => {
      ensureListening();
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => state,
  );
}

/**
 * Open the player and start `ids[index]`, with the rest of `ids` as the queue.
 * The request goes through the backend, which holds on to it until the player
 * confirms it: the player window may still be starting up.
 */
export async function playTracks(ids: string[], index: number) {
  await call("player_play", { ids, index });
}

export const togglePlayback = () => send("player-command", { type: "toggle" } satisfies PlayerCommand, "player");
export const showPlayer = () => call("player_show");
