// A highlight that flows like a drop of liquid to whatever element is "current" in a container: its
// two edges are springs of their own, the leading one stiff and the trailing one soft, so it
// stretches toward where it is going, lets go of where it was, overshoots a little and settles. It
// measures its target every frame while it moves, so it keeps up with labels and groups that spring
// open at the same time. Reduced motion or Off: it jumps (no travel), still following every change.
import { useLayoutEffect, type RefObject } from "react";
import { prefersReducedMotion } from "./reduced";
import { stepEdges, type Edge } from "./spring";

export type Follower = { kick: () => void; stop: () => void };

export type Box = { l: number; r: number; t: number; b: number };

/** Keeps `el` (absolutely placed in `container`) over `target()`; null hides it. `onFrame` runs after
 * each write with the drop's box, for followers that react to each other (the island's goo). */
export function follow(container: HTMLElement, el: HTMLElement, target: () => HTMLElement | null, onFrame?: (box: Box | null) => void): Follower {
  let l: Edge | null = null;
  let r: Edge | null = null;
  let dir = 1;
  let lastCenter: number | null = null;
  let raf = 0;
  let last = 0;
  let stopped = false;

  // Whether the target is being dragged (useGridDrag moves it with a transform under the pointer).
  let dragged = false;

  // Layout boxes, not painted ones: a tile still scaling in (the island's entrance, a press) must not
  // shrink the drop, and a transform ending changes no layout that would bring it back. A dragged
  // tile is the exception: the light goes with it, so it is measured where it is painted.
  const measure = (): Box | null => {
    const t = target();
    if (!t || !t.isConnected || !t.offsetWidth) return null;
    dragged = !!t.closest(".tile-drag");
    let x = 0;
    let y = 0;
    let n: HTMLElement | null = t;
    while (n && n !== container && !dragged) {
      x += n.offsetLeft;
      y += n.offsetTop;
      n = n.offsetParent as HTMLElement | null;
    }
    if (n !== container) {
      // Dragged, or the container isn't on the offset chain (not positioned): painted boxes.
      const c = container.getBoundingClientRect();
      const b = t.getBoundingClientRect();
      const s = container.offsetWidth ? c.width / container.offsetWidth : 1;
      return { l: (b.left - c.left) / s + container.scrollLeft, r: (b.right - c.left) / s + container.scrollLeft, t: (b.top - c.top) / s, b: (b.bottom - c.top) / s };
    }
    // Offsets are from the container's padding edge; the drop sits in the same space.
    return { l: x, r: x + t.offsetWidth, t: y, b: y + t.offsetHeight };
  };

  // An overshoot past the container's edge would be clipped flat (the tab strip scrolls): the drop
  // squashes against the edge instead.
  const write = (box: Box, squash: number) => {
    const lo = Math.min(box.l, 0);
    const hi = Math.max(box.r, container.scrollWidth);
    const x0 = Math.max(lo, Math.min(l!.x, hi));
    const x1 = Math.max(x0, Math.min(r!.x, hi));
    el.style.opacity = "1";
    el.style.transform = `translate(${x0}px, ${box.t}px) scaleY(${squash.toFixed(3)})`;
    el.style.width = `${x1 - x0}px`;
    el.style.height = `${box.b - box.t}px`;
    onFrame?.({ l: x0, r: x1, t: box.t, b: box.b });
  };

  const hide = () => {
    el.style.opacity = "0";
    l = r = null;
    lastCenter = null;
    onFrame?.(null);
  };

  // The current target's own size is watched too: its words can change width (the font loading
  // after the first paint, a badge leaving) without the container changing size.
  let watched: HTMLElement | null = null;
  const watch = () => {
    const t = target();
    if (t === watched) return;
    if (watched) ro.unobserve(watched);
    watched = t;
    if (t) ro.observe(t);
  };

  const step = (now: number) => {
    raf = 0;
    if (stopped) return;
    watch();
    const box = measure();
    if (!box) return hide();
    const center = (box.l + box.r) / 2;
    if (lastCenter !== null && Math.abs(center - lastCenter) > 1) dir = Math.sign(center - lastCenter);
    lastCenter = center;
    if (!l || !r || prefersReducedMotion()) {
      l = { x: box.l, v: 0 };
      r = { x: box.r, v: 0 };
      write(box, 1);
      return;
    }
    const elapsed = (now - (last || now - 16)) / 1000;
    last = now;
    const resting = stepEdges(l, r, box.l, box.r, dir, elapsed);
    // Moving fast it thins a little, like a stretched drop.
    const speed = Math.max(Math.abs(l.v), Math.abs(r.v));
    write(box, 1 - Math.min(0.14, speed / 9000));
    // While dragged, every frame: the tile moves by transform, which no observer reports.
    if (resting && !dragged) {
      last = 0;
      return;
    }
    raf = requestAnimationFrame(step);
  };

  const kick = () => {
    if (stopped || raf) return;
    raf = requestAnimationFrame(step);
  };

  // Layout changes (labels opening, a group folding, the window resizing) and selection changes.
  const ro = new ResizeObserver(kick);
  ro.observe(container);
  const mo = new MutationObserver(kick);
  // `class`: a drag starting or ending (.tile-drag) wakes the drop.
  mo.observe(container, { subtree: true, childList: true, attributes: true, attributeFilter: ["aria-current", "aria-pressed", "aria-selected", "aria-checked", "class"] });
  container.addEventListener("scroll", kick, { passive: true });
  document.fonts?.addEventListener("loadingdone", kick);
  void document.fonts?.ready.then(kick);
  kick();

  return {
    kick,
    stop: () => {
      // A stopped follower leaves nothing behind for the next one to fight.
      stopped = true;
      cancelAnimationFrame(raf);
      ro.disconnect();
      mo.disconnect();
      container.removeEventListener("scroll", kick);
      document.fonts?.removeEventListener("loadingdone", kick);
    },
  };
}

/** `follow` for a component: `el` flows to the first match of `selector` inside `container`. With
 * `container` null, inside `el`'s parent element: for a layer rendered by a child component, whose
 * parent's refs aren't attached yet when its effects run. */
export function useLiquid(container: RefObject<HTMLElement | null> | { readonly current: HTMLElement | null } | null, el: RefObject<HTMLElement | null>, selector: string, onFrame?: (box: Box | null) => void, remount?: unknown) {
  useLayoutEffect(() => {
    const e = el.current;
    const c = container ? container.current : (e?.parentElement ?? null);
    if (!c || !e) return;
    return follow(c, e, () => c.querySelector<HTMLElement>(selector), onFrame).stop;
    // onFrame is read once: callers pass a stable function. `remount`: the elements were replaced.
  }, [container, el, selector, remount]); // eslint-disable-line react-hooks/exhaustive-deps
}
