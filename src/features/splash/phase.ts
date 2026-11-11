import { useSyncExternalStore } from "react";

// Where the start-up splash is: playing, fading out, or gone. The island waits for "out" to inflate,
// so its one moment follows the splash's instead of hiding under it.
export type SplashPhase = "play" | "out" | "gone";
let phase: SplashPhase = "gone";
const subs = new Set<() => void>();

export function setSplashPhase(next: SplashPhase) {
  if (next === phase) return;
  phase = next;
  subs.forEach((f) => f());
}

const subscribe = (f: () => void) => {
  subs.add(f);
  return () => void subs.delete(f);
};

export const useSplashPhase = () => useSyncExternalStore(subscribe, () => phase);
