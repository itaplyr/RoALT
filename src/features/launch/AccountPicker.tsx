import { memo, useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import type { AccountSummary } from "../../api";
import { useGroups, useInstances, usePresence, useSettings } from "../../data";
import { isRtl, t } from "../../i18n";
import { rovingKeys } from "../../ui/a11y";
import { Chip } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Field";
import { Icon } from "../../ui/Icon";
import { Collapse, riseIn, STAGGER, useFlip, useFreshItems } from "../../ui/motion";
import { accountName, avatarColor, Avatar, MainBadge, Status } from "../accounts/shared";
import { isLive } from "./Running";
import "./launch.css";

const matches = (a: AccountSummary, needle: string) => `${a.alias}\n${a.displayName}\n@${a.username}\n${a.userId}`.toLowerCase().includes(needle);

/** Accounts whose client is on its way or in a game: they can't start another. */
function useLive() {
  const instances = useInstances().data;
  return useMemo(() => new Set((instances ?? []).filter(isLive).map((i) => i.accountId)), [instances]);
}

function Search({ value, onChange, find }: { value: string; onChange: (v: string) => void; find?: boolean }) {
  return (
    <Input
      leading="search"
      aria-label={t("accounts.filterLabel")}
      placeholder={t("accounts.filterPlaceholder")}
      autoComplete="off"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      data-find={find || undefined}
    />
  );
}

type Picks = { accounts: AccountSummary[]; picked: number[]; onPicked: (ids: number[]) => void; label: string };

/** One of Play's tiles. Memoised on primitive props, so a pick or an instances poll re-renders only the
 * tiles that changed. */
const Tile = memo(function Tile({ account: a, on, can, live, focusable, presence, i, flip, onToggle, onFocusId }: {
  account: AccountSummary;
  on: boolean;
  can: boolean;
  live: boolean;
  focusable: boolean;
  presence: Parameters<typeof Status>[0]["presence"];
  i: number;
  flip: boolean;
  onToggle: (id: number, range: boolean) => void;
  onFocusId: (id: number) => void;
}) {
  return (
    <button
      type="button"
      data-tile
      data-id={a.id}
      data-flip={flip || undefined}
      className="btn pick launch-tile"
      aria-pressed={on}
      aria-disabled={!can || undefined}
      tabIndex={focusable ? 0 : -1}
      style={{ "--i": Math.min(i, 12) } as CSSProperties}
      onFocus={() => onFocusId(a.id)}
      // The second click of a double-click would undo the first.
      onClick={(e) => e.detail < 2 && onToggle(a.id, e.shiftKey)}
    >
      <span aria-hidden="true" className="tick"><Icon name="check" size={14} /></span>
      <Avatar account={a} size={36} />
      <span className="flex-1 min-w-0 flex flex-col">
        <span className="flex items-center gap-1.5 min-w-0">
          <span className="truncate">{accountName(a)}</span>
          {a.main && <MainBadge />}
        </span>
        <span className="flex text-[13px] font-normal text-fg-2 min-w-0">
          {live ? t("play.already") : <Status status={a.status} presence={presence} />}
        </span>
      </span>
    </button>
  );
});

/** Tiles rendered at first, and added each time the end comes near: thousands at once would freeze Play. */
const PAGE = 120;

/** Play's grid. Shift picks a range (click or Space), Ctrl+A and Select all take what's shown (the MAIN
 * account left out while Settings protects it, and the note says so). One Tab stop; arrows move. */
function Tiles({ accounts, picked, onPicked, label, onAdd, onAccounts, className = "" }: Picks & { onAdd: () => void; onAccounts: () => void; className?: string }) {
  const groups = useGroups().data ?? [];
  const protect = useSettings().data?.protectMain !== false;
  const live = useLive();
  const [q, setQ] = useState("");
  const [group, setGroup] = useState<number | null>(null);
  const [focusId, setFocusId] = useState<number | null>(null);
  const [cascading, setCascading] = useState(false);
  const grid = useRef<HTMLDivElement>(null);
  const anchor = useRef<number | null>(null);

  const can = (a: AccountSummary) => a.status === "valid" && !live.has(a.id);
  const validIds = accounts.filter(can).map((a) => a.id).sort((x, y) => x - y);
  // ponytail: the first 100 only; one request per account each minute. Past that, tiles show Ready.
  const presence = usePresence(validIds.slice(0, 100)).data;
  const needle = q.trim().toLowerCase();
  const shown = accounts.filter((a) => (group === null || a.groupIds.includes(group)) && (!needle || matches(a, needle)));
  const has = new Set(picked);
  const mainId = protect ? (accounts.find((a) => a.main)?.id ?? null) : null;
  const pool = shown.filter(can).map((a) => a.id);
  const all = pool.filter((id) => id !== mainId);
  const allPicked = all.length > 0 && all.every((id) => has.has(id));
  const mainLeft = allPicked && mainId !== null && pool.includes(mainId) && !has.has(mainId);
  const count = accounts.filter((a) => can(a) && has.has(a.id)).length;

  // A pick that is playing goes with this one, or it would come back by itself when its client ends.
  const add = (ids: number[]) => onPicked([...new Set([...picked.filter((id) => !live.has(id)), ...ids])]);
  const selectAll = () => {
    add(all);
    setCascading(true);
  };
  const clear = () => onPicked(picked.filter((id) => !shown.some((a) => a.id === id)));
  const toggle = (a: AccountSummary, range: boolean) => {
    if (!can(a)) return;
    const from = range && anchor.current !== null ? shown.findIndex((x) => x.id === anchor.current) : -1;
    const to = shown.indexOf(a);
    const ids = from < 0 ? [a.id] : shown.slice(Math.min(from, to), Math.max(from, to) + 1).filter(can).map((x) => x.id);
    anchor.current = a.id;
    if (has.has(a.id)) onPicked(picked.filter((id) => !ids.includes(id)));
    else add(ids);
  };
  // Stable callbacks for the memoised tiles; they reach the latest state through a ref.
  const latest = useRef({ toggle, accounts });
  latest.current = { toggle, accounts };
  const onToggle = useCallback((id: number, range: boolean) => {
    const a = latest.current.accounts.find((x) => x.id === id);
    if (a) latest.current.toggle(a, range);
  }, []);
  const onFocusId = useCallback((id: number) => setFocusId(id), []);
  // The window: a new filter starts from the first page again.
  const windowKey = `${needle}\n${group}`;
  const [win, setWin] = useState({ key: windowKey, n: PAGE });
  const limit = win.key === windowKey ? win.n : PAGE;
  const visible = shown.length > limit ? shown.slice(0, limit) : shown;
  const focusable = visible.some((a) => a.id === focusId) ? focusId : visible[0]?.id;
  const more = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = more.current;
    if (!el) return;
    const io = new IntersectionObserver((entries) => entries.some((e) => e.isIntersecting) && setWin({ key: windowKey, n: limit + PAGE }), { root: grid.current, rootMargin: "600px" });
    io.observe(el);
    return () => io.disconnect();
  }, [windowKey, limit, visible.length < shown.length]); // eslint-disable-line react-hooks/exhaustive-deps
  const keys = (e: KeyboardEvent<HTMLDivElement>) => {
    const tiles = [...e.currentTarget.querySelectorAll<HTMLElement>("[data-tile]")];
    const at = tiles.indexOf(document.activeElement as HTMLElement);
    if (at < 0) return;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") {
      e.preventDefault();
      selectAll();
      return;
    }
    if (e.key === " " && e.shiftKey) {
      e.preventDefault();
      toggle(shown[at], true);
      return;
    }
    const row = tiles.findIndex((el) => el.offsetTop > tiles[0].offsetTop);
    const cols = row < 0 ? tiles.length : row;
    const fwd = isRtl() ? -1 : 1;
    const next = ({ ArrowRight: at + fwd, ArrowLeft: at - fwd, ArrowDown: at + cols, ArrowUp: at - cols, Home: 0, End: tiles.length - 1 } as Record<string, number>)[e.key];
    if (next === undefined || next < 0 || next >= tiles.length) return;
    e.preventDefault();
    tiles[next].focus();
  };

  useEffect(() => {
    if (!cascading) return;
    const timer = setTimeout(() => setCascading(false), 600);
    return () => clearTimeout(timer);
  }, [cascading]);
  // A filter change: tiles that stay glide to their new places, the ones coming back rise in.
  const few = shown.length <= STAGGER.listLimit;
  useFlip(grid);
  const fresh = useFreshItems(shown.map((a) => a.id));
  useLayoutEffect(() => {
    fresh.forEach((id, i) => {
      const el = grid.current?.querySelector(`[data-id="${id}"]`);
      if (el) riseIn(el, i);
    });
  }, [fresh.join()]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!accounts.length)
    return (
      <div className="flex flex-wrap items-center gap-3.5">
        <p className="flex-[1_1_240px] text-fg-2">{t("launch.noAccounts")}</p>
        <Button variant="primary" icon="plus" onClick={onAdd}>{t("accounts.add.title")}</Button>
      </div>
    );
  const usedGroups = groups.filter((g) => accounts.some((a) => a.groupIds.includes(g.id)));
  return (
    <div className={`flex flex-col gap-3 min-h-0 ${className}`}>
      {accounts.every((a) => a.status !== "valid") && (
        <div className="notice">
          <p className="flex-[1_1_240px] min-w-0 text-fg-2">{t("home.signedOutLead")}</p>
          <Button size="md" onClick={onAccounts}>{t("app.nav.accounts")}</Button>
        </div>
      )}
      <Search value={q} onChange={setQ} find />
      {usedGroups.length > 0 && (
        <div role="group" aria-label={t("accounts.groups.title")} className="flex flex-wrap gap-1.5">
          <Chip small pressed={group === null} onClick={() => setGroup(null)}>{t("accounts.filter.all")}</Chip>
          {usedGroups.map((g) => (
            <Chip key={g.id} small pressed={group === g.id} dot={g.color ?? undefined} count={accounts.filter((a) => a.groupIds.includes(g.id)).length} onClick={() => setGroup(group === g.id ? null : g.id)}>
              {g.name}
            </Chip>
          ))}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 min-h-[var(--h-md)]">
        <span className="text-[13px] text-fg-2" aria-live="polite">{t("launch.pickedOfValid", { picked: count, count: validIds.length })}</span>
        {pool.length > 1 && (
          <Button variant="ghost" size="md" onClick={allPicked ? clear : selectAll}>
            {allPicked ? t("launch.clear") : t("launch.selectAll")}
          </Button>
        )}
      </div>
      <Collapse open={mainLeft}>
        <p className="text-[13px] text-fg-3">{t("accounts.mainSkipped")}</p>
      </Collapse>
      {shown.length === 0 ? (
        <div className="flex flex-wrap items-center gap-3">
          <p className="text-fg-2">{t("accounts.noMatch")}</p>
          <Button variant="ghost" size="md" onClick={() => (setQ(""), setGroup(null))}>{t("launch.clear")}</Button>
        </div>
      ) : (
        <div
          ref={grid}
          role="group"
          aria-label={label}
          className={`scroll grid gap-3 min-h-0 p-1 -m-1 ${cascading ? "launch-cascade" : ""}`}
          style={{ gridTemplateColumns: "repeat(auto-fill, minmax(min(240px, 100%), 1fr))" }}
          onKeyDown={keys}
        >
          {visible.map((a, i) => (
            <Tile
              key={a.id}
              account={a}
              on={can(a) && has.has(a.id)}
              can={can(a)}
              live={live.has(a.id)}
              focusable={a.id === focusable}
              presence={presence?.get(a.id)}
              i={i}
              flip={few}
              onToggle={onToggle}
              onFocusId={onFocusId}
            />
          ))}
          {visible.length < shown.length && <div ref={more} aria-hidden="true" className="col-span-full h-px" />}
        </div>
      )}
    </div>
  );
}

/** Servers' send bar: the signed-in accounts as chips; a playing one can't be picked and says so. */
function Chips({ accounts, picked, onPicked, label }: Picks) {
  const live = useLive();
  const [q, setQ] = useState("");
  const [focusId, setFocusId] = useState<number | null>(null);
  const valid = accounts.filter((a) => a.status === "valid");
  if (!valid.length) return <span className="text-[13px] text-fg-2">{t("servers.noValidAccount")}</span>;
  const needle = q.trim().toLowerCase();
  const shown = needle ? valid.filter((a) => matches(a, needle)) : valid;
  const has = new Set(picked);
  const focusable = shown.some((a) => a.id === focusId) ? focusId : shown[0]?.id;
  return (
    <div className="flex flex-col gap-2 min-w-0">
      {valid.length > 12 && <Search value={q} onChange={setQ} />}
      <div role="group" aria-label={label} className="flex flex-wrap gap-1.5" onKeyDown={(e) => rovingKeys(e, "[data-tile]")}>
        {shown.map((a) => {
          const playing = live.has(a.id);
          return (
            <button
              key={a.id}
              type="button"
              data-tile
              className="chip chip-sm launch-chip"
              aria-pressed={!playing && has.has(a.id)}
              aria-disabled={playing || undefined}
              tabIndex={a.id === focusable ? 0 : -1}
              onFocus={() => setFocusId(a.id)}
              onClick={() => !playing && onPicked(has.has(a.id) ? picked.filter((id) => id !== a.id) : [...picked.filter((id) => !live.has(id)), a.id])}
            >
              <span aria-hidden="true" className="w-2 h-2 rounded-full flex-none" style={{ background: avatarColor(a) }} />
              <span className="truncate">{accountName(a)}</span>
              {playing && <span className="font-normal">· {t("play.already")}</span>}
            </button>
          );
        })}
        {!shown.length && <span className="text-[13px] text-fg-2">{t("accounts.noMatch")}</span>}
      </div>
    </div>
  );
}

/** Home's "+": a search that keeps the focus, arrows move through the accounts, Enter picks. */
function Menu({ accounts, label, onPick, onAdd }: { accounts: AccountSummary[]; label: string; onPick: (id: number) => void; onAdd: () => void }) {
  const list = useId();
  const [q, setQ] = useState("");
  const [active, setActive] = useState(0);
  const needle = q.trim().toLowerCase();
  const shown = needle ? accounts.filter((a) => matches(a, needle)) : accounts;
  const at = Math.min(active, shown.length - 1);
  const optionId = (a: AccountSummary) => `${list}-${a.id}`;
  const activeId = shown[at] ? optionId(shown[at]) : undefined;
  // Keeps the active option in sight within the list only (scrollIntoView would move the page too).
  useLayoutEffect(() => {
    const el = activeId ? document.getElementById(activeId) : null;
    const box = el?.parentElement;
    if (!el || !box) return;
    if (el.offsetTop < box.scrollTop) box.scrollTop = el.offsetTop;
    else if (el.offsetTop + el.offsetHeight > box.scrollTop + box.clientHeight) box.scrollTop = el.offsetTop + el.offsetHeight - box.clientHeight;
  }, [activeId]);
  const keys = (e: KeyboardEvent<HTMLInputElement>) => {
    if ((e.key === "ArrowDown" || e.key === "ArrowUp") && shown.length) {
      e.preventDefault();
      const by = e.key === "ArrowDown" ? 1 : -1;
      setActive((x) => (Math.min(x, shown.length - 1) + by + shown.length) % shown.length);
    } else if (e.key === "Enter" && shown[at]) {
      e.preventDefault();
      onPick(shown[at].id);
    }
  };
  return (
    <div className="flex flex-col gap-1">
      <Input
        leading="search"
        role="combobox"
        aria-expanded="true"
        aria-controls={list}
        aria-autocomplete="list"
        aria-activedescendant={activeId}
        aria-label={t("accounts.filterLabel")}
        placeholder={t("accounts.filterPlaceholder")}
        autoComplete="off"
        value={q}
        onChange={(e) => {
          setQ(e.target.value);
          setActive(0);
        }}
        onKeyDown={keys}
      />
      <ul id={list} role="listbox" aria-label={label} className="scroll relative flex flex-col max-h-[280px]">
        {shown.map((a, i) => (
          <li
            key={a.id}
            id={optionId(a)}
            role="option"
            aria-selected={i === at}
            className="menu-item launch-option"
            data-active={i === at || undefined}
            onPointerMove={() => i !== at && setActive(i)}
            onClick={() => onPick(a.id)}
          >
            <Avatar account={a} size={24} />
            <span className="flex-1 min-w-0 truncate">{accountName(a)}</span>
            {a.status !== "valid" && (
              <span className="flex min-w-0 text-xs">
                <Status status={a.status} />
              </span>
            )}
          </li>
        ))}
      </ul>
      {!shown.length && <p role="status" className="px-2.5 py-2 text-[13px] text-fg-2">{t("accounts.noMatch")}</p>}
      <div role="separator" className="menu-sep" />
      <button type="button" className="menu-item" onClick={onAdd}>
        <Icon name="plus" size={14} />
        {t("accounts.add.title")}
      </button>
    </div>
  );
}

/** A favourite's default accounts: a checklist with each one's status. */
function List({ accounts, picked, onPicked, label }: Picks) {
  const [q, setQ] = useState("");
  const needle = q.trim().toLowerCase();
  const shown = needle ? accounts.filter((a) => matches(a, needle)) : accounts;
  const has = new Set(picked);
  return (
    <div className="flex flex-col gap-1.5">
      {accounts.length > 12 && <Search value={q} onChange={setQ} />}
      <div role="group" aria-label={label} className="well scroll max-h-[208px] p-1.5">
        {!accounts.length ? <p className="p-3 text-fg-2">{t("launch.fav.noAccounts")}</p> : !shown.length && <p className="p-3 text-fg-2">{t("accounts.noMatch")}</p>}
        {shown.map((a) => (
          <label key={a.id} className="flex items-center gap-2.5 min-h-11 px-2.5 rounded-[10px] cursor-pointer hover:bg-hover has-[:checked]:bg-sel">
            <input type="checkbox" className="cb" checked={has.has(a.id)} onChange={() => onPicked(has.has(a.id) ? picked.filter((id) => id !== a.id) : [...picked, a.id])} />
            <Avatar account={a} size={20} />
            <span className="flex-1 min-w-0 truncate">{accountName(a)}</span>
            <span className="flex justify-end min-w-0 max-w-[45%] text-xs">
              <Status status={a.status} />
            </span>
          </label>
        ))}
      </div>
    </div>
  );
}

export type AccountPickerProps =
  | ({ variant: "tiles"; onAdd: () => void; onAccounts: () => void; className?: string } & Picks)
  | ({ variant: "chips" } & Picks)
  | ({ variant: "list" } & Picks)
  | { variant: "menu"; accounts: AccountSummary[]; label: string; onPick: (id: number) => void; onAdd: () => void };

/** Who plays, one picker in four shapes: "tiles" (Play's grid), "chips" (Servers' send bar), "menu"
 * (Home's "+": Enter picks, then Add account) and "list" (a favourite's default accounts). `label`
 * names the group for screen readers. In tiles and chips, picking an account drops picks that are
 * playing (they'd come back by themselves once their client ends). */
export function AccountPicker(props: AccountPickerProps) {
  switch (props.variant) {
    case "tiles":
      return <Tiles {...props} />;
    case "chips":
      return <Chips {...props} />;
    case "list":
      return <List {...props} />;
    case "menu":
      return <Menu {...props} />;
  }
}
