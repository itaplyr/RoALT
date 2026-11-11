import { useEffect, useId, useLayoutEffect, useRef, useState, type FocusEvent, type KeyboardEvent, type ReactNode, type RefObject } from "react";
import { animate, motion, useMotionValue } from "motion/react";
import { Sortable, type DragProps } from "../../features/dashboard/Sortable";
import { isRtl, t } from "../../i18n";
import { rovingKeys } from "../a11y";
import { Icon } from "../Icon";
import { IconSwap, Morph, prefersReducedMotion, SPRING, staggerDelay, Swap, TWEEN, useLiquid, useReducedMotionFlag } from "../motion";
import type { Box } from "../motion/liquid";
import type { IslandGroup, IslandLeaf, IslandModel } from "./model";

/** Inflated once per page life (a remount, a StrictMode double effect, a lock and unlock don't). */
let inflated = false;

/** The island: RoALT's one living control, in the title bar. Places are tiles; the current one is a
 * solid tile wearing its label (its width springs). A gray ghost glides under the pointer. A group
 * opens in place as an inset pill (✕, then its children); picking a child folds it back and the
 * group's tile turns into the child. `live` capsules (launch progress, running clients) sit at the
 * end. One Tab stop: arrows move, Alt and the arrows reorder, Esc folds an open group. */
export function Island({ model, live, inflate = true }: { model: IslandModel; live?: ReactNode; inflate?: boolean }) {
  const [open, setOpen] = useState<string | null>(null);
  const nav = useRef<HTMLElement>(null);
  const row = useRef<HTMLDivElement>(null);
  const reduced = useReducedMotionFlag();
  const [first] = useState(() => inflate && !inflated);

  // A new place on screen folds any open group, as does a press outside the island.
  const current = [...model.main, ...model.groups.flatMap((g) => g.children)].find((l) => l.current)?.id;
  useEffect(() => setOpen(null), [current]);
  useEffect(() => {
    if (!open) return;
    const down = (e: PointerEvent) => {
      if (!nav.current?.contains(e.target as Node)) setOpen(null);
    };
    document.addEventListener("pointerdown", down, true);
    return () => document.removeEventListener("pointerdown", down, true);
  }, [open]);

  const focusGroup = (id: string) => requestAnimationFrame(() => nav.current?.querySelector<HTMLElement>(`[data-group-id="${id}"]`)?.focus());
  const close = (refocus: boolean) => {
    const was = open;
    setOpen(null);
    if (refocus && was) focusGroup(was);
  };

  const keys = (e: KeyboardEvent<HTMLElement>) => {
    if (e.key === "Escape" && open) {
      e.stopPropagation();
      close(true);
      return;
    }
    // Keys inside a capsule's open sheet (a DOM child of the island) are the sheet's own.
    if ((e.target as HTMLElement).matches("[data-rove]")) rovingKeys(e, "[data-rove]");
  };
  // One Tab stop: the tile last focused, the current place again after each navigation, and a new
  // one when the tile holding it goes away (a capsule ending, a group folding). Kept by hand: every
  // tile renders tabIndex -1, and React only writes tabIndex when that prop changes.
  const tiles = () => [...(nav.current?.querySelectorAll<HTMLElement>("[data-rove]") ?? [])];
  const stopOn = (stop: HTMLElement | undefined) => {
    for (const el of tiles()) {
      const want = el === stop ? 0 : -1;
      if (el.tabIndex !== want) el.tabIndex = want;
    }
  };
  const home = () => tiles().find((el) => el.matches("[aria-current=page]")) ?? tiles()[0];
  useLayoutEffect(() => {
    if (!nav.current?.contains(document.activeElement)) stopOn(home());
  }, [current]);
  useEffect(() => {
    const root = nav.current;
    if (!root) return;
    // Whether the focus was last in the island (a tile, a capsule's sheet). Removing the focused
    // element moves no focus, so this outlives a capsule that shrinks away with the focus on it.
    let inside = false;
    const track = (e: globalThis.FocusEvent) => void (inside = root.contains(e.target as Node));
    // A real blur (a click elsewhere) leaves the element on the page; a removal does not.
    const left = (e: globalThis.FocusEvent) => {
      const from = e.target as Node;
      if (root.contains(from)) queueMicrotask(() => from.isConnected && (inside = false));
    };
    document.addEventListener("focusin", track);
    document.addEventListener("focusout", left);
    const repair = new MutationObserver(() => {
      if (!tiles().some((el) => el.tabIndex === 0)) stopOn(home());
      // The focused tile went away: the focus goes to the island's Tab stop, not to nowhere.
      if (inside && document.activeElement === document.body) tiles().find((el) => el.tabIndex === 0)?.focus();
    });
    repair.observe(root, { childList: true, subtree: true });
    return () => {
      repair.disconnect();
      document.removeEventListener("focusin", track);
      document.removeEventListener("focusout", left);
    };
  }, []);
  const rove = (e: FocusEvent<HTMLElement>) => {
    const target = e.target as HTMLElement;
    if (target.matches("[data-rove]")) stopOn(target);
  };

  return (
    <motion.nav
      ref={nav}
      className="island"
      aria-label={model.label}
      // The title bar is left to right; the island runs in the app's language (arrows, reordering).
      dir={isRtl() ? "rtl" : "ltr"}
      data-tauri-drag-region="false"
      data-group-open={open ? "" : undefined}
      initial={first ? { opacity: 0, scale: reduced ? 1 : 0.4, filter: reduced ? "blur(0px)" : "blur(8px)" } : false}
      animate={{ opacity: 1, scale: 1, filter: "blur(0px)" }}
      transition={first ? { ...SPRING.inflate, opacity: { duration: reduced ? 0.16 : 0.24 } } : undefined}
      onAnimationComplete={() => void (inflated = true)}
      onKeyDown={keys}
      onFocus={rove}
    >
      <div ref={row} className="island-row">
        <Drops row={row} nav={nav} />
        <Sortable
          ids={model.main.map((l) => l.id)}
          order={model.mainOrder.ids}
          onOrder={model.mainOrder.onOrder}
          name={(id) => model.main.find((l) => l.id === id)?.label ?? id}
          render={(id, drag) => {
            const leaf = model.main.find((l) => l.id === id)!;
            const i = model.main.indexOf(leaf);
            return (
              <span key={id} className="island-slot">
                <Item leaf={leaf} drag={drag} enter={first ? i : null} />
              </span>
            );
          }}
        />
        {model.groups.map((g, i) => (
          <Group
            key={g.id}
            group={g}
            open={open === g.id}
            enter={first ? model.main.length + i : null}
            onOpen={() => setOpen(g.id)}
            onClose={close}
          />
        ))}
        {live}
      </div>
    </motion.nav>
  );
}

/** The label of a tile, revealed while it is current: width and opacity spring together. */
function Label({ shown, children }: { shown: boolean; children: ReactNode }) {
  const reduced = useReducedMotionFlag();
  return (
    <motion.span
      aria-hidden="true"
      initial={false}
      animate={{ width: shown ? "auto" : 0, opacity: shown ? 1 : 0 }}
      transition={{ width: reduced ? { duration: 0 } : SPRING.size, opacity: shown ? { ...TWEEN.fade, delay: 0.04 } : TWEEN.exit }}
      style={{ display: "inline-flex", overflow: "clip", overflowClipMargin: 2 }}
    >
      <span className="island-label ps-2">{children}</span>
    </motion.span>
  );
}

/** A tile entering as the island inflates. */
const enterProps = (i: number | null) =>
  i === null
    ? {}
    : {
        initial: { opacity: 0, scale: 0.6 },
        animate: { opacity: 1, scale: 1, transition: { ...SPRING.snappy, delay: 0.12 + staggerDelay(i) } },
      };

/** A tile's name with the numbers it shows (its count, its badge): those are aria-hidden. */
const named = (leaf: IslandLeaf) => [leaf.label, leaf.count, leaf.badge || undefined].filter((x) => x !== undefined).join(", ");

function Item({ leaf, drag, enter }: { leaf: IslandLeaf; drag: DragProps; enter: number | null }) {
  return (
    <motion.button
      type="button"
      className="island-item"
      data-rove
      data-ghost
      aria-current={leaf.current ? "page" : undefined}
      aria-label={named(leaf)}
      data-tip={leaf.current ? undefined : leaf.label}
      data-tip-kbd={leaf.current ? undefined : leaf.shortcut}
      tabIndex={-1}
      onClick={leaf.onSelect}
      {...enterProps(enter)}
      {...(drag as object)}
    >
      <Icon name={leaf.icon} />
      <Label shown={leaf.current}>
        {leaf.label}
        {leaf.count !== undefined && <span className="island-count ms-1.5">{leaf.count}</span>}
      </Label>
      {!!leaf.badge && <span className="island-badge" aria-hidden="true">{leaf.badge}</span>}
    </motion.button>
  );
}

function Group({ group, open, enter, onOpen, onClose }: {
  group: IslandGroup;
  open: boolean;
  enter: number | null;
  onOpen: () => void;
  onClose: (refocus: boolean) => void;
}) {
  const pillId = useId();
  const pill = useRef<HTMLDivElement>(null);
  const current = group.children.find((c) => c.current);
  const icon = group.keepIcon ? group.icon : (current?.icon ?? group.icon);

  // Opened: the focus goes to the current child, else the first.
  useEffect(() => {
    if (!open) return;
    const id = requestAnimationFrame(() => {
      const el = pill.current;
      (el?.querySelector<HTMLElement>("[aria-current=page]") ?? el?.querySelector<HTMLElement>("[data-child]"))?.focus();
    });
    return () => cancelAnimationFrame(id);
  }, [open]);

  const children = (
    <div ref={pill} id={pillId} role="group" aria-label={group.label} className="island-group">
      <button type="button" className="island-child" data-close data-rove tabIndex={-1} aria-label={t("common.close")} data-tip={t("common.close")} onClick={() => onClose(true)}>
        <Icon name="x" size={14} />
      </button>
      {group.order ? (
        <Sortable
          ids={group.children.map((c) => c.id)}
          order={group.order.ids}
          onOrder={group.order.onOrder}
          name={(id) => group.children.find((c) => c.id === id)?.label ?? id}
          className="contents"
          render={(id, drag) => {
            const c = group.children.find((x) => x.id === id)!;
            return (
              <span key={id} className="flex">
                <Child leaf={c} i={group.children.indexOf(c)} drag={drag} onPick={() => onClose(false)} />
              </span>
            );
          }}
        />
      ) : (
        group.children.map((c, i) => <Child key={c.id} leaf={c} i={i} onPick={() => onClose(false)} />)
      )}
    </div>
  );

  return (
    <motion.span className="island-slot relative" {...enterProps(enter)}>
      <Morph axis="width">
        <Swap k={open ? "open" : "closed"} variant="fade">
          {open ? (
            children
          ) : (
            <button
              type="button"
              className="island-item"
              data-group
              data-group-id={group.id}
              data-rove
              data-ghost
              aria-expanded={false}
              aria-current={current ? "page" : undefined}
              aria-label={current && !group.keepIcon ? `${group.label}: ${current.label}` : group.label}
              aria-description={current && group.keepIcon ? current.label : undefined}
              data-tip={current ? undefined : group.label}
              tabIndex={-1}
              onClick={onOpen}
              onKeyDown={(e) => {
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  onOpen();
                }
              }}
            >
              <IconSwap name={icon} />
              <Label shown={!!current}>{current?.label}</Label>
            </button>
          )}
        </Swap>
      </Morph>
    </motion.span>
  );
}

function Child({ leaf, i, drag, onPick }: { leaf: IslandLeaf; i: number; drag?: DragProps; onPick: () => void }) {
  return (
    <motion.button
      type="button"
      className="island-child"
      data-child
      data-rove
      tabIndex={-1}
      aria-current={leaf.current ? "page" : undefined}
      aria-label={named(leaf)}
      data-tip={leaf.label}
      data-tip-kbd={leaf.shortcut}
      initial={{ opacity: 0, scale: 0.7 }}
      animate={{ opacity: 1, scale: 1, transition: { ...SPRING.snappy, delay: staggerDelay(i + 1) } }}
      onClick={() => {
        onPick();
        leaf.onSelect();
      }}
      {...(drag as object)}
    >
      <Icon name={leaf.icon} />
      {!!leaf.badge && <span className="island-badge" aria-hidden="true">{leaf.badge}</span>}
    </motion.button>
  );
}

/** The island's liquid layer, under its tiles: the current place's light (a drop that flows from tile
 * to tile, `useLiquid`), the gray hover drop, and an open group's pill. A goo filter melts them
 * together where they touch; an open group that touches the light turns light too (data-linked). */
function Drops({ row, nav }: { row: RefObject<HTMLDivElement | null>; nav: RefObject<HTMLElement | null> }) {
  const drop = useRef<HTMLSpanElement>(null);
  const group = useRef<HTMLSpanElement>(null);
  const layer = useRef<HTMLSpanElement>(null);
  const [rowOf] = useState(() => ({ get current() { return layer.current?.parentElement ?? null; } }));
  const boxes = useRef<{ drop: Box | null; group: Box | null }>({ drop: null, group: null });
  const [link] = useState(() => (which: "drop" | "group") => (box: Box | null) => {
    boxes.current[which] = box;
    const { drop: d, group: g } = boxes.current;
    // Side by side (not the light inside the group, on a child), with no more than a hair between.
    const gap = d && g ? Math.max(g.l - d.r, d.l - g.r) : Infinity;
    nav.current?.toggleAttribute("data-linked", gap > -1 && gap < 10);
  });
  // The row is the parent of this layer's parent: measured and searched through the DOM (see useLiquid).
  useLiquid(rowOf, drop, ".island-item[aria-current=page], .island-child[aria-current=page]", link("drop"));
  useLiquid(rowOf, group, ".island-group", link("group"));
  return (
    <span ref={layer} className="island-drops" aria-hidden="true">
      <svg width="0" height="0" className="absolute">
        <filter id="island-goo">
          <feGaussianBlur in="SourceGraphic" stdDeviation="4" result="b" />
          <feColorMatrix in="b" mode="matrix" values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 20 -9" result="g" />
          <feComposite in="SourceGraphic" in2="g" operator="atop" />
        </filter>
      </svg>
      <HoverGhost row={row} />
      <span ref={group} className="island-group-drop lq-drop" />
      <span ref={drop} className="island-drop lq-drop" />
    </span>
  );
}

/** One gray tile that glides to whatever top-level tile the pointer (or keyboard focus) is on. */
function HoverGhost({ row }: { row: RefObject<HTMLDivElement | null> }) {
  const x = useMotionValue(0);
  const width = useMotionValue(36);
  const opacity = useMotionValue(0);
  useEffect(() => {
    const el = row.current;
    if (!el) return;
    let on: HTMLElement | null = null;
    let shown = false;
    let leaving = 0;
    const place = (target: HTMLElement, jump: boolean) => {
      const r = el.getBoundingClientRect();
      const b = target.getBoundingClientRect();
      const [nx, nw] = [b.left - r.left, b.width];
      if (jump || !shown || prefersReducedMotion()) {
        x.set(nx);
        width.set(nw);
      } else {
        animate(x, nx, SPRING.snappy);
        animate(width, nw, SPRING.snappy);
      }
    };
    const show = (target: HTMLElement) => {
      if (el.querySelector(".tile-drag")) return;
      clearTimeout(leaving);
      place(target, false);
      on = target;
      if (!shown) {
        shown = true;
        animate(opacity, 1, TWEEN.fade);
      }
    };
    const hide = () => {
      clearTimeout(leaving);
      leaving = window.setTimeout(() => {
        shown = false;
        on = null;
        animate(opacity, 0, TWEEN.exit);
      }, 60);
    };
    const ghostOf = (n: EventTarget | null) => (n instanceof Element ? n.closest<HTMLElement>("[data-ghost]") : null);
    const over = (e: PointerEvent) => {
      const g = ghostOf(e.target);
      if (g && el.contains(g) && g !== on) show(g);
    };
    const out = (e: PointerEvent) => {
      const to = ghostOf(e.relatedTarget);
      if (!to || !el.contains(to)) hide();
    };
    const focus = (e: globalThis.FocusEvent) => {
      const g = ghostOf(e.target);
      if (g?.matches(":focus-visible")) show(g);
    };
    el.addEventListener("pointerover", over);
    el.addEventListener("pointerout", out);
    el.addEventListener("focusin", focus);
    el.addEventListener("focusout", hide);
    // The island resizes as labels and groups spring: the ghost stays under its tile.
    const ro = new ResizeObserver(() => on?.isConnected && place(on, true));
    ro.observe(el);
    return () => {
      clearTimeout(leaving);
      el.removeEventListener("pointerover", over);
      el.removeEventListener("pointerout", out);
      el.removeEventListener("focusin", focus);
      el.removeEventListener("focusout", hide);
      ro.disconnect();
    };
  }, [row, x, width, opacity]);
  return <motion.span aria-hidden="true" className="island-ghost" style={{ x, width, opacity }} />;
}
