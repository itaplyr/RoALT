import { memo, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent, type RefObject } from "react";
import { flushSync } from "react-dom";
import type { AccountSummary, Group, Instance, Presence } from "../../api";
import { groupColor, useUpdateAccount } from "../../data";
import { t } from "../../i18n";
import { announce } from "../../ui/a11y";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import { riseIn, STAGGER } from "../../ui/motion";
import { accountName, ago, Avatar, MainBadge, Status } from "./shared";

/** What a row does. The rows read it when they act, so they can stay memoized while it changes. */
type RowActions = {
  open: (id: number) => void;
  tick: (id: number, range: boolean) => void;
  menu: (id: number, x: number, y: number, from: HTMLElement) => void;
  launch: (id: number) => void;
  favourite: (a: AccountSummary) => void;
  gripDown: (e: PointerEvent<HTMLButtonElement>, id: number) => void;
  gripMove: (e: PointerEvent) => void;
  gripEnd: (cancel: boolean) => void;
  gripKey: (e: KeyboardEvent, id: number) => void;
  /** Arrow keys on a row (not its grip) move to the next or previous row's name. */
  rowKey: (e: KeyboardEvent, id: number) => void;
  /** The focus came into this row: it becomes the list's one Tab stop. */
  focused: (id: number) => void;
};

/** Long lists render only the rows in view (plus OVERSCAN each side); spacer rows keep the height.
 * shortcut: a row dragged and then scrolled far out of the window unmounts and drops the drag; keep it
 * mounted if drags over long distances get asked for. */
const WINDOW_FROM = 120;
const OVERSCAN = 12;

/** The accounts list. Its columns follow the table's own width (the `acc-table` container). Rows glide
 * to a new order (FLIP) and new ones rise in, on WAAPI and never with motion components: the drag sets
 * the row's transform inline. */
export function AccountsTable({ rows, ordered, groupById, presence, compact, now, currentId, checked, all, some, live, isChecking, onTick, onTickAll, onOpen, onMenu, onLaunch, menuId, onReorder, onClearFilters }: {
  /** What the filters show, in list order. */
  rows: AccountSummary[];
  /** Every account in list order: a drop lands among them. */
  ordered: AccountSummary[];
  groupById: Map<number, Group>;
  presence: Map<number, Presence> | undefined;
  compact: boolean;
  now: number;
  currentId: number | undefined;
  checked: Set<number>;
  /** The header checkbox: every pickable row ticked, some of them. */
  all: boolean;
  some: boolean;
  live: Map<number, Instance>;
  isChecking: (id: number) => boolean;
  /** `range`: Shift held, so everything from the last ticked row follows. */
  onTick: (id: number, range: boolean) => void;
  onTickAll: () => void;
  onOpen: (id: number) => void;
  onMenu: (id: number, x: number, y: number, from: HTMLElement) => void;
  onLaunch: (id: number) => void;
  /** The row whose "…" menu is open. */
  menuId?: number;
  onReorder: (ids: number[]) => void;
  onClearFilters: () => void;
}) {
  const update = useUpdateAccount();
  const scroller = useRef<HTMLDivElement>(null);
  const tbody = useRef<HTMLTableSectionElement>(null);
  const windowed = rows.length > WINDOW_FROM;
  const rowH = useRef(compact ? 44 : 56);
  const [view, setView] = useState({ top: 0, height: 900 });
  const first = windowed ? Math.max(0, Math.floor(view.top / rowH.current) - OVERSCAN) : 0;
  const last = windowed ? Math.min(rows.length, Math.ceil((view.top + view.height) / rowH.current) + OVERSCAN) : rows.length;
  const firstRef = useRef(first);
  firstRef.current = first;
  useEffect(() => {
    const el = scroller.current;
    if (!el || !windowed) return;
    const read = () => setView({ top: el.scrollTop, height: el.clientHeight });
    read();
    el.addEventListener("scroll", read, { passive: true });
    const ro = new ResizeObserver(read);
    ro.observe(el);
    return () => {
      el.removeEventListener("scroll", read);
      ro.disconnect();
    };
  }, [windowed]);
  // The spacers use a measured row height (rows grow with the density and wrapped group tags).
  useLayoutEffect(() => {
    const r = tbody.current?.querySelector<HTMLElement>("tr[data-id]");
    if (r && r.offsetHeight) rowH.current = r.offsetHeight;
  });
  /** The rendered rows only: no spacers. Their index in `rows` is firstRef + i. */
  const rowEls = () => [...(tbody.current?.rows ?? [])].filter((r) => r.dataset.id);

  // The list is one Tab stop: the row the focus was last in, else the open one, else the first shown.
  const [focusId, setFocusId] = useState<number | null>(null);
  const shownSlice = rows.slice(first, last);
  const tabId = [focusId, currentId].find((id) => id != null && shownSlice.some((a) => a.id === id)) ?? shownSlice[0]?.id;

  /** Focuses a row's name, bringing it into the window first when it is out of it. */
  function focusName(i: number) {
    const a = rows[i];
    if (!a) return;
    setFocusId(a.id);
    const el = document.querySelector<HTMLElement>(`[data-name="${a.id}"]`);
    if (el) return el.focus();
    const s = scroller.current;
    if (!s) return;
    s.scrollTop = Math.max(0, i * rowH.current - s.clientHeight / 2);
    flushSync(() => setView({ top: s.scrollTop, height: s.clientHeight }));
    document.querySelector<HTMLElement>(`[data-name="${a.id}"]`)?.focus();
  }
  const [drag, setDrag] = useState<{ id: number; over: number } | null>(null);
  const dragging = useRef<{ id: number; startY: number; row: HTMLElement; over: number } | null>(null);
  const flip = useRef<Map<number, number> | null>(null);
  const seen = useRef(new Set<number>());

  function snapshot() {
    const m = new Map<number, number>();
    for (const r of rowEls()) m.set(Number(r.dataset.id), r.getBoundingClientRect().top);
    flip.current = m;
  }

  /** Moves `id` so it sits before rows[over] (or after the last row shown). */
  function dropAt(id: number, over: number) {
    const from = rows.findIndex((a) => a.id === id);
    if (from < 0 || over < 0 || over > rows.length || over === from || over === from + 1) return false;
    // The MAIN is pinned first: it doesn't move, and nothing goes above it.
    if (rows[from].main || (over === 0 && rows[0]?.main)) return false;
    const ids = ordered.map((a) => a.id).filter((x) => x !== id);
    const at = over < rows.length ? ids.indexOf(rows[over].id) : ids.indexOf(rows[rows.length - 1].id) + 1;
    ids.splice(at, 0, id);
    snapshot();
    onReorder(ids);
    return true;
  }

  // FLIP: rows glide from where they were to where the new order puts them.
  useLayoutEffect(() => {
    const before = flip.current;
    const list = rowEls();
    if (!before || !list.length) return;
    flip.current = null;
    for (const r of list) {
      r.style.transition = "none";
      r.style.transform = "";
    }
    const deltas = list.map((r) => (before.get(Number(r.dataset.id)) ?? r.getBoundingClientRect().top) - r.getBoundingClientRect().top);
    // Many rows moving at once just take their places, as a long list arriving does.
    if (deltas.filter((d) => Math.abs(d) >= 1).length <= STAGGER.listLimit) {
      list.forEach((r, i) => Math.abs(deltas[i]) >= 1 && (r.style.transform = `translateY(${deltas[i]}px)`));
    }
    void tbody.current?.offsetHeight;
    for (const r of list) {
      r.style.transition = "";
      r.style.transform = "";
    }
  });

  // New rows rise in: WAAPI, as a CSS animation would restart whenever React moves the row.
  // New means new in `rows`, not newly in the window: a scroll brings no row "in".
  useLayoutEffect(() => {
    const fresh = rows.filter((a) => !seen.current.has(a.id));
    seen.current = new Set(rows.map((a) => a.id));
    // A whole list coming back at once is no new row, and would animate hundreds of them.
    if (fresh.length > STAGGER.listLimit) return;
    fresh.forEach((a, i) => {
      const r = tbody.current?.querySelector<HTMLElement>(`tr[data-id="${a.id}"]`);
      if (r) riseIn(r, i);
    });
  });

  function gripDown(e: PointerEvent<HTMLButtonElement>, id: number) {
    const row = e.currentTarget.closest("tr");
    if (e.button !== 0 || !row) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    const from = rows.findIndex((a) => a.id === id);
    row.style.transition = "none";
    dragging.current = { id, startY: e.clientY, row, over: from };
    setDrag({ id, over: from });
  }

  function gripMove(e: PointerEvent) {
    const d = dragging.current;
    if (!d || !tbody.current) return;
    d.row.style.transform = `translateY(${e.clientY - d.startY}px)`;
    const list = rowEls();
    // Indexes into `rows`: the window starts at firstRef.
    let over = firstRef.current + list.length;
    for (let i = 0; i < list.length; i++) {
      if (list[i] === d.row) continue;
      const r = list[i].getBoundingClientRect();
      if (e.clientY < r.top + r.height / 2) {
        over = firstRef.current + i;
        break;
      }
    }
    if (over !== d.over) {
      d.over = over;
      setDrag({ id: d.id, over });
    }
  }

  function gripEnd(cancel: boolean) {
    const d = dragging.current;
    if (!d) return;
    dragging.current = null;
    setDrag(null);
    if (cancel || !dropAt(d.id, d.over)) snapshot(); // glide back home
  }

  function gripKey(e: KeyboardEvent, id: number) {
    const dir = e.key === "ArrowUp" ? -1 : e.key === "ArrowDown" ? 1 : 0;
    if (!dir) return;
    e.preventDefault();
    const i = rows.findIndex((a) => a.id === id);
    if (dropAt(id, dir < 0 ? i - 1 : i + 2)) {
      announce(`${accountName(rows[i])}: ${i + 1 + dir} / ${rows.length}`);
      requestAnimationFrame(() => document.querySelector<HTMLElement>(`[data-grip="${id}"]`)?.focus());
    }
  }

  const act = useRef<RowActions>(null!);
  act.current = {
    open: onOpen,
    tick: onTick,
    menu: onMenu,
    launch: onLaunch,
    favourite: (a) => update.mutate({ id: a.id, patch: { favourite: !a.favourite } }),
    gripDown,
    gripMove,
    gripEnd,
    gripKey,
    rowKey: (e, id) => {
      const dir = e.key === "ArrowUp" ? -1 : e.key === "ArrowDown" ? 1 : 0;
      if (!dir || e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey) return;
      e.preventDefault();
      focusName(rows.findIndex((a) => a.id === id) + dir);
    },
    focused: setFocusId,
  };

  const from = drag ? rows.findIndex((a) => a.id === drag.id) : -1;
  const dropLine = drag && drag.over !== from && drag.over !== from + 1 ? drag.over : -1;
  const swap = rows.length <= STAGGER.listLimit;

  return (
    <div ref={scroller} className="acc-table scroll flex-1 min-h-0">
      <table className="w-full border-separate border-spacing-0" aria-label={t("accounts.title")} aria-rowcount={windowed ? rows.length + 1 : undefined}>
        <thead>
          <tr>
            {/* A miss beside the checkbox still ticks: the whole cell is its target. */}
            <th className="acc-c-pick" onClick={(e) => e.target === e.currentTarget || (e.target as Element).matches(".acc-pick") ? onTickAll() : undefined}>
              <span className="acc-pick">
                <span aria-hidden="true" className="w-[18px] flex-none" />
                <input
                  type="checkbox"
                  className="cb"
                  aria-label={t("accounts.selectAll")}
                  checked={all}
                  ref={(el) => {
                    if (el) el.indeterminate = some && !all;
                  }}
                  onChange={onTickAll}
                />
              </span>
            </th>
            <th>{t("accounts.colAccount")}</th>
            <th className="acc-c-status">{t("accounts.colStatus")}</th>
            <th className="acc-c-groups">{t("accounts.groups.title")}</th>
            <th className="acc-c-act">
              <span className="sr-only">{t("launch.running.actions")}</span>
            </th>
          </tr>
        </thead>
        <tbody ref={tbody}>
          {first > 0 && <Spacer height={first * rowH.current} />}
          {shownSlice.map((a, j) => (
            <AccountRow
              key={a.id}
              index={windowed ? first + j + 2 : undefined}
              tabbable={tabId === a.id}
              menuOpen={menuId === a.id}
              a={a}
              p={presence?.get(a.id)}
              groupById={groupById}
              compact={compact}
              checkedAgo={!compact && a.statusCheckedAt ? ago(a.statusCheckedAt, now) : undefined}
              current={currentId === a.id}
              checked={checked.has(a.id)}
              checking={isChecking(a.id)}
              playable={a.status === "valid" && !live.has(a.id)}
              swap={swap}
              drag={drag?.id === a.id}
              drop={dropLine === first + j ? "top" : dropLine === rows.length && first + j === rows.length - 1 ? "bottom" : undefined}
              act={act}
            />
          ))}
          {last < rows.length && <Spacer height={(rows.length - last) * rowH.current} />}
        </tbody>
      </table>
      {rows.length === 0 && (
        <div className="empty enter m-4">
          {t("accounts.noMatch")}
          <Button size="md" onClick={onClearFilters}>
            {t("accounts.clearFilters")}
          </Button>
        </div>
      )}
    </div>
  );
}

/** Stands in for the rows out of the window. */
const Spacer = ({ height }: { height: number }) => (
  <tr aria-hidden="true">
    <td colSpan={5} className="p-0" style={{ height }} />
  </tr>
);

/** One account's row. Memoized on what it shows: a tick, an open or a presence update re-renders the
 * rows it changes, not all 300. */
const AccountRow = memo(function AccountRow({ index, tabbable, menuOpen, a, p, groupById, compact, checkedAgo, current, checked, checking, playable, swap, drag, drop, act }: {
  /** aria-rowindex, while the list is windowed. */
  index: number | undefined;
  /** Its controls are the list's Tab stop; the other rows' are reached with the arrow keys. */
  tabbable: boolean;
  menuOpen: boolean;
  a: AccountSummary;
  p: Presence | undefined;
  groupById: Map<number, Group>;
  compact: boolean;
  /** "3 min ago" beside its status (Detailed density). */
  checkedAgo: string | undefined;
  /** Its details are the ones open. */
  current: boolean;
  checked: boolean;
  checking: boolean;
  playable: boolean;
  /** A check's result pops in (short lists only). */
  swap: boolean;
  drag: boolean;
  /** Where the dragged row would land: above or below this one. */
  drop: "top" | "bottom" | undefined;
  act: RefObject<RowActions>;
}) {
  const name = accountName(a);
  const cls = ["row", drag && "acc-row-drag", drop && `acc-drop-${drop}`];
  const tab = tabbable ? 0 : -1;
  return (
    <tr
      data-id={a.id}
      data-selected={current || undefined}
      aria-current={current || undefined}
      aria-rowindex={index}
      className={cls.filter(Boolean).join(" ")}
      onClick={() => act.current.open(a.id)}
      onFocus={() => act.current.focused(a.id)}
      onKeyDown={(e) => act.current.rowKey(e, a.id)}
      onContextMenu={(e) => {
        e.preventDefault();
        act.current.menu(a.id, e.clientX, e.clientY, e.currentTarget.querySelector<HTMLElement>("[data-more]") ?? e.currentTarget);
      }}
    >
      {/* A miss beside the grip or the checkbox ticks the row, rather than opening it. */}
      <td
        onClick={(e) => {
          // The grip and the checkbox keep their own clicks: whatever reaches here missed them.
          e.stopPropagation();
          act.current.tick(a.id, e.shiftKey);
        }}
      >
        <span className="acc-pick">
          {a.main ? (
            // Pinned first: it can't be dragged, and says why on hover.
            <span aria-hidden="true" className="grip acc-pin" data-tip={t("accounts.detail.mainHint")}>
              <Icon name="lock" size={12} />
            </span>
          ) : (
            <button
              type="button"
              className="grip"
              data-grip={a.id}
              tabIndex={tab}
              aria-label={t("accounts.reorder", { name })}
              onPointerDown={(e) => act.current.gripDown(e, a.id)}
              onPointerMove={(e) => act.current.gripMove(e)}
              onPointerUp={() => act.current.gripEnd(false)}
              onPointerCancel={() => act.current.gripEnd(true)}
              onKeyDown={(e) => act.current.gripKey(e, a.id)}
              onClick={(e) => e.stopPropagation()}
            >
              <Icon name="grip" size={14} />
            </button>
          )}
          <input
            type="checkbox"
            className="cb"
            tabIndex={tab}
            aria-label={t("accounts.select", { name })}
            checked={checked}
            onChange={(e) => act.current.tick(a.id, (e.nativeEvent as globalThis.MouseEvent).shiftKey)}
            onClick={(e) => e.stopPropagation()}
          />
        </span>
      </td>
      <td className="pe-3 max-w-0">
        <div className="flex items-center gap-2.5 min-w-0">
          <Avatar account={a} size={compact ? 22 : 28} />
          <div className={compact ? "flex items-baseline gap-2 min-w-0" : "flex flex-col min-w-0 leading-tight"}>
            <span className="flex items-center gap-1.5 min-w-0">
              <button type="button" className="linkbtn font-semibold truncate" data-name={a.id} tabIndex={tab}>
                {name}
              </button>
              {a.main && <MainBadge />}
            </span>
            <span className="acc-handle mono text-xs text-fg-3 truncate">@{a.username}</span>
            {/* No room for the Status column. */}
            <span className="acc-inline-status min-w-0 text-xs">
              <Status status={a.status} presence={p} checking={checking} swap={swap} />
            </span>
          </div>
        </div>
      </td>
      <td className="acc-c-status pe-3 max-w-0">
        <Status status={a.status} presence={p} checking={checking} detail={checkedAgo} swap={swap} />
      </td>
      <td className="acc-c-groups pe-3">
        <div className="flex flex-wrap gap-1">
          {a.groupIds.flatMap((gid) => groupById.get(gid) ?? []).map((g) => (
            <span key={g.id} className="tag">
              <span aria-hidden="true" className="w-1.5 h-1.5 rounded-full" style={{ background: groupColor(g) }} />
              {g.name}
            </span>
          ))}
        </div>
      </td>
      <td className="pe-2">
        <span className="flex items-center justify-end gap-0.5">
          <button
            type="button"
            className="wc acc-star"
            tabIndex={tab}
            aria-label={t("accounts.favouriteName", { name })}
            aria-pressed={a.favourite}
            onClick={(e) => {
              e.stopPropagation();
              act.current.favourite(a);
            }}
          >
            <Icon name="star" size={14} fill={a.favourite} />
          </button>
          <button
            type="button"
            className="wc"
            data-more
            tabIndex={tab}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            aria-label={t("accounts.menu.actionsFor", { name })}
            data-tip={t("accounts.menu.actionsFor", { name })}
            onClick={(e) => {
              e.stopPropagation();
              const r = e.currentTarget.getBoundingClientRect();
              act.current.menu(a.id, r.left, r.bottom + 4, e.currentTarget);
            }}
          >
            <Icon name="dots" size={16} />
          </button>
          {playable ? (
            <button
              type="button"
              className="btn btn-xs btn-icon"
              tabIndex={tab}
              aria-label={t("accounts.playWith", { name })}
              data-tip={t("accounts.playWith", { name })}
              onClick={(e) => {
                e.stopPropagation();
                act.current.launch(a.id);
              }}
            >
              <Icon name="play" size={12} fill />
            </button>
          ) : (
            <span aria-hidden="true" className="w-7 flex-none" />
          )}
        </span>
      </td>
    </tr>
  );
});
