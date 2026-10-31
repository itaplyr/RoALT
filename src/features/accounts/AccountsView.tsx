import { useCallback, useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState, type PointerEvent, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, type AccountStatus, type AccountSummary, type Density, type Group, type Presence, type VaultMode } from "../../api";
import { groupColor, qk, setAccounts, useInstances, usePresence, useUpdateAccount } from "../../data";
import { ConfirmDialog } from "../../ui/Dialog";
import { menuKeys } from "../../ui/menu";
import { Icon } from "../../ui/Icon";
import { toast } from "../../ui/Toast";
import { AccountMenu, type MenuAt } from "./AccountMenu";
import { isLive } from "../launch/Running";
import { DetailPanel } from "./DetailPanel";
import { GroupChips } from "./GroupsNav";
import { accountName, ago, Avatar, Status } from "./shared";
import { t, tx, type Key } from "../../i18n";
import { Divider, usePanelWidth } from "../../ui/Resizer";

type Filter = "all" | "inGame" | "online" | "offline" | Exclude<AccountStatus, "valid"> | "fav";
// The chips match what the Status column shows.
const FILTERS: { key: Filter; label: Key }[] = [
  { key: "all", label: "accounts.filter.all" },
  { key: "inGame", label: "accounts.presence.inGame" },
  { key: "online", label: "accounts.presence.online" },
  { key: "offline", label: "accounts.presence.offline" },
  { key: "invalid", label: "accounts.status.invalid" },
  { key: "challenged", label: "accounts.status.challenged" },
  { key: "unknown", label: "accounts.status.unknown" },
  { key: "fav", label: "accounts.filter.fav" },
];

/** What the Status cell shows: presence for a valid cookie (Studio counts as online), else the cookie status. */
const shownKey = (a: AccountSummary, p?: Presence): Filter | "valid" => {
  if (a.status !== "valid") return a.status;
  if (!p || p.kind === "unknown") return "valid";
  return p.kind === "studio" ? "online" : p.kind;
};
const inFilter = (a: AccountSummary, f: Filter, p?: Presence) =>
  f === "all" || (f === "fav" ? a.favourite : shownKey(a, p) === f);
const isTyping = (el: EventTarget | null) =>
  el instanceof HTMLElement &&
  (el.matches("textarea, select, input:not([type=checkbox]):not([type=radio])") || el.isContentEditable);
// Cookies are checked once per app session, when the list first shows (after unlock in master mode).
let validatedThisSession = false;
const reducedMotion = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

export function AccountsView({ accounts, groups, density, vaultMode, clipboardSeconds, group, setGroup, selected, setSelected, onAdd, onLaunch }: {
  accounts: AccountSummary[];
  groups: Group[];
  density: Density;
  vaultMode: VaultMode;
  clipboardSeconds: number;
  group: number | null;
  setGroup: (id: number | null) => void;
  selected: number | null;
  setSelected: (id: number | null) => void;
  onAdd: () => void;
  onLaunch: (ids: number[]) => void;
}) {
  const detail = usePanelWidth("detail", 340, 280, 560);
  const [menu, setMenu] = useState<MenuAt | null>(null);
  const closeMenu = useCallback(() => setMenu(null), []);
  const qc = useQueryClient();
  const update = useUpdateAccount();
  const [q, setQ] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [checked, setChecked] = useState<Set<number>>(new Set());
  const [removeIds, setRemoveIds] = useState<number[]>([]);
  const [removeOpen, setRemoveOpen] = useState(false);
  const [drag, setDrag] = useState<{ id: number; over: number } | null>(null);
  const [order, setOrder] = useState<number[] | null>(null);
  const [moved, setMoved] = useState("");
  const [now, tick] = useReducer(() => Date.now(), Date.now());
  const tbody = useRef<HTMLTableSectionElement>(null);
  const dragging = useRef<{ id: number; startY: number; row: HTMLElement; over: number } | null>(null);
  const flip = useRef<Map<number, number> | null>(null);
  const seen = useRef(new Set<number>());
  const compact = density === "compact";
  // An account with a client on its way or in a game can't start another.
  const live = new Set((useInstances().data ?? []).filter(isLive).map((i) => i.accountId));

  useEffect(() => {
    const timer = setInterval(tick, 60_000);
    return () => clearInterval(timer);
  }, []);
  // A local order bridges the gap until the optimistic cache update arrives.
  useEffect(() => setOrder(null), [accounts]);

  const byId = useMemo(() => new Map(accounts.map((a) => [a.id, a])), [accounts]);
  const groupById = useMemo(() => new Map(groups.map((g) => [g.id, g])), [groups]);
  const ordered = order ? order.flatMap((id) => byId.get(id) ?? []) : accounts;
  // A group deleted while it was the filter means no filter.
  const inGroup = group === null || !groupById.has(group) ? ordered : ordered.filter((a) => a.groupIds.includes(group));
  const needle = q.trim().toLowerCase();
  const matching = inGroup.filter(
    (a) => !needle || `${a.alias}\n${a.displayName}\n@${a.username}\n${a.userId}`.toLowerCase().includes(needle),
  );
  // Presence only for rows that can be on screen, and only for valid cookies (DESIGN: visible rows only).
  // Not per search: a new sweep for each result. ponytail: the first 100 unless a status filter
  // needs them all (one request per account each minute); past that, rows show Ready.
  const presenceIds = useMemo(
    () => {
      const ids = inGroup.filter((a) => a.status === "valid").map((a) => a.id).sort((x, y) => x - y);
      return filter === "all" ? ids.slice(0, 100) : ids;
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [inGroup.map((a) => `${a.id}:${a.status}`).join(), filter],
  );
  const presence = usePresence(presenceIds).data;
  const shown = matching.filter((a) => inFilter(a, filter, presence?.get(a.id)));
  const checkedIds = accounts.filter((a) => checked.has(a.id)).map((a) => a.id);
  const canPlay = (id: number) => byId.get(id)?.status === "valid" && !live.has(id);
  const checkedValid = checkedIds.filter(canPlay).length;
  const current = (selected !== null && byId.get(selected)) || shown[0] || null;
  const needsSignIn = accounts.filter((a) => a.status === "invalid").length;
  const lastChecked = Math.max(0, ...accounts.map((a) => a.statusCheckedAt ?? 0)) || null;
  const activeGroup = group === null ? null : groupById.get(group) ?? null;

  const validate = useMutation({
    mutationFn: async (ids?: number[]) => ({ started: Date.now(), list: await api.accountsValidate(ids) }),
    onSuccess: ({ started, list }, ids) => {
      qc.setQueryData(qk.accounts, list);
      const asked = ids ? list.filter((a) => ids.includes(a.id)) : list;
      // Accounts Roblox couldn't be asked about (network, rate limit) keep their old stamp.
      const done = asked.filter((a) => (a.statusCheckedAt ?? 0) >= started - 1000);
      const missed = asked.length - done.length;
      const bad = done.filter((a) => a.status === "invalid").length;
      const ch = done.filter((a) => a.status === "challenged").length;
      toast(
        [
          t("accounts.toast.checked", { count: done.length }),
          missed && t("accounts.toast.missed", { count: missed }),
          bad && t("accounts.toast.needSignIn", { count: bad }),
          ch && t("accounts.toast.challenged", { count: ch }),
        ]
          .filter(Boolean)
          .join(" · "),
      );
    },
  });
  useEffect(() => {
    if (validatedThisSession || !accounts.length) return;
    validatedThisSession = true;
    validate.mutate(undefined);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const checking = validate.isPending ? (validate.variables ?? null) : undefined;
  const isChecking = (id: number) => checking === null || (checking !== undefined && checking.includes(id));

  const remove = useMutation({
    mutationFn: (ids: number[]) => api.accountsRemove(ids),
    onSuccess: (_, ids) => {
      setAccounts(qc, (list) => list.filter((a) => !ids.includes(a.id)));
      // Ids can be reused by the next account added: drop what belonged to these.
      for (const id of ids) qc.removeQueries({ queryKey: qk.info(id) });
      setChecked(new Set());
      setRemoveOpen(false);
      toast(t("accounts.toast.removed", { count: ids.length }));
    },
  });

  const reorder = useMutation({
    mutationFn: (ids: number[]) => api.accountsReorder(ids),
    onError: () => qc.invalidateQueries({ queryKey: qk.accounts }),
  });

  const askRemove = (ids: number[]) => {
    setRemoveIds(ids);
    setRemoveOpen(true);
  };
  const runValidate = () => {
    if (!validate.isPending && accounts.length) validate.mutate(checkedIds.length ? checkedIds : undefined);
  };

  // ---------- reorder (pointer drag on the grip, or arrow keys on it) ----------

  function snapshot() {
    const m = new Map<number, number>();
    for (const r of tbody.current?.rows ?? []) m.set(Number(r.dataset.id), r.getBoundingClientRect().top);
    flip.current = m;
  }

  function applyOrder(ids: number[]) {
    snapshot();
    setOrder(ids);
    setAccounts(qc, (list) => list.map((a) => ({ ...a, sortOrder: ids.indexOf(a.id) })));
    reorder.mutate(ids);
  }

  /** Moves `id` so it sits before shown[over] (or after the last shown row). */
  function dropAt(id: number, over: number) {
    const from = shown.findIndex((a) => a.id === id);
    if (from < 0 || over < 0 || over > shown.length || over === from || over === from + 1) return false;
    const ids = ordered.map((a) => a.id).filter((x) => x !== id);
    const at = over < shown.length ? ids.indexOf(shown[over].id) : ids.indexOf(shown[shown.length - 1].id) + 1;
    ids.splice(at, 0, id);
    applyOrder(ids);
    return true;
  }

  // FLIP: rows glide from where they were to where the new order puts them.
  useLayoutEffect(() => {
    const before = flip.current;
    const rows = tbody.current ? [...tbody.current.rows] : [];
    if (!before || !rows.length) return;
    flip.current = null;
    for (const r of rows) {
      r.style.transition = "none";
      r.style.transform = "";
    }
    const deltas = rows.map((r) => (before.get(Number(r.dataset.id)) ?? r.getBoundingClientRect().top) - r.getBoundingClientRect().top);
    rows.forEach((r, i) => Math.abs(deltas[i]) >= 1 && (r.style.transform = `translateY(${deltas[i]}px)`));
    void tbody.current?.offsetHeight;
    for (const r of rows) {
      r.style.transition = "";
      r.style.transform = "";
    }
  });

  // New rows rise in. Done with WAAPI because a CSS animation restarts whenever React moves the row.
  useLayoutEffect(() => {
    const rows = tbody.current ? [...tbody.current.rows] : [];
    const fresh = rows.filter((r) => !seen.current.has(Number(r.dataset.id)));
    seen.current = new Set(rows.map((r) => Number(r.dataset.id)));
    // A whole list coming back at once is no new row, and would animate thousands of elements.
    if (reducedMotion() || fresh.length > 40) return;
    fresh.forEach((r, i) =>
      r.animate([{ opacity: 0, transform: "translateY(8px)" }, { opacity: 1, transform: "none" }], {
        duration: 220,
        easing: "cubic-bezier(.2,.8,.2,1)",
        delay: Math.min(i, 14) * 16,
        fill: "backwards",
      }),
    );
  });

  function gripDown(e: PointerEvent<HTMLButtonElement>, id: number) {
    const row = e.currentTarget.closest("tr");
    if (e.button !== 0 || !row) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    const from = shown.findIndex((a) => a.id === id);
    row.style.transition = "none";
    dragging.current = { id, startY: e.clientY, row, over: from };
    setDrag({ id, over: from });
  }

  function gripMove(e: PointerEvent) {
    const d = dragging.current;
    if (!d || !tbody.current) return;
    d.row.style.transform = `translateY(${e.clientY - d.startY}px)`;
    const rows = [...tbody.current.rows];
    let over = rows.length;
    for (let i = 0; i < rows.length; i++) {
      if (rows[i] === d.row) continue;
      const r = rows[i].getBoundingClientRect();
      if (e.clientY < r.top + r.height / 2) {
        over = i;
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

  function gripKey(e: ReactKeyboardEvent, id: number) {
    const dir = e.key === "ArrowUp" ? -1 : e.key === "ArrowDown" ? 1 : 0;
    if (!dir) return;
    e.preventDefault();
    const i = shown.findIndex((a) => a.id === id);
    if (dropAt(id, dir < 0 ? i - 1 : i + 2)) {
      setMoved(`${accountName(shown[i])}: ${i + 1 + dir} / ${shown.length}`);
      requestAnimationFrame(() => document.querySelector<HTMLElement>(`[data-grip="${id}"]`)?.focus());
    }
  }

  const from = drag ? shown.findIndex((a) => a.id === drag.id) : -1;
  const dropLine = drag && drag.over !== from && drag.over !== from + 1 ? drag.over : -1;

  // ---------- selection and shortcuts ----------

  const toggle = (id: number) =>
    setChecked((s) => {
      const n = new Set(s);
      if (!n.delete(id)) n.add(id);
      return n;
    });
  const shownChecked = shown.filter((a) => checked.has(a.id)).length;
  const allShown = shown.length > 0 && shownChecked === shown.length;
  const toggleAll = () =>
    setChecked((s) => {
      const n = new Set(s);
      for (const a of shown) allShown ? n.delete(a.id) : n.add(a.id);
      return n;
    });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector("dialog[open], :popover-open")) return;
      const typing = isTyping(e.target);
      const ctrl = e.ctrlKey || e.metaKey;
      const k = e.key.toLowerCase();
      if (ctrl && k === "a" && !typing) {
        e.preventDefault();
        setChecked((s) => new Set([...s, ...shown.map((a) => a.id)]));
      } else if (ctrl && k === "r") {
        e.preventDefault();
        runValidate();
      } else if (e.key === "Delete" && !typing) {
        const row = e.target instanceof Element ? e.target.closest<HTMLElement>("tr[data-id]") : null;
        const ids = checkedIds.length ? checkedIds : row ? [Number(row.dataset.id)] : current ? [current.id] : [];
        if (ids.length) askRemove(ids);
      } else if (e.key === "Escape" && !typing) {
        setChecked(new Set());
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  async function addToGroup(g: Group) {
    const targets = checkedIds.flatMap((id) => byId.get(id) ?? []).filter((a) => !a.groupIds.includes(g.id));
    try {
      for (const a of targets) await update.mutateAsync({ id: a.id, patch: { groupIds: [...a.groupIds, g.id] } });
      toast(t("accounts.toast.addedToGroup", { count: checkedIds.length, group: g.name }));
    } catch {
      // The mutation cache already showed the error.
    }
  }

  const counts = (f: Filter) => matching.filter((a) => inFilter(a, f, presence?.get(a.id))).length;

  return (
    <>
      <main className={`@container scroll flex-1 min-w-0 flex flex-col gap-4 px-8 pt-5 pb-6 ${compact ? "compact" : ""}`}>
        <span className="sr-only" aria-live="polite">{moved}</span>
        <div className="flex flex-wrap items-center gap-3">
          <div className="min-w-0">
            <h1 className="page-title">{t("accounts.title")}</h1>
            <p className="page-sub">
              {t("accounts.total", { count: accounts.length })}
              {needsSignIn > 0 && <> · <span className="text-orange">{t("accounts.toast.needSignIn", { count: needsSignIn })}</span></>}
            </p>
          </div>
          <div className="ms-auto flex flex-wrap gap-3">
            <button className="btn" onClick={() => !validate.isPending && validate.mutate(undefined)} disabled={validate.isPending || !accounts.length} title={t("accounts.validateAllHint", { keys: "Ctrl+R" })}>
              <Icon name="refresh" size={14} className={validate.isPending ? "spin" : ""} />
              {validate.isPending ? t("accounts.validating") : t("accounts.validateAll")}
            </button>
            <button className="btn btn-primary" onClick={onAdd} title={t("accounts.addHint", { keys: "Ctrl+N" })}>
              <Icon name="plus" size={14} />
              {t("accounts.add.title")}
            </button>
          </div>
        </div>

        {accounts.length === 0 ? (
          <>
            {/* Left over from accounts that are gone: they can still be edited or deleted. */}
            {groups.length > 0 && (
              <div className="flex flex-wrap items-center gap-3">
                <GroupChips groups={groups} accounts={accounts} active={activeGroup?.id ?? null} onPick={setGroup} />
              </div>
            )}
            <Empty onAdd={onAdd} />
          </>
        ) : (
          <>
            <div className="flex flex-wrap items-center gap-3">
              <label className="field flex items-center gap-2 flex-[1_1_160px] max-w-[320px] w-auto text-dim focus-within:border-[var(--focus)]">
                <Icon name="search" size={14} />
                <span className="sr-only">{t("accounts.filterLabel")}</span>
                <input
                  id="account-filter"
                  value={q}
                  onChange={(e) => setQ(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape" && q) {
                      e.stopPropagation();
                      setQ("");
                    }
                  }}
                  placeholder={t("accounts.filterPlaceholder")}
                  spellCheck={false}
                  className="flex-1 min-w-0 bg-transparent outline-none text-text"
                />
              </label>
              <button
                className="chip"
                popoverTarget="status-filter"
                style={{ anchorName: "--status-filter" }}
                data-on={filter !== "all" || undefined}
              >
                <Icon name="sliders" size={14} />
                {filter === "all" ? t("accounts.filter.status") : t(FILTERS.find((f) => f.key === filter)!.label)}
                <Icon name="chevron" size={12} />
              </button>
              <div id="status-filter" popover="auto" className="menu" onKeyDown={menuKeys} style={{ positionAnchor: "--status-filter", positionArea: "block-end span-inline-end", margin: "6px 0 0" }}>
                {FILTERS.map((f) => (
                  <button
                    key={f.key}
                    aria-pressed={filter === f.key}
                    className="menu-item"
                    popoverTarget="status-filter"
                    popoverTargetAction="hide"
                    onClick={() => setFilter(f.key)}
                  >
                    <Icon name="check" size={12} className={filter === f.key ? "" : "invisible"} />
                    <span className="flex-1">{t(f.label)}</span>
                    <span className="mono text-xs text-dim">{counts(f.key)}</span>
                  </button>
                ))}
              </div>
              <GroupChips groups={groups} accounts={accounts} active={activeGroup?.id ?? null} onPick={setGroup} />
            </div>

            <div className="flex-[1_0_300px] min-h-0 flex">
            <div className="panel flex-1 min-w-0 flex flex-col overflow-hidden p-2 me-3">
            <div className="scroll flex-1 min-h-0">
              <table className="w-full border-separate border-spacing-0">
                <thead>
                  <tr>
                    <th className="w-[68px] ps-[35px] rounded-ss-[14px]">
                      <input
                        type="checkbox"
                        className="cb"
                        aria-label={t("accounts.selectAll")}
                        checked={allShown}
                        ref={(el) => {
                          if (el) el.indeterminate = shownChecked > 0 && !allShown;
                        }}
                        onChange={toggleAll}
                      />
                    </th>
                    <th className="label">{t("accounts.colAccount")}</th>
                    <th className="label w-[30%] hidden @3xl:table-cell">{t("accounts.colStatus")}</th>
                    <th className="label w-[22%] hidden @5xl:table-cell">{t("accounts.groups.title")}</th>
                    <th className="w-[104px]"><span className="sr-only">{t("launch.running.actions")}</span></th>
                  </tr>
                </thead>
                <tbody ref={tbody}>
                  {shown.map((a, i) => {
                    const name = accountName(a);
                    const cls = [
                      "row",
                      drag?.id === a.id && "row-drag",
                      dropLine === i && "drop-top",
                      dropLine === shown.length && i === shown.length - 1 && "drop-bottom",
                    ];
                    return (
                      <tr
                        key={a.id}
                        data-id={a.id}
                        data-selected={current?.id === a.id || undefined}
                        aria-current={current?.id === a.id || undefined}
                        className={cls.filter(Boolean).join(" ")}
                        onClick={() => setSelected(a.id)}
                        onContextMenu={(e) => {
                          e.preventDefault();
                          setSelected(a.id);
                          setMenu({ account: a, x: e.clientX, y: e.clientY });
                        }}
                      >
                        <td className="ps-3">
                          <div className="flex items-center gap-[5px]">
                            <button
                              type="button"
                              className="grip"
                              data-grip={a.id}
                              aria-label={t("accounts.reorder", { name })}
                              onPointerDown={(e) => gripDown(e, a.id)}
                              onPointerMove={gripMove}
                              onPointerUp={() => gripEnd(false)}
                              onPointerCancel={() => gripEnd(true)}
                              onKeyDown={(e) => gripKey(e, a.id)}
                              onClick={(e) => e.stopPropagation()}
                            >
                              <Icon name="grip" size={14} />
                            </button>
                            <input
                              type="checkbox"
                              className="cb"
                              aria-label={t("accounts.select", { name })}
                              checked={checked.has(a.id)}
                              onChange={() => toggle(a.id)}
                              onClick={(e) => e.stopPropagation()}
                            />
                          </div>
                        </td>
                        <td className="pe-3 max-w-0">
                          <div className="flex items-center gap-2.5 min-w-0">
                            <Avatar account={a} size={compact ? 22 : 28} />
                            <div className={compact ? "flex items-baseline gap-2 min-w-0" : "flex flex-col min-w-0 leading-tight"}>
                              <button type="button" className="linkbtn font-semibold truncate" onClick={() => setSelected(a.id)}>
                                {name}
                              </button>
                              <span className="mono text-xs text-dim truncate hidden @3xl:block">@{a.username}</span>
                              {/* No room for the Status column. */}
                              <span className="flex min-w-0 text-xs @3xl:hidden">
                                <Status status={a.status} presence={presence?.get(a.id)} checking={isChecking(a.id)} />
                              </span>
                            </div>
                          </div>
                        </td>
                        <td className="pe-3 max-w-0 hidden @3xl:table-cell">
                          <Status
                            status={a.status}
                            presence={presence?.get(a.id)}
                            checking={isChecking(a.id)}
                            detail={!compact && a.statusCheckedAt ? ago(a.statusCheckedAt, now) : undefined}
                          />
                        </td>
                        <td className="pe-3 hidden @5xl:table-cell">
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
                          <span className="flex items-center justify-end gap-1">
                            <button
                              type="button"
                              className="wc w-9 h-9"
                              style={a.favourite ? { color: "var(--accent-text)" } : undefined}
                              aria-label={t("accounts.favouriteName", { name })}
                              aria-pressed={a.favourite}
                              onClick={(e) => {
                                e.stopPropagation();
                                update.mutate({ id: a.id, patch: { favourite: !a.favourite } });
                              }}
                            >
                              <Icon name="star" size={14} fill={a.favourite} />
                            </button>
                            {canPlay(a.id) && (
                              <button
                                type="button"
                                className="btn btn-sm btn-icon w-9"
                                aria-label={t("launch.launchAs", { name })}
                                title={t("launch.launchAs", { name })}
                                onClick={(e) => {
                                  e.stopPropagation();
                                  onLaunch([a.id]);
                                }}
                              >
                                <Icon name="play" size={12} fill />
                              </button>
                            )}
                          </span>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              {shown.length === 0 && (
                <div className="enter flex flex-col items-center gap-3 py-16 text-dim">
                  {t("accounts.noMatch")}
                  <button
                    className="btn btn-sm"
                    onClick={() => {
                      setQ("");
                      setFilter("all");
                      setGroup(null);
                    }}
                  >
                    {t("accounts.clearFilters")}
                  </button>
                </div>
              )}
            </div>

        <footer className="h-7 flex-none flex items-center gap-4 px-3 text-xs text-dim">
          <span>{lastChecked ? t("accounts.validatedAgo", { when: ago(lastChecked, now) }) : t("accounts.notValidated")}</span>
          <span className="mono ms-auto">v{import.meta.env.APP_VERSION}</span>
        </footer>
            </div>
      {current && <Divider panel={detail} side="right" label={t("accounts.detailsWidth")} />}
      {current && (
        <aside
          key={current.id}
          // Pin the panel to this account once it is used, so an edit that drops it from the
          // filtered list can't swap the panel to another account under the next click.
          onFocusCapture={() => setSelected(current.id)}
          aria-label={t("accounts.details")}
          style={{ width: detail.width, maxWidth: "40cqw" }}
          className="panel scroll flex-none ms-3">
          <DetailPanel
            account={current}
            groups={groups}
            vaultMode={vaultMode}
            clipboardSeconds={clipboardSeconds}
            checking={isChecking(current.id)}
            playing={live.has(current.id)}
            onValidate={() => validate.mutate([current.id])}
            onRemove={() => askRemove([current.id])}
            onLaunch={() => onLaunch([current.id])}
          />
        </aside>
      )}
            </div>
        {checkedIds.length > 0 && (
          <div className="enter well flex-none flex flex-wrap items-center gap-2.5 px-3.5 py-2.5">
            <span className="mono font-medium">{t("accounts.selected", { count: checkedIds.length })}</span>
            <button
              className="btn btn-primary"
              onClick={() => onLaunch(checkedIds)}
              disabled={checkedValid === 0}
              title={checkedValid < checkedIds.length ? t("accounts.cantLaunch", { count: checkedIds.length - checkedValid }) : undefined}
            >
              <Icon name="play" size={12} />
              {t("launch.launchMany", { count: checkedValid })}
            </button>
            <button className="btn" onClick={() => validate.mutate(checkedIds)} disabled={validate.isPending}>
              <Icon name="refresh" size={14} />
              {t("accounts.menu.validate")}
            </button>
            <button className="btn" popoverTarget="bulk-groups" style={{ anchorName: "--bulk-groups" }}>
              {t("accounts.addToGroup")}
              <Icon name="chevron" size={12} />
            </button>
            <div id="bulk-groups" popover="auto" className="menu" style={{ positionAnchor: "--bulk-groups" }}>
              {groups.length === 0 && <div className="px-2.5 py-2 text-dim">{t("accounts.noGroupsMenu")}</div>}
              {groups.map((g) => (
                <button
                  key={g.id}
                  className="menu-item"
                  popoverTarget="bulk-groups"
                  popoverTargetAction="hide"
                  onClick={() => addToGroup(g)}
                >
                  <span aria-hidden="true" className="w-2 h-2" style={{ background: groupColor(g) }} />
                  {g.name}
                </button>
              ))}
            </div>
            <button className="btn btn-danger" onClick={() => askRemove(checkedIds)}>
              <Icon name="trash" size={14} />
              {t("common.remove")}
            </button>
            <button className="btn ms-auto" onClick={() => setChecked(new Set())}>{t("accounts.clear")}</button>
          </div>
        )}
          </>
        )}
      </main>

      <AccountMenu
        at={menu}
        onClose={closeMenu}
        selection={checkedIds}
        canLaunch={canPlay}
        onLaunch={onLaunch}
        onValidate={(ids) => validate.mutate(ids)}
        onRemove={askRemove}
      />
      <ConfirmDialog
        open={removeOpen}
        onClose={() => setRemoveOpen(false)}
        onConfirm={() => remove.mutate(removeIds)}
        busy={remove.isPending}
        title={t("accounts.removeTitle", { count: removeIds.length })}
        text={
          removeIds.length === 1
            ? tx("accounts.removeOneText", {
                name: (
                  <span className="text-text font-medium">
                    {byId.has(removeIds[0]) ? accountName(byId.get(removeIds[0])!) : t("accounts.thisAccount")}
                  </span>
                ),
              })
            : t("accounts.removeManyText", { count: removeIds.length })
        }
        confirmLabel={t("common.remove")}
      />
    </>
  );
}

function Empty({ onAdd }: { onAdd: () => void }) {
  return (
    <div className="enter flex-1 grid place-items-center p-8">
      <div className="panel flex flex-col items-center gap-4 max-w-sm p-8 text-center">
        <span className="num w-12 h-12 text-dim">
          <Icon name="accounts" size={20} />
        </span>
        <h2 className="section-title">{t("accounts.emptyTitle")}</h2>
        <p className="text-dim">
          {t("accounts.emptyText")}
        </p>
        <button className="btn btn-primary" onClick={onAdd}>
          <Icon name="plus" size={14} />
          {t("accounts.add.title")}
        </button>
      </div>
    </div>
  );
}
