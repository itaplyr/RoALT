import { useLayoutEffect, type RefObject } from "react";
import type { Settings, VaultStatus } from "../../api";
import { requestNavigation } from "../../app/navigation";
import { qk, queryClient } from "../../data";
import { prefersReducedMotion } from "../../ui/motion";
import { ENTRIES, entry, shown, type Ctx, type Entry, type Place } from "./registry";

// The row a jump waits for, and the rows on screen that can take one.
let pending: string | null = null;
const targets = new Map<string, HTMLElement>();

/** Row conditions as they are now, outside React. */
function snapshot(): Ctx | null {
  const s = queryClient.getQueryData<Settings>(qk.settings);
  const vault = queryClient.getQueryData<VaultStatus>(qk.vault);
  return s && vault ? { s, vault, background: !!queryClient.getQueryData<ArrayBuffer>(qk.background)?.byteLength } : null;
}

/** Asks for row `id`: once it is on screen it scrolls to the centre, takes the focus and flashes. */
export function requestJump(id: string) {
  pending = id;
  if (targets.has(id)) requestAnimationFrame(() => land(id));
  else if (import.meta.env.DEV && !("link" in entry(id).place))
    setTimeout(() => pending === id && console.warn(`settings: the jump to "${id}" found no row within 1 s`), 1000);
}

/** Makes `ref` the jump target for entry `id` while it is mounted. SettingRow does this for its own
 * rows; a view gives one to anything else an entry stands for (a preset card, say). `onItsScreen`:
 * false for a row drawn away from its screen (search results), which the dev check then ignores. */
export function useJumpTarget(id: string, ref: RefObject<HTMLElement | null>, onItsScreen = true) {
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    targets.set(id, el);
    if (pending === id) requestAnimationFrame(() => land(id));
    if (import.meta.env.DEV && onItsScreen) audit(id);
    return () => void (targets.get(id) === el && targets.delete(id));
  }, [id, ref, onItsScreen]);
}

/** Opens the setting's view (and Settings area) and lands on its row; on its parent's (or fallback's)
 * while the row itself is hidden. */
export function jumpTo(id: string) {
  const ctx = snapshot();
  let e = entry(id);
  while (ctx && !shown(e, ctx) && (e.fallback ?? e.parent)) e = entry((e.fallback ?? e.parent)!);
  requestNavigation(e.place.view, e.place.view === "settings" ? e.place.area : undefined);
  requestJump(e.id);
}

const FOCUSABLE = ":is(button, input, select, textarea, a[href], [tabindex]:not([tabindex='-1'])):not(:disabled)";

// The row is looked up a frame after the ask: a copy on its way out (a search's, as the area
// replaces it) has handed over to the one arriving by then.
function land(id: string) {
  const el = targets.get(id);
  if (pending !== id || !el?.isConnected) return;
  pending = null;
  const reduced = prefersReducedMotion();
  const control = el.querySelector<HTMLElement>(`.set-control ${FOCUSABLE}`) ?? (el.matches(FOCUSABLE) ? el : el.querySelector<HTMLElement>(FOCUSABLE));
  if (!control) el.tabIndex = -1;
  (control ?? el).focus({ preventScroll: true });
  const scroller = el.closest<HTMLElement>(".scroll");
  const r = el.getBoundingClientRect();
  const v = scroller?.getBoundingClientRect();
  if (!scroller || !v || (r.top >= v.top && r.bottom <= v.bottom)) return flash(el, reduced);
  el.scrollIntoView({ block: "center", behavior: reduced ? "instant" : "smooth" });
  if (reduced) return flash(el, reduced);
  // The flash waits for the scroll, so it is seen; a short scroll may not fire scrollend.
  const done = () => {
    clearTimeout(timer);
    scroller.removeEventListener("scrollend", done);
    flash(el, reduced);
  };
  const timer = setTimeout(done, 700);
  scroller.addEventListener("scrollend", done);
}

/** An accent wash 0 → 14 % → 0 with a ring (it must show on see-through panels too); reduced
 * motion: the same tint, still, for 1.5 s. Shadows, not a background: a filled card (a preset)
 * keeps its own fill under the wash. */
function flash(el: HTMLElement, reduced: boolean) {
  // Forced colours drop shadows: a system-coloured outline instead.
  if (matchMedia("(forced-colors: active)").matches) {
    const on = { outline: "2px solid Highlight", outlineOffset: "-2px" };
    el.animate(reduced ? [on, on] : [{ ...on, outlineColor: "transparent" }, { ...on, offset: 0.3 }, { ...on, outlineColor: "transparent" }], { duration: reduced ? 1500 : 900, easing: "ease-out" });
    return;
  }
  const lit = { boxShadow: "inset 0 0 0 1px color-mix(in srgb, var(--accent-text) 40%, transparent), inset 0 0 0 100vmax color-mix(in srgb, var(--accent-text) 14%, transparent)" };
  const clear = { boxShadow: "inset 0 0 0 1px transparent, inset 0 0 0 100vmax transparent" };
  el.animate(reduced ? [lit, lit] : [clear, { ...lit, offset: 0.3 }, clear], { duration: reduced ? 1500 : 900, easing: "ease-out" });
}

// Dev: entries of the view (or Settings area) on screen that no row drew.
const drawn = new Set<string>();
const told = new Set<string>();
let auditTimer = 0;
const screenOf = (p: Place) => (p.view === "settings" ? `settings:${p.area}` : p.view);

function audit(id: string) {
  drawn.add(id);
  clearTimeout(auditTimer);
  auditTimer = window.setTimeout(() => {
    const screen = screenOf(entry(id).place);
    const ctx = snapshot();
    const missing = ENTRIES.filter((e: Entry) => screenOf(e.place) === screen && !("link" in e.place) && !drawn.has(e.id) && !told.has(e.id) && (!ctx || shown(e, ctx)));
    for (const e of missing) told.add(e.id);
    if (missing.length) console.warn(`settings registry: nothing on ${screen} drew ${missing.map((e) => e.id).join(", ")}`);
  }, 1500);
}
