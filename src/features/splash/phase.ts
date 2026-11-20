import { useSyncExternalStore } from "react";
import { signal } from "../../ui/signal";

// Where the start-up splash is: playing, fading out, or gone. The island waits for "out" to inflate,
// so its one moment follows the splash's instead of hiding under it.
export type SplashPhase = "play" | "out" | "gone";
let phase: SplashPhase = "gone";
const { subscribe, notify } = signal();

export function setSplashPhase(next: SplashPhase) {
  if (next === phase) return;
  phase = next;
  notify();
}

export const useSplashPhase = () => useSyncExternalStore(subscribe, () => phase);
