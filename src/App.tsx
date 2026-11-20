import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { QueryClientProvider, useQueryClient } from "@tanstack/react-query";
import { api, events, type VaultStatus } from "./api";
import { qk, queryClient, useSetSettings, useSettings, useVault } from "./data";
import { AppMenu } from "./app/AppMenu";
import { TitleTools } from "./app/TitleTools";
import { useTheme } from "./app/theme";
import { DonateDialog } from "./features/donate/DonateDialog";
import { PublicNetworkBanner } from "./features/settings/PublicNetworkBanner";
import { resetLaunches } from "./features/launch/launchQueue";
import { Splash } from "./features/splash/Splash";
import { RecoveryCodeHost } from "./features/vault/recoveryCode";
import { UnlockScreen, type LockReason } from "./features/vault/UnlockScreen";
import { t, useLang } from "./i18n";
import { Announcer } from "./ui/a11y";
import { MotionRoot } from "./ui/motion";
import { TitleBar } from "./ui/TitleBar";
import { LoadFailed, toast, toastError, Toaster } from "./ui/Toast";
import { TooltipHost } from "./ui/Tooltip";

// The start shows the splash or the unlock screen first: the shell (every view) loads alongside,
// asked for now rather than at unlock.
const shellChunk = import("./app/Shell");
const Shell = lazy(() => shellChunk.then((m) => ({ default: m.Shell })));

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <MotionRoot>
        <Root />
        <Toaster />
        <Splash />
        <TooltipHost />
        <Announcer />
      </MotionRoot>
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
  const [islandSlot, setIslandSlot] = useState<HTMLDivElement | null>(null);
  const [donating, setDonating] = useState(false);
  const manualLockAt = useRef(0);
  const v = vault.data;
  const s = settings.data;
  // Before any child renders, so the whole tree uses the chosen language.
  useLang(s?.language);
  const unlocked = !!v && !v.locked;

  const markLocked = () => {
    qc.setQueryData<VaultStatus>(qk.vault, (old) => old && { ...old, locked: true });
    for (const queryKey of [qk.accounts, qk.presence, ["info"], qk.favourites, qk.join, qk.privateServers, qk.discord]) qc.removeQueries({ queryKey });
    // A launch kept on show from here (its target can hold a private server's code) goes too. The
    // backend's batch (qk.bulk) stays: "unlock, then launch the ones cancelled" shows after unlock.
    resetLaunches();
  };

  // Everything on screen asks again; the webview itself is never reloaded. Not the background
  // image: only Appearance changes it (and refreshes it), and a reread is up to 8 MB.
  const refresh = () => void qc.invalidateQueries({ predicate: (q) => q.queryKey[0] !== qk.background[0] });

  const lock = () => {
    manualLockAt.current = Date.now();
    setLockReason("manual");
    api.vaultLock().then(markLocked, toastError);
  };

  useEffect(() => {
    const subs = [
      events.onAccountsChanged(() => {
        // A restore fires it too.
        for (const queryKey of [qk.accounts, qk.groups, qk.vault]) qc.invalidateQueries({ queryKey });
      }),
      events.onInstancesChanged(() => {
        for (const queryKey of [qk.instances, qk.multi, qk.crashes, ["serverHistory"]]) qc.invalidateQueries({ queryKey });
      }),
      events.onBulkProgress((p) => qc.setQueryData(qk.bulk, p)),
      events.onRobloxClient((c) => qc.setQueryData(qk.client, c)),
      events.onNetwork((n) => qc.setQueryData(qk.network, n)),
      events.onAppUpdate((u) => qc.setQueryData(qk.appUpdate, u)),
      // Errors stay until dismissed: the window may have been hidden in the tray.
      events.onNotice((n) => toast(n.message, n.error ? "error" : "ok", n.error, n.error ? "error" : "notification")),
      events.onVaultLocked(() => {
        // A manual lock waits up to ~5 s for session windows to close first.
        if (Date.now() - manualLockAt.current > 8000) setLockReason("idle");
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
    <div className="app">
      <TitleBar
        islandSlot={setIslandSlot}
        start={<AppMenu unlocked={unlocked} vaultMode={v?.mode} onRefresh={refresh} onDonate={() => setDonating(true)} />}
        tools={
          <TitleTools
            dark={dark}
            onRefresh={unlocked ? refresh : undefined}
            onToggleTheme={s ? () => setSettings.mutate({ theme: dark ? "light" : "dark" }) : undefined}
            onLock={unlocked && v.mode === "master" ? lock : undefined}
            vaultMode={unlocked ? v.mode : undefined}
            sideTabs={s?.tabLayout === "side"}
            onToggleTabs={unlocked && s ? () => setSettings.mutate({ tabLayout: s.tabLayout === "side" ? "top" : "side" }) : undefined}
          />
        }
      />
      {/* Unlocked, the island carries notices; locked, there is no island. */}
      {s && !failed && !unlocked && <PublicNetworkBanner settings={s} />}
      {failed ? (
        <LoadFailed title={t("app.startFailed")} error={failed} onRetry={() => qc.invalidateQueries()} />
      ) : !v || !s ? (
        <div className="flex-1" />
      ) : v.locked ? (
        <UnlockScreen status={v} reason={lockReason} />
      ) : (
        <Suspense fallback={<div className="flex-1" />}>
          <Shell vault={v} settings={s} islandSlot={islandSlot} />
        </Suspense>
      )}
      {/* A new recovery code no flow is showing (a recovered unlock, a lock mid-flow): up here because it must outlive what made it, as the old code is void by now. Never while locked: it would open the vault for whoever comes by. */}
      <RecoveryCodeHost unlocked={unlocked} />
      <DonateDialog open={donating} onClose={() => setDonating(false)} />
    </div>
  );
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
