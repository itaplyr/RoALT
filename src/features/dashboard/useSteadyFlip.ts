import { useCallback, useEffect, useLayoutEffect, useRef, type RefObject } from "react";
import { isRtl } from "../../i18n";
import { DUR, EASE_CSS, prefersReducedMotion, STAGGER } from "../../ui/motion";

/** Where each `[data-flip]` child of `root` sits, from the list's start corner; none past the long-list
 * limit. */
function spots(root: HTMLElement) {
  const els = [...root.querySelectorAll<HTMLElement>(":scope > [data-flip]")];
  const box = root.getBoundingClientRect();
  const rtl = isRtl();
  const at = new Map<Element, [number, number]>();
  if (els.length <= STAGGER.listLimit)
    for (const el of els) {
      const r = el.getBoundingClientRect();
      at.set(el, [rtl ? r.right - box.right : r.left - box.left, r.top - box.top]);
    }
  return at;
}

/** Home's grid, tab strip and rail: items glide to where a render put them. As useFlip, but measured
 * inside the list (`ref` needs a box of its own), so a scroll or a notice above it is no move.
 * useGridDrag glides its own moves: call `hold()` just before something else moves the items (a
 * drag's glide, a resize, a density switch, a rise). They are measured again `ms` later, once still,
 * so a later render can't replay a glide from where an animation had them. A change of `key` (the ids
 * in order) gets the same quiet time once its glides start, and so does a window resize, which
 * reflows them without a render. */
export function useSteadyFlip(ref: RefObject<HTMLElement | null>, key: string) {
  const last = useRef(new Map<Element, [number, number]>());
  const held = useRef(false);
  const timer = useRef(0);
  const hold = useCallback(
    (ms: number = DUR.move + 120) => {
      held.current = true;
      clearTimeout(timer.current);
      timer.current = window.setTimeout(() => {
        held.current = false;
        if (ref.current) last.current = spots(ref.current);
      }, ms);
    },
    [ref],
  );
  useLayoutEffect(() => {
    const root = ref.current;
    if (!root) return;
    const before = last.current;
    last.current = spots(root);
    if (held.current || prefersReducedMotion()) return;
    for (const [el, [x, y]] of last.current) {
      const was = before.get(el);
      if (!was || el.classList.contains("tile-drag")) continue;
      const [dx, dy] = [was[0] - x, was[1] - y];
      if (Math.abs(dx) >= 1 || Math.abs(dy) >= 1) el.animate([{ translate: `${dx}px ${dy}px` }, { translate: "0 0" }], { duration: DUR.move + 80, easing: EASE_CSS.out });
    }
  });
  const seen = useRef(key);
  useLayoutEffect(() => {
    if (seen.current === key) return;
    seen.current = key;
    hold();
  }, [key, hold]);
  useEffect(() => {
    const resized = () => hold();
    window.addEventListener("resize", resized);
    return () => {
      window.removeEventListener("resize", resized);
      clearTimeout(timer.current);
    };
  }, [hold]);
  return hold;
}
