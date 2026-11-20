import { useSyncExternalStore } from "react";
// No relative imports (signal.ts, stored.ts): scripts/test-sounds.mjs loads this file in plain Node.

export type Sound = "click" | "on" | "off" | "notification" | "success" | "error";
type Preferences = { enabled: boolean; volume: number };
const KEY = "roalt.sounds";
// Off until the user opts in, so upgrades don't suddenly start clicking.
const DEFAULT: Preferences = { enabled: false, volume: 0.35 };
function read(): Preferences {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "null");
    return { enabled: typeof saved?.enabled === "boolean" ? saved.enabled : DEFAULT.enabled,
      volume: typeof saved?.volume === "number" && Number.isFinite(saved.volume) ? Math.max(0, Math.min(1, saved.volume)) : DEFAULT.volume };
  } catch { return DEFAULT; }
}
let preferences = read();
const subscribers = new Set<() => void>();
const subscribe = (f: () => void) => { subscribers.add(f); return () => { subscribers.delete(f); }; };
export function useSoundPreferences() {
  return useSyncExternalStore(subscribe, () => preferences);
}
export function setSoundPreferences(patch: Partial<Preferences>) {
  preferences = { ...preferences, ...patch };
  try { localStorage.setItem(KEY, JSON.stringify(preferences)); } catch { /* Session-only if storage is unavailable. */ }
  if (!preferences.enabled || !preferences.volume) active.forEach((node) => { try { node.stop(); } catch { /* Already ended. */ } });
  subscribers.forEach((f) => f());
}
let context: AudioContext | null = null;
const active = new Set<OscillatorNode>();
let installed = false;
let lastControl = -Infinity;
let lastNotice = -Infinity;
let idle: ReturnType<typeof setTimeout> | undefined;
// A running context renders silence all session: suspend it 30 s after the last input or sound.
// Not sooner: suspending closes the output stream, and reopening it delays or clips the next sound.
function rest() {
  clearTimeout(idle);
  idle = setTimeout(() => {
    if (!active.size) void context?.suspend().catch(() => {});
  }, 30_000);
}
function unlock() {
  if (!preferences.enabled || !preferences.volume) return;
  try {
    context ??= new AudioContext();
    if (context.state === "suspended") void context.resume().catch(() => {});
  } catch { /* Audio must never interrupt an action. */ }
  rest();
}
/** Short locally synthesized sounds; no network or audio assets. */
export function playSound(sound: Sound) {
  if (!preferences.enabled || preferences.volume === 0 || document.hidden || !context) return;
  if (context.state === "suspended") {
    void context.resume().then(() => { if (context?.state === "running") playSound(sound); }).catch(() => {});
    return;
  }
  if (context.state !== "running") return;
  const notice = sound === "notification" || sound === "success" || sound === "error";
  const stamp = performance.now();
  if (stamp - (notice ? lastNotice : lastControl) < (notice ? 1000 : 35)) return;
  if (notice) lastNotice = stamp; else lastControl = stamp;
  // Let a notification replace the click that immediately preceded it.
  if (notice && stamp - lastControl < 120) active.forEach((node) => { try { node.stop(); } catch { /* Already ended. */ } });
  const ctx = context;
  const note = (from: number, to: number, duration: number, delay = 0) => {
    const oscillator = ctx.createOscillator();
    const gain = ctx.createGain();
    const start = ctx.currentTime + delay;
    oscillator.type = "sine";
    oscillator.frequency.setValueAtTime(from, start);
    oscillator.frequency.exponentialRampToValueAtTime(to, start + duration);
    gain.gain.setValueAtTime(0.0001, start);
    gain.gain.exponentialRampToValueAtTime(0.12 * preferences.volume, start + 0.005);
    gain.gain.exponentialRampToValueAtTime(0.0001, start + duration);
    oscillator.connect(gain);
    gain.connect(ctx.destination);
    active.add(oscillator);
    oscillator.onended = () => { active.delete(oscillator); oscillator.disconnect(); gain.disconnect(); };
    oscillator.start(start);
    oscillator.stop(start + duration + 0.01);
  };
  try {
    if (sound === "click") note(650, 430, 0.04);
    else if (sound === "on") { note(480, 480, 0.045); note(720, 720, 0.065, 0.045); }
    else if (sound === "off") { note(600, 600, 0.045); note(400, 400, 0.065, 0.045); }
    else if (sound === "notification") { note(740, 740, 0.08); note(620, 620, 0.12, 0.09); }
    else if (sound === "success") { note(660, 660, 0.07); note(880, 880, 0.10, 0.08); }
    else { note(240, 210, 0.09); note(190, 170, 0.12, 0.10); }
  } catch { /* An unavailable audio device doesn't affect the UI. */ }
  rest();
}
export function previewSound() {
  unlock();
  if (context?.state === "suspended") void context.resume().then(() => playSound("success")).catch(() => {});
  else playSound("success");
}
/** Controls that pick one of several: a click, or nothing when it is already the one. */
const CHOICE = '[data-choice], .seg, .swatch, .preset, .island-item, .island-child, .rail-item, [role="tab"], [role="option"], [role="radio"], [role="menuitemradio"]';

/** Read ARIA state before React starts a possibly asynchronous settings update. */
export function interactionSound(target: HTMLElement): Sound | null {
  if (target instanceof HTMLInputElement && ["checkbox", "radio"].includes(target.type)) {
    return target.type === "radio" ? "click" : target.checked ? "on" : "off";
  }
  const checked = target.getAttribute("aria-checked");
  const pressed = target.getAttribute("aria-pressed");
  const selected = target.getAttribute("aria-selected");
  const current = target.getAttribute("aria-current") === "page" ? "true" : null;
  // Picking a preset, tab, place or option is selection, not switching a feature off.
  if (target.matches(CHOICE)) {
    return [checked, pressed, selected, current].includes("true") ? null : "click";
  }
  const state = checked ?? pressed;
  return state === "true" ? "off" : state === "false" || state === "mixed" ? "on" : "click";
}

export function installSounds() {
  if (installed) return;
  installed = true;
  document.addEventListener("pointerdown", unlock, { capture: true, passive: true });
  document.addEventListener("keydown", unlock, { capture: true, passive: true });
  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target.closest<HTMLElement>("button, input[type=checkbox], input[type=radio], input[type=range], input[type=color], summary") : null;
    if (!target || target.matches(":disabled, [aria-disabled=true]") || target.closest('[data-sound="off"]')) return;
    const sound = interactionSound(target);
    if (sound) playSound(sound);
  }, { capture: true });
  window.addEventListener("storage", (event) => {
    if (event.key === KEY || event.key === null) { preferences = read(); subscribers.forEach((f) => f()); }
  });
}
