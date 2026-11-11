import { useSyncExternalStore } from "react";

// Motion, one source for CSS and JS: this PC's choice (Settings › Appearance › Motion), kept in
// localStorage so it survives a restart. "on" (the default, whatever Windows says): the liquid
// motion. "reduced": html[data-rm], fades and progress only. "off": html[data-rm][data-motion-off],
// nothing animates. ?rm=1 forces reduced in a dev preview. CSS reads the attributes
// (styles/motion.css); motion reads them through MotionRoot; WAAPI calls read prefersReducedMotion().
export type MotionPreference = "on" | "reduced" | "off";
const KEY = "roalt.motion";

const subs = new Set<() => void>();
let installed = false;
let pref: MotionPreference = "on";
let apply = () => {};

/** What an older RoALT stored: "full" and "system" (no key) are On now; "reduced" stays. */
function read(stored: string | null): MotionPreference {
  return stored === "reduced" || stored === "off" ? stored : "on";
}

export function installReducedMotion() {
  if (installed) return;
  installed = true;
  let forced = false;
  try {
    forced = import.meta.env.DEV && new URLSearchParams(location.search).get("rm") === "1";
    pref = read(localStorage.getItem(KEY));
  } catch {
    // No URL or storage to read: On.
  }
  apply = () => {
    const root = document.documentElement;
    root.toggleAttribute("data-rm", forced || pref !== "on");
    root.toggleAttribute("data-motion-off", pref === "off");
    subs.forEach((f) => f());
  };
  apply();
}

/** Less motion: Reduced or Off. Travel, scale and blur go; Reduced keeps fades and progress. */
export const prefersReducedMotion = () => document.documentElement.hasAttribute("data-rm");

/** No motion at all. */
export const motionOff = () => document.documentElement.hasAttribute("data-motion-off");

/** This PC's choice now. */
export const motionPreference = () => pref;

/** This PC's choice, kept on this PC only. */
export function setMotionPreference(next: MotionPreference) {
  pref = next;
  try {
    if (next === "on") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, next);
  } catch {
    // Not kept: it applies until RoALT restarts.
  }
  apply();
}

const subscribe = (f: () => void) => {
  subs.add(f);
  return () => void subs.delete(f);
};

/** True while motion should be reduced (Reduced or Off); re-renders when the choice changes. */
export function useReducedMotionFlag() {
  return useSyncExternalStore(subscribe, prefersReducedMotion, () => false);
}

/** True while motion is Off. */
export function useMotionOff() {
  return useSyncExternalStore(subscribe, motionOff, () => false);
}

/** This PC's choice, kept current. */
export function useMotionPreference() {
  return useSyncExternalStore(subscribe, () => pref, () => "on" as MotionPreference);
}
