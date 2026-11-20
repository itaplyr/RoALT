import { createElement, useSyncExternalStore, type KeyboardEvent } from "react";
import { isRtl } from "../i18n";
import { signal } from "./signal";

// Screen-reader announcements from anywhere (a batch finished, a reorder): one polite and one
// assertive live region, rendered once by <Announcer/>.
let polite = "";
let assertive = "";
let tick = 0;
const { subscribe, notify } = signal();

export function announce(text: string, mode: "polite" | "assertive" = "polite") {
  // A zero-width suffix that flips, so the same words announced twice are read twice.
  const mark = ++tick % 2 ? "​" : "";
  if (mode === "assertive") assertive = text + mark;
  else polite = text + mark;
  notify();
}

export function Announcer() {
  const p = useSyncExternalStore(subscribe, () => polite);
  const a = useSyncExternalStore(subscribe, () => assertive);
  return createElement(
    "div",
    { className: "sr-only" },
    createElement("div", { "aria-live": "polite" }, p),
    createElement("div", { "aria-live": "assertive" }, a),
  );
}

/** Whether a dialog or a popover (not a passive one, the tooltip) is open: shortcuts wait then. */
export const overlayOpen = () => !!document.querySelector("dialog[open], [popover]:popover-open:not([data-passive])");

/** Arrow keys (and Home, End) move focus between `selector` items inside the handler's element;
 * horizontal follows the reading direction. Returns true when it handled the key. */
export function rovingKeys(e: KeyboardEvent<HTMLElement>, selector: string, orientation: "horizontal" | "vertical" = "horizontal") {
  // Arrows in a text field (a toolbar's search) move its caret, not the focus.
  if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return false;
  const items = [...e.currentTarget.querySelectorAll<HTMLElement>(selector)].filter((el) => !el.matches(":disabled"));
  const at = items.indexOf(document.activeElement as HTMLElement);
  const fwd = orientation === "vertical" ? "ArrowDown" : isRtl() ? "ArrowLeft" : "ArrowRight";
  const back = orientation === "vertical" ? "ArrowUp" : isRtl() ? "ArrowRight" : "ArrowLeft";
  const next = e.key === fwd ? at + 1 : e.key === back ? at - 1 : e.key === "Home" ? 0 : e.key === "End" ? items.length - 1 : null;
  if (next === null || !items.length || e.altKey) return false;
  e.preventDefault();
  items[(next + items.length) % items.length].focus();
  return true;
}
