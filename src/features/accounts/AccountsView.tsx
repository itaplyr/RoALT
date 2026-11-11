import { useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import "./accounts.css";
import { api, type AccountStatus, type AccountSummary, type Density, type Group, type Presence, type VaultMode } from "../../api";
import { qk, setAccounts, useInstances, usePresence, useSettings, useUpdateAccount } from "../../data";
import { isRtl, t, tx, type Key } from "../../i18n";
import { announce, overlayOpen } from "../../ui/a11y";
import { Kbd } from "../../ui/Bits";
import { Button, IconButton } from "../../ui/Button";
import { ConfirmDialog } from "../../ui/Dialog";
import { Input } from "../../ui/Field";
import { Icon } from "../../ui/Icon";
import { SPRING, Swap, TWEEN, useReducedMotionFlag } from "../../ui/motion";
import { MenuRadioItem, Popover, useAnchor } from "../../ui/Popover";
import { Divider, usePanelWidth } from "../../ui/Resizer";
import { toast } from "../../ui/Toast";
import { isLive } from "../launch/Running";
import { AccountMenu, type MenuAt } from "./AccountMenu";
import { AccountsTable } from "./AccountsTable";
import { AccountsToolbar, Counted, focusFirstItem } from "./AccountsToolbar";
import { DetailPanel, type DetailTab } from "./DetailPanel";
import { ExportCookies } from "./ExportCookies";
import { GroupChips } from "./GroupsNav";
import { accountName, ago } from "./shared";

type Filter = "all" | "inGame" | "online" | "offline" | Exclude<AccountStatus, "valid"> | "fav";
// The filters match what the Status column shows.
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
const inFilter = (a: AccountSummary, f: Filter, p?: Presence) => f === "all" || (f === "fav" ? a.favourite : shownKey(a, p) === f);
const isTyping = (el: EventTarget | null) =>
  el instanceof HTMLElement && (el.matches("textarea, select, input:not([type=checkbox]):not([type=radio])") || el.isContentEditable);
/** Main narrower than this: the details become a sheet over the list. */
const SHEET = 760;
// Cookies are checked once per app session, when the list first shows (after unlock in master mode).
let validatedThisSession = false;

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
  const qc = useQueryClient();
  const update = useUpdateAccount();
  const reduced = useReducedMotionFlag();
  const main = useRef<HTMLElement>(null);
  const sheetRef = useRef<HTMLElement>(null);
  const filterInput = useRef<HTMLInputElement>(null);
  const statusMenu = useAnchor();
  const [q, setQ] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [checked, setChecked] = useState<Set<number>>(() => new Set());
  const [mainSkipped, setMainSkipped] = useState(false);
  const rangeFrom = useRef<number | null>(null);
  const [menu, setMenu] = useState<MenuAt | null>(null);
  const [tab, setTab] = useState<DetailTab>("overview");
  const [narrow, setNarrow] = useState(false);
  const [sheet, setSheet] = useState(false);
  const [removeIds, setRemoveIds] = useState<number[]>([]);
  const [removeOpen, setRemoveOpen] = useState(false);
  const [mainConfirm, setMainConfirm] = useState(false);
  const [befriendOpen, setBefriendOpen] = useState(false);
  // Accounts whose cookies "Export cookies" writes: null for all, undefined while closed.
  const [exportIds, setExportIds] = useState<number[] | null | undefined>(undefined);
  const [order, setOrder] = useState<number[] | null>(null);
  const [now, tick] = useReducer(() => Date.now(), Date.now());
  const compact = density === "compact";
  const instances = useInstances().data;
  // An account with a client on its way or in a game can't start another.
  const live = useMemo(() => new Map((instances ?? []).filter(isLive).map((i) => [i.accountId, i])), [instances]);
  if (!narrow && sheet) setSheet(false);

  useEffect(() => {
    const timer = setInterval(tick, 60_000);
    return () => clearInterval(timer);
  }, []);
  // A local order bridges the gap until the optimistic cache update arrives.
  useEffect(() => setOrder(null), [accounts]);
  // The room <main> has decides where the details go; measured before paint, then on every resize.
  useLayoutEffect(() => {
    const el = main.current;
    if (!el) return;
    setNarrow(el.offsetWidth < SHEET);
    const ro = new ResizeObserver(() => setNarrow(el.offsetWidth < SHEET));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const byId = useMemo(() => new Map(accounts.map((a) => [a.id, a])), [accounts]);
  const groupById = useMemo(() => new Map(groups.map((g) => [g.id, g])), [groups]);
  const ordered = order ? order.flatMap((id) => byId.get(id) ?? []) : accounts;
  // A group deleted while it was the filter means no filter.
  const inGroup = group === null || !groupById.has(group) ? ordered : ordered.filter((a) => a.groupIds.includes(group));
  const needle = q.trim().toLowerCase();
  const matching = inGroup.filter((a) => !needle || `${a.alias}\n${a.displayName}\n@${a.username}\n${a.userId}`.toLowerCase().includes(needle));
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
  const current = (selected !== null && byId.get(selected)) || shown[0] || null;
  // The account whose details show: none while the narrow layout's sheet is closed.
  const openId = narrow && !sheet ? undefined : current?.id;
  const needsSignIn = accounts.filter((a) => a.status === "invalid").length;
  const lastChecked = Math.max(0, ...accounts.map((a) => a.statusCheckedAt ?? 0)) || null;
  const activeGroup = group === null ? null : groupById.get(group) ?? null;
  const friendable = checkedIds.filter((id) => byId.get(id)?.status === "valid").length;
  const mainAccount = accounts.find((a) => a.main);
  const mainToBefriend = checkedIds.map((id) => byId.get(id)).find((a) => a?.main && a.status === "valid");
  if (mainSkipped && (checked.size === 0 || (mainAccount && checked.has(mainAccount.id)))) setMainSkipped(false);

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
  const runValidate = (ids?: number[]) => {
    if (!validate.isPending && accounts.length) validate.mutate(ids);
  };

  const befriend = useMutation({
    mutationFn: (ids: number[]) => api.accountsBefriend(ids),
    onSuccess: (r) => {
      const names = r.challenged.map((id) => byId.get(id)?.username).filter(Boolean).join(", ");
      toast(
        [
          t("accounts.toast.befriended", { count: r.added }),
          r.already && t("accounts.toast.alreadyFriends", { count: r.already }),
          r.failed && t("accounts.toast.befriendFailed", { count: r.failed }),
          names && t("accounts.toast.befriendCaptcha", { names }),
        ]
          .filter(Boolean)
          .join(" · "),
        r.failed ? "error" : "ok",
        r.challenged.length > 0,
      );
    },
  });

  const remove = useMutation({
    mutationFn: (ids: number[]) => api.accountsRemove(ids),
    onSuccess: (_, ids) => {
      // The focus goes to the row after the last one removed (else before it), not to a button that is gone.
      const keep = (a: AccountSummary) => !ids.includes(a.id);
      const last = shown.length - 1 - [...shown].reverse().findIndex((a) => !keep(a));
      const next = shown.slice(last + 1).find(keep) ?? shown.slice(0, last).reverse().find(keep);
      refocus.current = next?.id ?? null;
      setAccounts(qc, (list) => list.filter((a) => !ids.includes(a.id)));
      // Ids can be reused by the next account added: drop what belonged to these.
      for (const id of ids) qc.removeQueries({ queryKey: qk.info(id) });
      setChecked(new Set());
      setRemoveOpen(false);
      setMainConfirm(false);
      toast(t("accounts.toast.removed", { count: ids.length }));
    },
  });

  const reorder = useMutation({
    mutationFn: (ids: number[]) => api.accountsReorder(ids),
    onError: () => qc.invalidateQueries({ queryKey: qk.accounts }),
  });
  const applyOrder = (ids: number[]) => {
    setOrder(ids);
    setAccounts(qc, (list) => list.map((a) => ({ ...a, sortOrder: ids.indexOf(a.id) })));
    reorder.mutate(ids);
  };

  // Runs after the confirm's own close (a child's effect runs first), which gives the focus back to a
  // button that may be gone.
  const refocus = useRef<number | null | undefined>(undefined);
  useEffect(() => {
    if (refocus.current === undefined || removeOpen || mainConfirm) return;
    const id = refocus.current;
    refocus.current = undefined;
    const to = (id !== null && document.querySelector<HTMLElement>(`[data-name="${id}"]`)) || document.getElementById("account-filter") || document.querySelector<HTMLElement>("[data-rest]");
    to?.focus();
  }, [removeOpen, mainConfirm, accounts]);

  const mainToRemove = removeIds.map((id) => byId.get(id)).find((a) => a?.main);
  const askRemove = (ids: number[]) => {
    setRemoveIds(ids);
    setRemoveOpen(true);
  };

  // ---------- selection ----------

  // Select-all and Shift ranges leave the MAIN account out (Settings, protect MAIN); a hand tick still works.
  const protectMain = useSettings().data?.protectMain ?? true;
  const pickable = protectMain ? shown.filter((a) => !a.main) : shown;
  const shownChecked = pickable.filter((a) => checked.has(a.id)).length;
  const allShown = pickable.length > 0 && shownChecked === pickable.length;
  const skipsMain = pickable.length < shown.length;
  // The ticks change away from the focus: a screen reader is told the new count.
  const setTicks = (next: Set<number>) => {
    setChecked(next);
    announce(t("accounts.selected", { count: accounts.filter((a) => next.has(a.id)).length }));
  };
  const selectAll = () => {
    setTicks(new Set([...checked, ...pickable.map((a) => a.id)]));
    if (skipsMain) setMainSkipped(true);
  };
  const toggleAll = () => {
    if (!allShown) return selectAll();
    const n = new Set(checked);
    for (const a of shown) n.delete(a.id);
    setTicks(n);
  };
  const tickRow = (id: number, range: boolean) => {
    const on = !checked.has(id);
    const [a, b] = [shown.findIndex((x) => x.id === rangeFrom.current), shown.findIndex((x) => x.id === id)];
    let ids = [id];
    if (range && a >= 0 && b >= 0) {
      const span = shown.slice(Math.min(a, b), Math.max(a, b) + 1);
      ids = span.filter((x) => x.id === id || !(protectMain && x.main)).map((x) => x.id);
      if (on && ids.length < span.length) setMainSkipped(true);
    }
    rangeFrom.current = id;
    setChecked((s) => {
      const n = new Set(s);
      for (const x of ids) if (on) n.add(x);
      else n.delete(x);
      return n;
    });
  };
  const clear = () => checked.size > 0 && setTicks(new Set());
  // What a search or filter leaves, once the typing settles.
  const shownCount = shown.length;
  const filterKey = `${q}\n${filter}\n${group}`;
  const lastFilter = useRef(filterKey);
  useEffect(() => {
    if (lastFilter.current === filterKey) return;
    lastFilter.current = filterKey;
    const timer = setTimeout(() => announce(shownCount ? t("accounts.shown", { count: shownCount }) : t("accounts.noMatch")), 600);
    return () => clearTimeout(timer);
  }, [filterKey]); // eslint-disable-line react-hooks/exhaustive-deps

  // ---------- the details: docked beside the list, or a sheet over it when <main> is narrow ----------

  const open = (id: number) => {
    setSelected(id);
    if (narrow) setSheet(true);
  };
  const closeSheet = () => {
    const id = current?.id;
    setSheet(false);
    requestAnimationFrame(() => document.querySelector<HTMLElement>(`[data-name="${id}"]`)?.focus({ preventScroll: true }));
  };
  useEffect(() => {
    if (sheet) sheetRef.current?.focus({ preventScroll: true });
  }, [sheet]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // A dialog that took this Esc has closed already by now: it is not ours.
      if (e.defaultPrevented || overlayOpen()) return;
      const typing = isTyping(e.target);
      const ctrl = e.ctrlKey || e.metaKey;
      const k = e.key.toLowerCase();
      if (ctrl && k === "a" && !typing) {
        e.preventDefault();
        selectAll();
      } else if (ctrl && k === "r") {
        e.preventDefault();
        runValidate(checkedIds.length ? checkedIds : undefined);
      } else if (e.key === "Delete" && !typing) {
        const row = e.target instanceof Element ? e.target.closest<HTMLElement>("tr[data-id]") : null;
        const ids = checkedIds.length ? checkedIds : row ? [Number(row.dataset.id)] : openId !== undefined ? [openId] : [];
        if (ids.length) askRemove(ids);
      } else if (e.key === "Escape" && !typing) {
        // The sheet over the list closes first; the selection clears after.
        if (sheet) closeSheet();
        else clear();
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
  const clearFilters = () => {
    setQ("");
    setFilter("all");
    setGroup(null);
  };

  const panel = current && (
    // Another account: the panel cross-fades (the old one lets go of a revealed cookie at once).
    <AnimatePresence mode="popLayout">
      <motion.div
        key={current.id}
        className="flex-1 min-h-0 flex flex-col"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1, transition: TWEEN.fade }}
        exit={{ opacity: 0, transition: TWEEN.exit }}
      >
        <DetailPanel
          account={current}
          groups={groups}
          vaultMode={vaultMode}
          clipboardSeconds={clipboardSeconds}
          checking={isChecking(current.id)}
          instance={live.get(current.id)}
          tab={tab}
          onTab={setTab}
          onValidate={() => runValidate([current.id])}
          onRemove={() => askRemove([current.id])}
          onLaunch={() => onLaunch([current.id])}
          menuOpen={menu?.solo === true && menu.id === current.id}
          onMenu={(from) => {
            const r = from.getBoundingClientRect();
            setMenu({ id: current.id, x: r.left, y: r.bottom + 4, from, solo: true });
          }}
          onClose={narrow ? closeSheet : undefined}
        />
      </motion.div>
    </AnimatePresence>
  );
  // Pins the panel to its account once it is used, so an edit that drops it from the filtered list
  // can't swap the panel to another account under the next click.
  const pin = () => current && setSelected(current.id);
  const away = (isRtl() ? -1 : 1) * (reduced ? 0 : 32);

  return (
    <>
      <main ref={main} className={`@container scroll flex-1 min-w-0 page pb-6 flex flex-col gap-4 ${compact ? "compact" : ""}`}>
        <header className="flex flex-wrap items-start gap-x-6 gap-y-3">
          <div className="min-w-0 pt-1">
            <h1 className="page-title">{t("accounts.title")}</h1>
            <p className="page-sub flex flex-wrap items-center gap-x-3 gap-y-1">
              <span>
                <Counted k="accounts.total" n={accounts.length} />
              </span>
              {needsSignIn > 0 && (
                <button
                  type="button"
                  className="status-pill acc-need"
                  aria-pressed={filter === "invalid"}
                  onClick={() => {
                    if (filter === "invalid") return setFilter("all");
                    clearFilters();
                    setFilter("invalid");
                  }}
                >
                  <span aria-hidden="true" className="status-mark" data-shape="diamond" />
                  <span className="text-fg">
                    <Counted k="accounts.toast.needSignIn" n={needsSignIn} />
                  </span>
                </button>
              )}
            </p>
          </div>
          <AccountsToolbar
            total={accounts.length}
            selected={checkedIds}
            groups={groups}
            playable={checkedIds.filter(canPlay).length}
            friendable={friendable}
            mainSkipped={mainSkipped}
            checking={checking !== undefined}
            befriending={befriend.isPending}
            onCheckAll={() => runValidate()}
            onCheck={() => runValidate(checkedIds)}
            onExport={setExportIds}
            onAdd={onAdd}
            onPlay={() => onLaunch(checkedIds)}
            onBefriend={() => setBefriendOpen(true)}
            onAddToGroup={addToGroup}
            onRemove={() => askRemove(checkedIds)}
            onClear={clear}
          />
        </header>

        {accounts.length === 0 ? (
          <>
            {/* Left over from accounts that are gone: they can still be edited or deleted. */}
            {groups.length > 0 && (
              <div className="flex flex-wrap items-center gap-2">
                <GroupChips groups={groups} accounts={accounts} active={activeGroup?.id ?? null} onPick={setGroup} />
              </div>
            )}
            <Empty onAdd={onAdd} />
          </>
        ) : (
          // Positioned: the narrow layout's sheet covers the filters and the list, never the header's pill.
          <div className="relative flex-1 min-h-0 flex flex-col gap-4">
            {/* Under the open sheet the filters and the list are out of reach (Tab stays in the sheet). */}
            <div className="flex flex-wrap items-center gap-2" inert={(narrow && sheet) || undefined}>
              <div className="acc-search relative flex-[1_1_220px] min-w-[200px] max-w-[320px] flex">
                <Input
                  ref={filterInput}
                  id="account-filter"
                  data-find
                  leading="search"
                  aria-label={t("accounts.filterLabel")}
                  placeholder={t("accounts.filterPlaceholder")}
                  value={q}
                  onChange={(e) => setQ(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape" && q) {
                      e.stopPropagation();
                      setQ("");
                    }
                  }}
                />
                <span className="absolute inset-y-0 end-1.5 grid place-items-center">
                  <Swap k={q ? "clear" : "keys"} variant="blur">
                    {q ? (
                      <IconButton
                        icon="x"
                        size="sm"
                        label={t("accounts.clearSearch")}
                        onClick={() => {
                          setQ("");
                          filterInput.current?.focus();
                        }}
                      />
                    ) : (
                      <span aria-hidden="true" className="acc-keys pe-1">
                        <Kbd keys="Ctrl+F" />
                      </span>
                    )}
                  </Swap>
                </span>
              </div>
              <button type="button" className="chip" popoverTarget="status-filter" style={statusMenu.style} data-on={filter !== "all" || undefined} aria-haspopup="menu">
                <Icon name="sliders" size={14} />
                {filter === "all" ? t("accounts.filter.status") : t(FILTERS.find((f) => f.key === filter)!.label)}
                <Icon name="chevron" size={12} />
              </button>
              <Popover id="status-filter" anchor={statusMenu.name} role="menu" label={t("accounts.filter.status")} onToggle={focusFirstItem("status-filter")}>
                {FILTERS.map((f) => (
                  <MenuRadioItem
                    key={f.key}
                    checked={filter === f.key}
                    onSelect={() => {
                      setFilter(f.key);
                      document.getElementById("status-filter")?.hidePopover();
                    }}
                  >
                    <span className="flex items-baseline justify-between gap-4">
                      <span className="truncate">{t(f.label)}</span>
                      <span className="text-[12px] text-fg-3 tabular-nums">{counts(f.key)}</span>
                    </span>
                  </MenuRadioItem>
                ))}
              </Popover>
              <GroupChips groups={groups} accounts={accounts} active={activeGroup?.id ?? null} onPick={setGroup} />
            </div>

            <div className="flex-[1_1_240px] min-h-0 flex" inert={(narrow && sheet) || undefined}>
              <div className="panel flex-1 min-w-0 flex flex-col overflow-hidden">
                <AccountsTable
                  rows={shown}
                  ordered={ordered}
                  groupById={groupById}
                  presence={presence}
                  compact={compact}
                  now={now}
                  currentId={openId}
                  checked={checked}
                  all={allShown}
                  some={shownChecked > 0}
                  live={live}
                  isChecking={isChecking}
                  onTick={tickRow}
                  onTickAll={toggleAll}
                  onOpen={open}
                  onMenu={(id, x, y, from) => {
                    setSelected(id);
                    setMenu({ id, x, y, from });
                  }}
                  onLaunch={(id) => onLaunch([id])}
                  menuId={menu && !menu.solo ? menu.id : undefined}
                  onReorder={applyOrder}
                  onClearFilters={clearFilters}
                />
                <footer className="h-9 flex-none flex items-center gap-4 px-4 meta border-t border-line">
                  <span>{lastChecked ? t("accounts.footerChecked", { when: ago(lastChecked, now) }) : t("accounts.footerNotChecked")}</span>
                  <span className="mono ms-auto">v{import.meta.env.APP_VERSION}</span>
                </footer>
              </div>
              {!narrow && current && (
                <>
                  <Divider panel={detail} side="right" label={t("accounts.detailsWidth")} />
                  <aside
                    aria-label={t("accounts.details")}
                    onFocusCapture={pin}
                    style={{ width: detail.width, maxWidth: "40cqw" }}
                    className="panel relative flex-none ms-3 overflow-hidden flex flex-col"
                  >
                    {panel}
                  </aside>
                </>
              )}
            </div>
            <AnimatePresence>
              {narrow && sheet && current && (
                <motion.aside
                  key="sheet"
                  ref={sheetRef}
                  tabIndex={-1}
                  aria-label={t("accounts.details")}
                  onFocusCapture={pin}
                  className="acc-sheet"
                  initial={{ opacity: 0, x: away }}
                  animate={{ opacity: 1, x: 0, transition: SPRING.gentle }}
                  exit={{ opacity: 0, x: away / 2, transition: TWEEN.exit }}
                >
                  {panel}
                </motion.aside>
              )}
            </AnimatePresence>
          </div>
        )}
      </main>

      <AccountMenu
        at={menu}
        account={menu ? byId.get(menu.id) : undefined}
        selection={checkedIds}
        live={live}
        canLaunch={canPlay}
        onClose={() => setMenu(null)}
        onLaunch={onLaunch}
        onValidate={runValidate}
        onRemove={askRemove}
      />
      <ConfirmDialog
        open={removeOpen}
        onClose={() => setRemoveOpen(false)}
        onConfirm={() => {
          // The MAIN goes only after a second, explicit confirm.
          if (!mainToRemove) return remove.mutate(removeIds);
          setRemoveOpen(false);
          setMainConfirm(true);
        }}
        busy={remove.isPending}
        title={t("accounts.removeTitle", { count: removeIds.length })}
        text={
          <>
            {removeIds.length === 1
              ? tx("accounts.removeOneText", {
                  name: <span className="text-fg font-medium">{byId.has(removeIds[0]) ? accountName(byId.get(removeIds[0])!) : t("accounts.thisAccount")}</span>,
                })
              : t("accounts.removeManyText", { count: removeIds.length })}
            {mainToRemove && <p className="alert mt-2">{t("accounts.removeMainWarning", { name: `@${mainToRemove.username}` })}</p>}
          </>
        }
        confirmLabel={t("common.remove")}
      />
      <ConfirmDialog
        open={mainConfirm}
        onClose={() => setMainConfirm(false)}
        onConfirm={() => remove.mutate(removeIds)}
        busy={remove.isPending}
        title={t("accounts.removeMainTitle")}
        text={t("accounts.removeMainConfirm", { name: `@${mainToRemove?.username ?? ""}` })}
        confirmLabel={t("common.remove")}
      />
      <ConfirmDialog
        open={befriendOpen}
        onClose={() => setBefriendOpen(false)}
        onConfirm={() => {
          setBefriendOpen(false);
          befriend.mutate(checkedIds);
        }}
        busy={befriend.isPending}
        title={t("accounts.befriendTitle")}
        text={
          <>
            {t("accounts.befriendText", { count: friendable, pairs: (friendable * (friendable - 1)) / 2 })}
            {mainToBefriend && <p className="alert mt-2">{t("accounts.removeMainWarning", { name: `@${mainToBefriend.username}` })}</p>}
          </>
        }
        confirmLabel={t("accounts.befriend")}
      />
      <ExportCookies ids={exportIds} accounts={accounts} vaultMode={vaultMode} onClose={() => setExportIds(undefined)} />
    </>
  );
}

function Empty({ onAdd }: { onAdd: () => void }) {
  return (
    <div className="enter flex-1 grid place-items-center p-8">
      <div className="empty max-w-sm">
        <span className="num w-12 h-12 text-fg-2">
          <Icon name="accounts" size={20} />
        </span>
        <h2 className="section-title text-fg">{t("accounts.emptyTitle")}</h2>
        <p className="text-pretty">{t("accounts.emptyText")}</p>
        {/* The header pill's Add account is this screen's one primary. */}
        <Button icon="plus" onClick={onAdd}>
          {t("accounts.add.title")}
        </Button>
      </div>
    </div>
  );
}
