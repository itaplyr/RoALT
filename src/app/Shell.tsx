import { Fragment, lazy, startTransition, Suspense, useCallback, useEffect, useLayoutEffect, useRef, useState, ViewTransition, type ComponentType } from "react";
import { createPortal } from "react-dom";
import { useQueryClient } from "@tanstack/react-query";
import { LoadFailed, toast } from "../ui/Toast";
import type { Favourite, SameServer, Settings, VaultStatus } from "../api";
import { qk, useAccounts, useGroups, useInstances } from "../data";
import { t } from "../i18n";
import { AddAccountDialog, type Tab as AddTab } from "../features/accounts/AddAccountDialog";
import { AccountRail } from "../features/dashboard/AccountRail";
import { DashboardView } from "../features/dashboard/DashboardView";
import { useHomeTabs, useSectionOrder } from "../features/dashboard/homeLayout";
import { LaunchFlight } from "../features/launch/LaunchButton";
import { lastLaunchAccount } from "../features/launch/launchQueue";
import { isLive } from "../features/launch/Running";
import { draftFor, EMPTY_DRAFT, type Draft } from "../features/launch/target";
import { AREAS, type SettingsArea } from "../features/settings/areas";
import { jumpTo } from "../features/settings/jumpTo";
import { useSplashPhase } from "../features/splash/phase";
import { overlayOpen } from "../ui/a11y";
import { Island } from "../ui/island/Island";
import type { IslandLeaf, IslandModel } from "../ui/island/model";
import { markViewEntering } from "../ui/motion";
import { Divider, usePanelWidth } from "../ui/Resizer";
import { LiveActivities } from "./LiveActivities";
import { MAIN_SECTIONS, requestNavigation, SECTION_ICON, SECTION_VIEW, SHORTCUT_VIEW, shortcutFor, TOOL_SECTIONS, useNavigationRequests, VIEW_NAME, type BrowseRequest, type Request, type Section, type View } from "./navigation";

// Home is the start; every other view is its own chunk, fetched once Home has painted.
const viewChunks: (() => Promise<unknown>)[] = [];
function lazyView<M, C extends ComponentType<any>>(load: () => Promise<M>, pick: (m: M) => C) {
  viewChunks.push(load);
  return lazy(() => load().then((m) => ({ default: pick(m) })));
}
const AccountsView = lazyView(() => import("../features/accounts/AccountsView"), (m) => m.AccountsView);
const BrowseView = lazyView(() => import("../features/browse/BrowseView"), (m) => m.BrowseView);
const DebugView = lazyView(() => import("../features/debug/DebugView"), (m) => m.DebugView);
const DiscordView = lazyView(() => import("../features/discord/DiscordView"), (m) => m.DiscordView);
const LaunchView = lazyView(() => import("../features/launch/LaunchView"), (m) => m.LaunchView);
const MonitorView = lazyView(() => import("../features/monitor/MonitorView"), (m) => m.MonitorView);
const PerformanceView = lazyView(() => import("../features/performance/PerformanceView"), (m) => m.PerformanceView);
const ServersView = lazyView(() => import("../features/servers/ServersView"), (m) => m.ServersView);
const SettingsView = lazyView(() => import("../features/settings/SettingsView"), (m) => m.SettingsView);

const MAIN = ".stage main";
/** Numbers Browse's asks; outlives the shell (a lock unmounts it), as Browse's "already followed" does. */
let browseSeq = 0;

/** Focuses the first match and says whether there was one: a view's <main> becomes focusable for
 * it, and a field's text is selected. */
function focusIn(selector: string) {
  const el = document.querySelector<HTMLElement>(selector);
  if (!el) return false;
  if (el.matches("main")) el.setAttribute("tabindex", "-1");
  el.focus({ preventScroll: true });
  if (el instanceof HTMLInputElement) el.select();
  return true;
}

/** What the shell shows once the vault is open: the island (in the title bar), the account rail in the
 * side layout, and one view at a time. Owns the state that must outlive a view: picked accounts, the
 * draft target, Home's tabs, a launch in flight. */
export function Shell({ vault, settings, islandSlot }: { vault: VaultStatus; settings: Settings; islandSlot: HTMLElement | null }) {
  const nav = usePanelWidth("nav", 228, 180, 320);
  const qc = useQueryClient();
  const accounts = useAccounts();
  const groups = useGroups();
  const instances = useInstances().data ?? [];
  const [view, setView] = useState<View>("dashboard");
  const [area, setArea] = useState<SettingsArea>("launching");
  const [group, setGroup] = useState<number | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  // The Launch view's accounts; the last one launched until something else is picked.
  const [picked, setPicked] = useState<number[]>(() => {
    const id = lastLaunchAccount();
    return id === null ? [] : [id];
  });
  const [sameServer, setSameServer] = useState<SameServer>("off");
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  const homeTabs = useHomeTabs(accounts.data);
  const [sectionOrder, setSectionOrder] = useSectionOrder();
  // The game the Servers view shows.
  const [serversPlace, setServersPlace] = useState<number | null>(null);
  // What Browse was last asked to show (an account's "Open on roblox.com"); `n` tells two alike apart.
  const [browseAsk, setBrowseAsk] = useState<(BrowseRequest & { n: number }) | null>(null);
  const [adding, setAdding] = useState(false);
  const [addOn, setAddOn] = useState<AddTab>("login");
  const phase = useSplashPhase();
  // What takes the focus once that view is on screen: its <main> (a shortcut), or a field. Tied to
  // the view: a switch still pending when the next one is asked for commits first, and must not use it.
  const focusNext = useRef<{ view: View; to: string } | null>(null);
  // The view last asked for; a view switch is a transition, so it may not be on screen yet.
  const wanted = useRef<View>(view);
  const known = useRef<Set<number> | null>(null);
  // The Play target's game last handed to Servers.
  const seeded = useRef<number | null>(null);
  const viewName = t(VIEW_NAME[view]);

  useEffect(() => {
    document.title = `${viewName} · RoALT`;
  }, [viewName]);

  // So a first visit doesn't wait on its chunk (a switch is a transition: the old view stays meanwhile).
  useEffect(() => {
    const load = () => viewChunks.forEach((l) => void l());
    // The website demo also runs in Safari, which has no requestIdleCallback before 18.4.
    if (!("requestIdleCallback" in window)) {
      const timer = setTimeout(load, 1000);
      return () => clearTimeout(timer);
    }
    const idle = requestIdleCallback(load, { timeout: 3000 });
    return () => cancelIdleCallback(idle);
  }, []);

  // A new account needs its headshot; the backend only asks Roblox for the missing ones.
  useEffect(() => {
    if (!accounts.data) return;
    const ids = new Set(accounts.data.map((a) => a.id));
    if (known.current && [...ids].some((id) => !known.current?.has(id))) qc.invalidateQueries({ queryKey: qk.headshots });
    known.current = ids;
  }, [accounts.data, qc]);

  /** Switches the view: the content cross-fades (a view transition), the island and rail stay live.
   * `focus` (a selector): what takes the focus once the view is on screen. */
  const go = (next: View, opts: { focus?: string } = {}) => {
    if (next === view && next === wanted.current) {
      if (opts.focus) focusIn(opts.focus);
      return;
    }
    focusNext.current = opts.focus ? { view: next, to: opts.focus } : null;
    if (next === wanted.current) return;
    wanted.current = next;
    markViewEntering();
    startTransition(() => setView(next));
  };

  useLayoutEffect(() => {
    const next = focusNext.current;
    if (next?.view !== view) return;
    focusNext.current = null;
    focusIn(next.to);
  }, [view]);

  const openArea = (a: SettingsArea) => {
    setArea(a);
    go("settings");
  };

  const openAdd = (tab: AddTab = "login") => {
    setAddOn(tab);
    setAdding(true);
  };

  // Opens on Play's game whenever Play's target moved to another game since; a game loaded in
  // Servers stays otherwise.
  const openServers = (opts: { focus?: string } = {}) => {
    const p = draft.parsed?.text === draft.text ? draft.parsed.value : null;
    const target = p?.kind === "target" ? p.target : null;
    if (target && "placeId" in target && target.placeId !== seeded.current) {
      seeded.current = target.placeId;
      setServersPlace(target.placeId);
    }
    go("servers", opts);
  };

  // Asked for from outside the shell (the app menu, an island sheet).
  const handlers = useRef({ go, openArea });
  handlers.current = { go, openArea };
  useNavigationRequests(
    useCallback((r: Request) => {
      if (r.view === "settings" && r.area) handlers.current.openArea(r.area);
      else {
        const browse = r.browse;
        if (browse) setBrowseAsk({ ...browse, n: ++browseSeq });
        handlers.current.go(r.view);
      }
    }, []),
  );

  // Re-bound each render: Ctrl+4 needs the current draft. Alt is left alone (AltGr types characters).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (overlayOpen()) return;
      const typing = e.target instanceof HTMLElement && e.target.matches("input, textarea, select, [contenteditable]");
      if (e.key === "?" && !typing && !e.ctrlKey && !e.altKey) {
        e.preventDefault();
        // As a request, so Settings also ends a search when it is already on Shortcuts.
        requestNavigation("settings", "shortcuts");
        return;
      }
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      const k = e.key.toLowerCase();
      const to = SHORTCUT_VIEW[e.code];
      if (to) {
        e.preventDefault();
        // A field saves on blur: what was typed goes with it, not lost with the view.
        if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
        if (to === "servers") openServers({ focus: MAIN });
        else go(to, { focus: MAIN });
      } else if (k === "n") {
        e.preventDefault();
        openAdd();
      } else if (k === "f") {
        e.preventDefault();
        // The current view's own search first (Settings, Debug, Accounts), else the accounts filter.
        if (!focusIn(".stage main [data-find], #settings-search, #account-filter")) go("accounts", { focus: "#account-filter" });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const failed = accounts.error ?? groups.error;
  if (failed) return <LoadFailed error={failed} busy={accounts.isFetching || groups.isFetching} onRetry={() => { accounts.refetch(); groups.refetch(); }} />;
  if (!accounts.data || !groups.data) return <div className="flex-1" />;

  const showAll = () => {
    go("accounts");
    setGroup(null);
  };
  /** Opens Launch with these accounts picked; ones that can't launch (signed out, already
   * playing) are left out, and with none left nothing changes. */
  const launchAs = (ids: number[]) => {
    const live = new Set(instances.filter(isLive).map((i) => i.accountId));
    const valid = ids.filter((id) => !live.has(id) && accounts.data?.some((a) => a.id === id && a.status === "valid"));
    if (valid.length < ids.length) toast(t("app.leftOut", { count: ids.length - valid.length }));
    if (!valid.length) return;
    setPicked(valid);
    go("launch");
  };
  const pickFavourite = (f: Favourite) => {
    setDraft(draftFor(f.target, f.id, f.name));
    const valid = f.accountIds.filter((id) => accounts.data?.some((a) => a.id === id && a.status === "valid"));
    if (valid.length) setPicked(valid);
    go("launch");
  };

  const leaf = (s: Section): IslandLeaf => {
    const v = SECTION_VIEW[s];
    return {
      id: s,
      icon: SECTION_ICON[s],
      label: t(VIEW_NAME[v]),
      shortcut: shortcutFor(v),
      current: view === v,
      count: s === "accounts" && accounts.data!.length > 0 ? accounts.data!.length : undefined,
      onSelect: () => (s === "servers" ? openServers() : s === "accounts" ? showAll() : go(v)),
    };
  };
  const model: IslandModel = {
    label: t("app.sections"),
    main: MAIN_SECTIONS.map(leaf),
    mainOrder: { ids: sectionOrder, onOrder: setSectionOrder },
    groups: [
      { id: "tools", icon: "tools", label: t("app.nav.moreTools"), children: TOOL_SECTIONS.map(leaf), order: { ids: sectionOrder, onOrder: setSectionOrder } },
      {
        id: "settings",
        icon: "settings",
        label: t("app.nav.settings"),
        keepIcon: true,
        children: AREAS.map((a) => ({
          id: a.id,
          icon: a.icon,
          label: t(a.label),
          shortcut: a.id === "launching" ? shortcutFor("settings") : undefined,
          current: view === "settings" && area === a.id,
          // A request (Shell runs openArea for it): Settings also ends a search for the area on screen.
          onSelect: () => requestNavigation("settings", a.id),
        })),
      },
    ],
  };

  const showViewTransition = (window as { __ROALT_VIEW_TRANSITIONS?: boolean }).__ROALT_VIEW_TRANSITIONS !== false;
  // WebKitGTK with software compositing aborts inside a view transition (2.54). Fragment skips it.
  const Transition = (showViewTransition ? ViewTransition : Fragment) as typeof ViewTransition;

  const side = settings.tabLayout === "side";
  return (
    <div className="shell">
      <span className="sr-only" aria-live="polite">{viewName}</span>
      {islandSlot &&
        phase !== "play" &&
        createPortal(<Island model={model} live={<LiveActivities accounts={accounts.data} settings={settings} onPick={launchAs} />} />, islandSlot)}
      {/* Where a launch button's pill flies from into the island; outlives the view that launched. */}
      <LaunchFlight />
      {side && (
        <>
          <AccountRail
            width={nav.width}
            accounts={accounts.data}
            instances={instances}
            home={homeTabs}
            onHome={view === "dashboard"}
            onOpen={() => go("dashboard")}
            onAdd={() => openAdd()}
          />
          <Divider panel={nav} side="left" label={t("app.sidebarWidth")} />
        </>
      )}
      <div className="stage">
        {/* Outside the keyed transition: a view whose chunk is still on its way never flashes an empty stage. */}
        <Suspense fallback={null}>
        <Transition key={view} {...(showViewTransition ? { enter: "view-in", exit: "view-out", update: "none", default: "none" } : {})}>
          {view === "dashboard" ? (
            <DashboardView
              accounts={accounts.data}
              home={homeTabs}
              onAdd={() => openAdd()}
              onImport={() => openAdd("many")}
              onAccounts={showAll}
              onPickFavourite={pickFavourite}
            />
          ) : view === "accounts" ? (
            <AccountsView
              accounts={accounts.data}
              groups={groups.data}
              density={settings.density}
              vaultMode={vault.mode}
              clipboardSeconds={settings.clipboardClearSeconds}
              group={group}
              setGroup={setGroup}
              selected={selected}
              setSelected={setSelected}
              onAdd={() => openAdd()}
              onLaunch={launchAs}
            />
          ) : view === "launch" ? (
            <LaunchView
              accounts={accounts.data}
              picked={picked}
              setPicked={setPicked}
              sameServer={sameServer}
              setSameServer={setSameServer}
              draft={draft}
              setDraft={setDraft}
              onAdd={() => openAdd()}
              onServers={openServers}
              onPickFavourite={pickFavourite}
              onHome={() => go("dashboard")}
              onAccounts={showAll}
              onSettings={() => jumpTo("client")}
            />
          ) : view === "browse" ? (
            <BrowseView accounts={accounts.data} ask={browseAsk} />
          ) : view === "servers" ? (
            <ServersView
              accounts={accounts.data}
              picked={picked}
              setPicked={setPicked}
              place={serversPlace}
              setPlace={setServersPlace}
            />
          ) : view === "monitor" ? (
            <MonitorView accounts={accounts.data} />
          ) : view === "discord" ? (
            <DiscordView />
          ) : view === "debug" ? (
            <DebugView />
          ) : view === "performance" ? (
            <PerformanceView />
          ) : (
            <SettingsView area={area} />
          )}
        </Transition>
        </Suspense>
      </div>

      <AddAccountDialog
        open={adding}
        startOn={addOn}
        onClose={() => setAdding(false)}
        onAdded={(a) => {
          showAll();
          setSelected(a.id);
        }}
      />
    </div>
  );
}
