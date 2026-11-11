import { useLayoutEffect, useRef, type RefObject } from "react";
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

/** Neighbours glide to their new places when items are added, removed or resized (FLIP on WAAPI).
 * Children of `container` carrying `data-flip` are measured on every render. Skips while reduced, for
 * elements being dragged (.tile-drag), and when `skip()` says so (useGridDrag already glides a
 * reorder). Never use it on lists over 40 items. */
export function useFlip(container: RefObject<HTMLElement | null>, opts: { skip?: () => boolean; duration?: number } = {}) {
  const last = useRef(new Map<Element, DOMRect>());
  useLayoutEffect(() => {
    const root = container.current;
    if (!root) return;
    const els = [...root.querySelectorAll<HTMLElement>(":scope > [data-flip]")];
    // In the container's own content space: scrolling it, or the page around it, between two renders
    // moves nothing.
    const base = root.getBoundingClientRect();
    const now = new Map(
      els.map((el) => {
        const r = el.getBoundingClientRect();
        return [el, new DOMRect(r.left - base.left + root.scrollLeft, r.top - base.top + root.scrollTop, r.width, r.height)] as const;
      }),
    );
    const before = last.current;
    last.current = now;
    if (prefersReducedMotion() || opts.skip?.() || els.length > STAGGER.listLimit) return;
    for (const el of els) {
      const was = before.get(el);
      const r = now.get(el)!;
      if (!was || el.classList.contains("tile-drag")) continue;
      const [dx, dy] = [was.left - r.left, was.top - r.top];
      if (Math.abs(dx) < 1 && Math.abs(dy) < 1) continue;
      el.animate([{ translate: `${dx}px ${dy}px` }, { translate: "0 0" }], { duration: opts.duration ?? DUR.move + 80, easing: EASE_CSS.out });
    }
  });
}
