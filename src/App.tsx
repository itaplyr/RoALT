import { useEffect, useRef, useState, type ReactNode } from "react";
import { QueryClientProvider, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, asError, events, type Favourite, type SameServer, type Settings, type VaultStatus } from "./api";
import { qk, queryClient, useAccounts, useGroups, useInstances, useSetSettings, useSettings, useVault } from "./data";
import { AccountsView } from "./features/accounts/AccountsView";
import { AddAccountDialog, type Tab as AddTab } from "./features/accounts/AddAccountDialog";
import { AccountRail, SectionTabs } from "./features/dashboard/AccountRail";
import { DashboardView } from "./features/dashboard/DashboardView";
import { useHomeTabs, useSectionOrder } from "./features/dashboard/homeLayout";
import { Sortable, type DragProps } from "./features/dashboard/Sortable";
import { DiscordView } from "./features/discord/DiscordView";
import { PerformanceView } from "./features/performance/PerformanceView";
import { DebugView } from "./features/debug/DebugView";
import { lastLaunchAccount, LaunchView } from "./features/launch/LaunchView";
import { DonateDialog } from "./features/donate/DonateDialog";
import { isLive, isRunning, KillAllButton } from "./features/launch/Running";
import { draftFor, EMPTY_DRAFT, type Draft } from "./features/launch/target";
import { MonitorView } from "./features/monitor/MonitorView";
import { ServersView } from "./features/servers/ServersView";
import { SettingsView, SETTINGS_SECTIONS } from "./features/settings/SettingsView";
import { Splash } from "./features/splash/Splash";
import { UpdateBanner } from "./features/settings/UpdateBanner";
import { PublicNetworkBanner } from "./features/settings/PublicNetworkBanner";
import { RecoveryCodeDialog } from "./features/vault/RecoveryCodeDialog";
import { UnlockScreen, type LockReason } from "./features/vault/UnlockScreen";
import { setLang, t } from "./i18n";
import { Icon } from "./ui/Icon";
import { Divider, usePanelWidth } from "./ui/Resizer";
import { TitleBar } from "./ui/TitleBar";
import { friendly, toast, toastError, Toaster } from "./ui/Toast";

/** Last applied theme, read before first paint so a light-theme start does not flash dark. */
export const THEME_KEY = "roalt.theme";

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <Root />
      <Toaster />
      <Splash />
    </QueryClientProvider>
  );
}

function Root() {
  const qc = useQueryClient();
  const vault = useVault();
  const settings = useSettings();
  const setSettings = useSetSettings();
  const dark = useTheme(settings.data);
  const [lockReason, setLockReason] = useState<LockReason>("start");
  const [recoveryCode, setRecoveryCode] = useState<string | null>(null);
  const manualLockAt = useRef(0);
  const v = vault.data;
  const s = settings.data;
  // Before any child renders, so the whole tree uses the chosen language.
  setLang(s?.language);
  const unlocked = !!v && !v.locked;

  const markLocked = () => {
    qc.setQueryData<VaultStatus>(qk.vault, (old) => old && { ...old, locked: true });
    for (const queryKey of [qk.accounts, qk.presence, ["info"], qk.favourites, qk.join, qk.privateServers, qk.discord]) qc.removeQueries({ queryKey });
  };

  // Everything on screen asks again; the webview itself is never reloaded.
  const refresh = () => void qc.invalidateQueries();

  const lock = () => {
    manualLockAt.current = Date.now();
    setLockReason("manual");
    api.vaultLock().then(markLocked, toastError);
  };

  useEffect(() => {
    const subs = [
      events.onAccountsChanged(() => {
        for (const queryKey of [qk.accounts, qk.groups, qk.vault]) qc.invalidateQueries({ queryKey });
      }),
      events.onInstancesChanged(() => {
        for (const queryKey of [qk.instances, qk.multi, qk.crashes]) qc.invalidateQueries({ queryKey });
      }),
      events.onBulkProgress((p) => qc.setQueryData(qk.bulk, p)),
      events.onRobloxClient((c) => qc.setQueryData(qk.client, c)),
      events.onNetwork((n) => qc.setQueryData(qk.network, n)),
      events.onAppUpdate((u) => qc.setQueryData(qk.appUpdate, u)),
      // Errors stay until dismissed: the window may have been hidden in the tray.
      events.onNotice((n) => toast(n.message, n.error ? "error" : "ok", n.error)),
      events.onVaultLocked(() => {
        if (Date.now() - manualLockAt.current > 3000) setLockReason("idle");
        markLocked();
        qc.invalidateQueries({ queryKey: qk.vault });
      }),
    ];
    return () => subs.forEach((p) => p.then((un) => un()));
  }, [qc]);

  useActivity(unlocked);

  // Keyboard use: when what had focus went away (a menu item, a button that disabled itself),
  // focus goes to the page, not to nowhere, where the next Tab starts over.
  useEffect(() => {
    let keys = false;
    const key = () => void (keys = true);
    const pointer = () => void (keys = false);
    const lost = () =>
      requestAnimationFrame(() => {
        if (!keys || document.activeElement !== document.body) return;
        const to = document.querySelector<HTMLElement>("dialog[open]") ?? document.querySelector<HTMLElement>("main");
        to?.setAttribute("tabindex", "-1");
        to?.focus({ preventScroll: true });
      });
    window.addEventListener("keydown", key, true);
    window.addEventListener("pointerdown", pointer, true);
    window.addEventListener("focusout", lost);
    return () => {
      window.removeEventListener("keydown", key, true);
      window.removeEventListener("pointerdown", pointer, true);
      window.removeEventListener("focusout", lost);
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const ctrl = e.ctrlKey || e.metaKey;
      const k = e.key.toLowerCase();
      if ((ctrl && k === "r") || e.key === "F5") e.preventDefault(); // never reload the webview
      if (e.key === "F5" && unlocked) refresh();
      if (ctrl && k === "l" && unlocked && v?.mode === "master") {
        e.preventDefault();
        lock();
      }
    };
    // The WebView2 menu (Reload, Inspect) has no place in the app; fields keep theirs.
    const onMenu = (e: MouseEvent) => {
      if (import.meta.env.PROD && !(e.target instanceof Element && e.target.closest("input, textarea, .selectable"))) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("contextmenu", onMenu);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("contextmenu", onMenu);
    };
  });

  const failed = vault.error ?? settings.error;
  return (
    <div className="h-full flex flex-col">
      <TitleBar
        dark={dark}
        onToggleTheme={s ? () => setSettings.mutate({ theme: dark ? "light" : "dark" }) : undefined}
        onLock={unlocked && v.mode === "master" ? lock : undefined}
        onRefresh={unlocked ? refresh : undefined}
        sideTabs={s?.tabLayout === "side"}
        onToggleTabs={unlocked && s ? () => setSettings.mutate({ tabLayout: s.tabLayout === "side" ? "top" : "side" }) : undefined}
      />
      {unlocked && s && !failed && <UpdateBanner />}
      {s && !failed && <PublicNetworkBanner settings={s} />}
      {failed ? (
        <div className="flex-1 grid place-items-center p-8">
          <div className="flex flex-col items-start gap-3 max-w-sm">
            <h1 className="text-[15px] font-semibold">{t("app.startFailed")}</h1>
            <p className="text-dim">{friendly(asError(failed))}</p>
            <button className="btn" onClick={() => qc.invalidateQueries()}>{t("common.tryAgain")}</button>
          </div>
        </div>
      ) : !v || !s ? (
        <div className="flex-1" />
      ) : v.locked ? (
        <UnlockScreen status={v} reason={lockReason} onRecovered={setRecoveryCode} />
      ) : (
        <Shell vault={v} settings={s} onRecoveryCode={setRecoveryCode} />
      )}
      {/* Up here because it must outlive what opened it: the old code is void by now, and a recovered unlock swaps the screen below. Hidden while locked: it would open the vault for whoever comes by. */}
      <RecoveryCodeDialog code={unlocked ? recoveryCode : null} onClose={() => setRecoveryCode(null)} />
    </div>
  );
}

const MAIN_SECTIONS = ["home", "play", "accounts", "performance", "debug"] as const;
const TOOL_SECTIONS = ["servers", "monitor", "discord"] as const;
type Section = (typeof MAIN_SECTIONS)[number] | (typeof TOOL_SECTIONS)[number] | "settings";
const SECTION_NAMES = {
  home: "app.nav.dashboard", play: "app.nav.launch", accounts: "app.nav.accounts", performance: "app.nav.performance", debug: "app.nav.debug",
  servers: "app.nav.servers", monitor: "app.nav.monitor", discord: "app.nav.discord", settings: "app.nav.settings",
} as const;

function Shell({ vault, settings, onRecoveryCode }: { vault: VaultStatus; settings: Settings; onRecoveryCode: (code: string) => void }) {
  const nav = usePanelWidth("nav", 228, 180, 320);
  const qc = useQueryClient();
  const accounts = useAccounts();
  const groups = useGroups();
  const instances = useInstances().data ?? [];
  const running = instances.filter(isRunning).length;
  const [view, setView] = useState<"dashboard" | "accounts" | "launch" | "performance" | "servers" | "monitor" | "discord" | "debug" | "settings">("dashboard");
  const [donating, setDonating] = useState(false);
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
  // Kept here so a launch in flight survives leaving the view.
  const [launchStep, setLaunchStep] = useState<string | null>(null);
  // The game the Servers view shows.
  const [serversPlace, setServersPlace] = useState<number | null>(null);
  const [adding, setAdding] = useState(false);
  const [addOn, setAddOn] = useState<AddTab>("login");
  const [toolsOpen, setToolsOpen] = useState(false);
  // A tool that is open stays in sight.
  const showTools = toolsOpen || view === "servers" || view === "monitor" || view === "discord";
  const openAdd = (tab: AddTab = "login") => {
    setAddOn(tab);
    setAdding(true);
  };
  const known = useRef<Set<number> | null>(null);
  const viewName = t(({ dashboard: "app.nav.dashboard", accounts: "app.nav.accounts", launch: "app.nav.launch", performance: "app.nav.performance", servers: "app.nav.servers", monitor: "app.nav.monitor", discord: "app.nav.discord", debug: "app.nav.debug", settings: "app.nav.settings" } as const)[view]);
  useEffect(() => {
    document.title = `${viewName} · RoALT`;
  }, [viewName]);

  // A new account needs its headshot; the backend only asks Roblox for the missing ones.
  useEffect(() => {
    if (!accounts.data) return;
    const ids = new Set(accounts.data.map((a) => a.id));
    if (known.current && [...ids].some((id) => !known.current?.has(id))) qc.invalidateQueries({ queryKey: qk.headshots });
    known.current = ids;
  }, [accounts.data, qc]);

  // Opens on the Launch target's game until a game is loaded here.
  const openServers = () => {
    const p = draft.parsed?.text === draft.text ? draft.parsed.value : null;
    const t = p?.kind === "target" ? p.target : null;
    if (serversPlace === null && t && "placeId" in t) setServersPlace(t.placeId);
    setView("servers");
  };

  // Re-bound each render: Ctrl+4 needs the current draft. Alt is left alone (AltGr types characters).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey || document.querySelector("dialog[open]")) return;
      const k = e.key.toLowerCase();
      // By key position, so Ctrl+1 works on layouts where the digits need Shift.
      const views: Record<string, () => void> = {
        Digit1: () => setView("dashboard"),
        Digit2: () => setView("accounts"),
        Digit3: () => setView("launch"),
        Digit4: openServers,
        Digit5: () => setView("settings"),
        Digit6: () => setView("monitor"),
        Digit7: () => setView("discord"),
        Digit8: () => setView("debug"),
        Digit9: () => setView("performance"),
      };
      const go = views[e.code];
      if (go) {
        e.preventDefault();
        // A field saves on blur: what was typed goes with it, not lost with the view.
        if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
        go();
      } else if (k === "n") {
        e.preventDefault();
        openAdd();
      } else if (k === "f") {
        e.preventDefault();
        setView("accounts");
        requestAnimationFrame(() => document.getElementById("account-filter")?.focus());
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const failed = accounts.error ?? groups.error;
  if (failed)
    return (
      <div className="flex-1 grid place-items-center p-8">
        <div className="flex flex-col items-start gap-3 max-w-sm">
          <p className="text-dim">{friendly(asError(failed))}</p>
          <button className="btn" onClick={() => { accounts.refetch(); groups.refetch(); }}>{t("common.tryAgain")}</button>
        </div>
      </div>
    );
  if (!accounts.data || !groups.data) return <div className="flex-1" />;

  const showAll = () => {
    setView("accounts");
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
    setView("launch");
  };
  const pickFavourite = (f: Favourite) => {
    setDraft(draftFor(f.target, f.id, f.name));
    const valid = f.accountIds.filter((id) => accounts.data?.some((a) => a.id === id && a.status === "valid"));
    if (valid.length) setPicked(valid);
    setView("launch");
  };
  const item = (to: typeof view, icon: Parameters<typeof Icon>[0]["name"], label: string, open: () => void, count?: ReactNode) => (drag?: DragProps) => (
    <button key={to} className="nav-item" aria-current={view === to ? "page" : undefined} onClick={open} {...drag}>
      <Icon name={icon} />
      <span className="flex-1 min-w-0 truncate">{label}</span>
      {count}
    </button>
  );
  const items: Record<Section, (drag?: DragProps) => ReactNode> = {
    home: item("dashboard", "home", t("app.nav.dashboard"), () => setView("dashboard"), running > 0 && (
      <span className="nav-count mono" title={t("app.running", { count: running })}>
        {running}
      </span>
    )),
    play: item("launch", "play", t("app.nav.launch"), () => setView("launch")),
    accounts: item("accounts", "accounts", t("app.nav.accounts"), showAll, accounts.data.length > 0 && <span className="nav-count mono">{accounts.data.length}</span>),
    performance: item("performance", "gauge", t("app.nav.performance"), () => setView("performance")),
    debug: item("debug", "sliders", t("app.nav.debug"), () => setView("debug")),
    servers: item("servers", "servers", t("app.nav.servers"), openServers),
    monitor: item("monitor", "monitor", t("app.nav.monitor"), () => setView("monitor")),
    discord: item("discord", "bell", t("app.nav.discord"), () => setView("discord")),
    settings: item("settings", "settings", t("app.nav.settings"), () => setView("settings")),
  };
  const sections = (ids: readonly Section[], column: boolean, className?: string) => (
    <Sortable column={column} className={className} ids={ids} order={sectionOrder as Section[]} onOrder={setSectionOrder} name={(id) => t(SECTION_NAMES[id])} render={(id, drag) => items[id](drag)} />
  );
  const footer = (
    <div className="flex-none flex flex-col gap-3.5 px-3 pt-3 pb-4">
      <button className="btn w-full" onClick={() => setDonating(true)}>
        <Icon name="heart" size={14} />
        {t("donate.button")}
      </button>
      <DonateDialog open={donating} onClose={() => setDonating(false)} />
      <KillAllButton />
      <p className="flex items-center gap-2 text-[13px] text-dim" title={vault.mode === "master" ? t("app.vaultUnlockedMaster") : t("app.vaultUnlockedDpapi")}>
        <Icon name="lock" size={14} />
        {t("app.encrypted")}
      </p>
    </div>
  );
  const side = settings.tabLayout === "side";
  const settingsLinks = SETTINGS_SECTIONS.map(([id, label]) => (
    <button key={id} className="sub" onClick={() => {
      const to = document.getElementById(id);
      to?.closest("details")?.setAttribute("open", "");
      to?.scrollIntoView({ block: "start" });
      to?.setAttribute("tabindex", "-1");
      to?.focus({ preventScroll: true });
    }}>
      {t(label)}
    </button>
  ));
  return (
    <div className="flex-1 flex min-h-0">
      <span className="sr-only" aria-live="polite">{viewName}</span>
      {side ? (
        <AccountRail
          width={nav.width}
          accounts={accounts.data}
          instances={instances}
          home={homeTabs}
          onHome={view === "dashboard"}
          onOpen={() => setView("dashboard")}
          onAdd={() => openAdd()}
          footer={footer}
        />
      ) : (
      <nav aria-label={t("app.sections")} style={{ width: nav.width }} className="flex-none flex flex-col min-h-0">
        <div className="scroll flex-1 min-h-0 flex flex-col gap-1.5 px-3 pt-2.5 pb-3" style={{ scrollbarWidth: "none" }}>
          {/* Sorted within each group; More tools and Settings stay where they are. */}
          {sections(MAIN_SECTIONS, true, "flex flex-col gap-1.5")}
          <button className="nav-item" aria-expanded={showTools} onClick={() => setToolsOpen(!showTools)}>
            <Icon name="chevron" className={showTools ? "" : "-rotate-90 rtl:rotate-90"} />
            <span className="flex-1 min-w-0 truncate">{t("app.nav.moreTools")}</span>
          </button>
          {showTools && sections(TOOL_SECTIONS, true, "flex flex-col gap-1.5")}
          {items.settings()}
          {view === "settings" && <div className="enter flex flex-col ps-7 pt-1">{settingsLinks}</div>}
        </div>
        {footer}
      </nav>
      )}
      <Divider panel={nav} side="left" label={t("app.sidebarWidth")} />

      <div className="flex-1 min-w-0 flex flex-col min-h-0">
      {side && <SectionTabs>{sections([...MAIN_SECTIONS, ...TOOL_SECTIONS, "settings"], false, "contents")}</SectionTabs>}
      {side && view === "settings" && <div className="enter flex-none flex flex-wrap gap-x-1 px-1 pb-2 [&>.sub]:w-auto">{settingsLinks}</div>}
      <div className="sheet min-h-0">
      {view === "dashboard" ? (
        <DashboardView accounts={accounts.data} home={homeTabs} onLaunch={() => setView("launch")} onAdd={() => openAdd()} onImport={() => openAdd("many")} onAccounts={showAll} />
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
          step={launchStep}
          setStep={setLaunchStep}
          onAdd={() => openAdd()}
          onServers={openServers}
          onPickFavourite={pickFavourite}
          onHome={() => setView("dashboard")}
          onAccounts={showAll}
          onSettings={() => setView("settings")}
        />
      ) : view === "servers" ? (
        <ServersView
          accounts={accounts.data}
          picked={picked}
          setPicked={setPicked}
          place={serversPlace}
          setPlace={setServersPlace}
          onBatch={() => setView("launch")}
          step={launchStep}
          setStep={setLaunchStep}
        />
      ) : view === "monitor" ? (
        <MonitorView accounts={accounts.data} />
      ) : view === "discord" ? (
        <DiscordView settings={settings} />
      ) : view === "debug" ? (
        <DebugView />
      ) : view === "performance" ? (
        <PerformanceView settings={settings} />
      ) : (
        <SettingsView vault={vault} settings={settings} onPerformance={() => setView("performance")} onRecoveryCode={onRecoveryCode} />
      )}
      </div>
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

/** Surfaces that turn see-through over a background image, and how much of them stays (the rest
 * stay solid). Panels hold most of the text, so they keep it readable over any picture. */
const SEE_THROUGH = { "--bg": 50, "--panel": 72 };

/** Text on the accent: whichever of ink and white has the higher contrast (WCAG luminance). */
function onAccent(hex: string) {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.18 ? "#0a0b0d" : "#ffffff";
}

/** Applies the theme to <html>: `data-theme` (dark or light, "system" follows Windows live), the
 * accent colour (`data-accent`, theme.css) and the background image. Returns whether it is dark. */
function useTheme(s: Settings | undefined) {
  const [dark, setDark] = useState(() => document.documentElement.dataset.theme !== "light");
  const theme = s?.theme;
  const accent = s?.accent ?? "";
  // Less than this over a bright picture and no text stays readable.
  const dim = Math.max(40, s?.backgroundDim ?? 60);
  const bytes = useQuery({ queryKey: qk.background, queryFn: api.backgroundGet, staleTime: Infinity, enabled: !!s }).data;
  const [image, setImage] = useState<string | null>(null);

  useEffect(() => {
    if (!bytes?.byteLength) return setImage(null);
    const url = URL.createObjectURL(new Blob([bytes]));
    setImage(url);
    return () => URL.revokeObjectURL(url);
  }, [bytes]);

  useEffect(() => {
    if (!theme) return;
    const root = document.documentElement;
    const mq = matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const t = theme === "system" ? (mq.matches ? "dark" : "light") : theme;
      root.dataset.theme = t;
      setDark(t !== "light");
      if (accent) {
        root.dataset.accent = "";
        root.style.setProperty("--user-accent", accent);
        root.style.setProperty("--user-on-accent", onAccent(accent));
      } else {
        delete root.dataset.accent;
      }
      // The window's colour covers the picture by `dim`; panels on top are half see-through.
      for (const v of Object.keys(SEE_THROUGH)) root.style.removeProperty(v);
      if (image) {
        const css = getComputedStyle(root);
        root.style.setProperty("--bg-dim", `color-mix(in srgb, ${css.getPropertyValue("--bg").trim()} ${dim}%, transparent)`);
        for (const [v, keep] of Object.entries(SEE_THROUGH)) root.style.setProperty(v, `color-mix(in srgb, ${css.getPropertyValue(v).trim()} ${keep}%, transparent)`);
        root.style.setProperty("--bg-image", `url("${image}")`);
        root.dataset.bg = "";
      } else {
        delete root.dataset.bg;
      }
      try {
        localStorage.setItem(THEME_KEY, t);
      } catch {
        // Storage can be unavailable; the theme still applies.
      }
    };
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme, accent, image, dim]);
  return dark;
}

/** Tells the backend about user input (auto-lock), at most once every 30 s. */
function useActivity(enabled: boolean) {
  useEffect(() => {
    if (!enabled) return;
    let last = 0;
    const ping = () => {
      const now = Date.now();
      if (now - last < 30_000) return;
      last = now;
      api.appActivity().catch(() => {});
    };
    const types = ["pointerdown", "pointermove", "keydown", "wheel"] as const;
    types.forEach((t) => window.addEventListener(t, ping, { passive: true, capture: true }));
    return () => types.forEach((t) => window.removeEventListener(t, ping, { capture: true }));
  }, [enabled]);
}
