import { useCallback, useEffect, useLayoutEffect, useRef, type RefObject } from "react";
import { isRtl } from "../../i18n";
import { prefersReducedMotion } from "./reduced";
import { DUR, EASE_CSS, RISE, STAGGER } from "./tokens";
import { isViewEntering } from "./viewEnter";

/** The delay of the i-th item of a list arriving: 24 ms apart, capped. Seconds. */
export const staggerDelay = (i: number) => Math.min(i, STAGGER.max) * STAGGER.step;

/** One element rises into place (WAAPI, so React moving it later doesn't restart it). Skipped while
 * its view is entering (the view's own fade carries it); reduced motion: a short fade. */
export function riseIn(el: Element, i = 0) {
  if (isViewEntering()) return;
  if (prefersReducedMotion()) {
    el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 120, easing: "ease-out", fill: "backwards" });
    return;
  }
  el.animate([{ opacity: 0, translate: `0 ${RISE.md}px` }, { opacity: 1, translate: "0 0" }], {
    duration: DUR.base + 60,
    delay: staggerDelay(i) * 1000,
    easing: EASE_CSS.out,
    fill: "backwards",
  });
}

/** Which ids are new since the last render, in order (for staggered entrances). Nothing counts as new
 * on the first render, or when more than `limit` arrive at once (a long list just appears). */
export function useFreshItems(ids: readonly (string | number)[], limit: number = STAGGER.listLimit) {
  // The committed set, updated after commit: a render React throws away (StrictMode) sees the same.
  const seen = useRef<Set<string | number> | null>(null);
  const prev = seen.current;
  const fresh = prev ? ids.filter((id) => !prev.has(id)) : [];
  useLayoutEffect(() => {
    seen.current = new Set(ids);
  });
  return fresh.length > limit ? [] : fresh;
}

/** Where each `[data-flip]` child of `root` sits, from the list's inline-start corner; none past the
 * long-list limit. In the container's own content space: scrolling it, or the page around it, between
 * two renders moves nothing. */
function spots(root: HTMLElement) {
  const els = [...root.querySelectorAll<HTMLElement>(":scope > [data-flip]")];
  const box = root.getBoundingClientRect();
  const rtl = isRtl();
  const at = new Map<Element, [number, number]>();
  if (els.length <= STAGGER.listLimit)
    for (const el of els) {
      const r = el.getBoundingClientRect();
      // scrollLeft runs negative in RTL, so the same sum holds both ways.
      at.set(el, [(rtl ? r.right - box.right : r.left - box.left) + root.scrollLeft, r.top - box.top + root.scrollTop]);
    }
  return at;
}

/** Neighbours glide to their new places when items are added, removed or resized (FLIP on WAAPI).
 * Children of `container` carrying `data-flip` are measured on every render. Skips while reduced, for
 * elements being dragged (.tile-drag), and when `skip()` says so (useGridDrag already glides a
 * reorder). Lists over 40 items just jump.
 * With `key` (Home's grid, tab strip and rail: the ids in order): useGridDrag glides its own moves, so
 * call the returned `hold()` just before something else moves the items (a drag's glide, a resize, a
 * density switch, a rise). They are measured again `ms` later, once still, so a later render can't
 * replay a glide from where an animation had them. A change of `key` gets the same quiet time once its
 * glides start, and so does a window resize, which reflows them without a render. */
export function useFlip(container: RefObject<HTMLElement | null>, opts: { skip?: () => boolean; duration?: number; key?: string } = {}) {
  const last = useRef(new Map<Element, [number, number]>());
  const held = useRef(false);
  const timer = useRef(0);
  const hold = useCallback(
    (ms: number = DUR.move + 120) => {
      held.current = true;
      clearTimeout(timer.current);
      timer.current = window.setTimeout(() => {
        held.current = false;
        if (container.current) last.current = spots(container.current);
      }, ms);
    },
    [container],
  );
  useLayoutEffect(() => {
    const root = container.current;
    if (!root) return;
    const before = last.current;
    last.current = spots(root);
    if (held.current || prefersReducedMotion() || opts.skip?.()) return;
    for (const [el, [x, y]] of last.current) {
      const was = before.get(el);
      if (!was || el.classList.contains("tile-drag")) continue;
      const [dx, dy] = [was[0] - x, was[1] - y];
      if (Math.abs(dx) < 1 && Math.abs(dy) < 1) continue;
      el.animate([{ translate: `${dx}px ${dy}px` }, { translate: "0 0" }], { duration: opts.duration ?? DUR.move + 80, easing: EASE_CSS.out });
    }
  });
  const { key } = opts;
  const keyed = key !== undefined;
  const seen = useRef(key);
  useLayoutEffect(() => {
    if (seen.current === key) return;
    seen.current = key;
    hold();
  }, [key, hold]);
  useEffect(() => {
    const resized = () => hold();
    if (keyed) window.addEventListener("resize", resized);
    return () => {
      window.removeEventListener("resize", resized);
      clearTimeout(timer.current);
    };
  }, [hold, keyed]);
  return hold;
}
