import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { useQueryClient } from "@tanstack/react-query";
import "./dashboard.css";
import { api, asError, type AccountSummary, type Favourite, type GameInfo, type Instance } from "../../api";
import { qk, useInstances, useSetSettings, useSettings } from "../../data";
import { fmtList, t } from "../../i18n";
import { Skeleton } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { DUR, riseIn, STAGGER, staggerDelay, Swap, useFlip, useFreshItems } from "../../ui/motion";
import { friendly, toastError } from "../../ui/Toast";
import { Counted } from "../accounts/AccountsToolbar";
import { isRunning, useNow } from "../launch/Running";
import { AccountTabs } from "./AccountTabs";
import { cardFlex, ClientCard, clientName, type GripProps } from "./ClientCard";
import { ClientMenu, closeKey, type MenuAt } from "./ClientMenu";
import { EndedPanel } from "./Ended";
import { homeSorted, reordered, useHomeDetailed, useHomeSizes, type HomeTabs } from "./homeLayout";
import { HomeToolbar, type Fold } from "./HomeToolbar";
import { LimitsDialog } from "./LimitsDialog";
import { usePreviewFps } from "./previewFps";
import { Trouble } from "./Trouble";
import { useGridDrag } from "./useGridDrag";
import { Welcome } from "./Welcome";

/** True once `on` has lasted `ms`: a quick load shows no placeholder at all. */
function useLate(on: boolean, ms: number) {
  const [late, setLate] = useState(false);
  useEffect(() => {
    if (!on) return setLate(false);
    const timer = setTimeout(() => setLate(true), ms);
    return () => clearTimeout(timer);
  }, [on, ms]);
  return late;
}

/** How far the header's width folds its pill (HomeToolbar's `fold`), kept current; null until
 * measured. Only a change of step renders. */
function useFold(ref: RefObject<HTMLElement | null>) {
  const [fold, setFold] = useState<Fold | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    // Steps with room to spare for longer languages; the full pill is about 1100 px in English.
    const measure = () => {
      const w = el.offsetWidth;
      setFold(w < 900 ? 3 : w < 1080 ? 2 : w < 1250 ? 1 : 0);
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return fold;
}

/** The client list hasn't come yet: the header and three cards at their real size. */
function Loading({ detailed }: { detailed: boolean }) {
  return (
    <div aria-busy="true" className="flex flex-col gap-6">
      <span role="status" className="sr-only">{t("common.loading")}</span>
      <div className="flex flex-col gap-2">
        <Skeleton width={240} height={24} />
        <Skeleton width={320} height={14} />
      </div>
      <div className="home-grid">
        {[0, 1, 2].map((n) => (
          <div key={n} className="panel home-card" style={cardFlex(undefined, detailed)}>
            <span aria-hidden="true" className="skel block aspect-video rounded-[10px]" />
            {detailed && (
              <div className="home-info">
                <Skeleton width="55%" height={14} />
                <Skeleton width="35%" height={12} />
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function LoadError({ error, busy, onRetry }: { error: unknown; busy: boolean; onRetry: () => void }) {
  return (
    <div role="alert" className="notice" data-tone="attention">
      <p className="alert flex-[1_1_240px] min-w-0">{friendly(asError(error))}</p>
      <Button size="md" icon="refresh" busy={busy} onClick={onRetry}>
        {t("common.tryAgain")}
      </Button>
    </div>
  );
}

/** The running clients: the header with its pill, what waits to come back, and the cards. The cards
 * glide when others come, go or move (useFlip), and rise in as they arrive. */
function Clients({ accounts, home, list, running, detailed, setDetailed, extra }: {
  accounts: AccountSummary[];
  home: HomeTabs;
  list: Instance[];
  running: Instance[];
  detailed: boolean;
  setDetailed: (on: boolean) => void;
  extra: ReactNode;
}) {
  const settings = useSettings().data;
  const setSettings = useSetSettings();
  const qc = useQueryClient();
  const [sizes, setSize] = useHomeSizes();
  const [fps] = usePreviewFps();
  const now = useNow(30_000);
  const byId = new Map(accounts.map((a) => [a.id, a]));
  const nameOf = (i: Instance) => clientName(i, byId.get(i.accountId));
  const shown = homeSorted(running, home.order);
  const ids = shown.map((i) => i.id);
  const grid = useRef<HTMLElement | null>(null);
  const hold = useFlip(grid, { key: ids.join() });
  const drag = useGridDrag((from, over) => {
    hold();
    home.setOrder(reordered(home.order, shown.map((i) => i.accountId), from, over));
  });
  const setGrid = useCallback((el: HTMLElement | null) => void (grid.current = drag.grid.current = el), [drag.grid]);
  const head = useRef<HTMLElement>(null);
  const fold = useFold(head);
  const [menu, setMenu] = useState<MenuAt | null>(null);
  const closeMenu = useCallback(() => setMenu(null), []);
  // The key that opened the menu closes it again: the click already dismissed the popover.
  const openMenu = (at: MenuAt) => setMenu((m) => (at.beside !== undefined && m?.from === at.from ? null : at));
  const [limitsFor, setLimitsFor] = useState<string | null>(null);
  const [closingOne, setClosingOne] = useState<ReadonlySet<string>>(new Set());
  const afkOn = settings?.antiAfk.enabled ?? false;
  const inGame = running.filter((i) => i.state === "inGame").length;
  const coming = running.some((i) => i.state === "starting" || i.state === "joining" || (i.relaunching && i.state === "disconnected"));
  const games = [...new Set(running.flatMap((i) => (i.state !== "inGame" || i.placeId === null ? [] : [i.placeId])))].map((id) => qc.getQueryData<GameInfo>(qk.game(id))?.name).filter((n): n is string => !!n);
  const sub = [games.length > 0 && t("home.in", { games: fmtList(games) }), settings?.autoRejoin ? t("home.autoRejoinOn") : t("home.autoRejoinOff")].filter(Boolean).join(" ");
  const limited = list.find((i) => i.id === limitsFor);

  // The first paint's cards rise one after another; later ones as they come. Glides wait till they're up.
  const fresh = useFreshItems(ids);
  const painted = useRef(false);
  useLayoutEffect(() => {
    const tiles = [...(grid.current?.children ?? [])] as HTMLElement[];
    const rise = painted.current ? tiles.filter((el) => fresh.includes(el.dataset.id!)) : tiles.length <= STAGGER.listLimit ? tiles : [];
    painted.current = true;
    rise.forEach((el, n) => riseIn(el, n));
    if (rise.length) hold(DUR.base + 140 + staggerDelay(rise.length - 1) * 1000);
  }, [ids.join()]); // eslint-disable-line react-hooks/exhaustive-deps

  // Another density: every card changes size, so the grid fades and rises in instead of gliding.
  const density = (on: boolean) => {
    hold(DUR.base + 140);
    setDetailed(on);
  };
  const was = useRef(detailed);
  useLayoutEffect(() => {
    if (was.current === detailed) return;
    was.current = detailed;
    if (grid.current) riseIn(grid.current);
  }, [detailed]);

  // A drag that glides back home moves its tile too.
  const grip = (name: string): GripProps => {
    const g = drag.grip(name);
    return {
      ...g,
      onPointerUp: (e) => (g.onPointerUp(e), hold()),
      onPointerCancel: () => (g.onPointerCancel(), hold()),
    };
  };
  const closeOne = (i: Instance, force: boolean) => {
    const key = closeKey(i);
    setClosingOne((s) => new Set(s).add(key));
    api.instanceClose(i.id, force).catch((e) => {
      setClosingOne((s) => {
        const next = new Set(s);
        next.delete(key);
        return next;
      });
      toastError(e);
    });
  };

  return (
    <>
      <header ref={head} className="home-head">
        {/* At its full width: the pill goes under it rather than cut its line short. */}
        <div className="min-w-0 flex-auto">
          <h1 className="page-title relative">
            <Swap k={inGame ? "playing" : coming ? "coming" : "none"}>
              {/* One inline box: as a flex item of its own, the count's space would collapse. */}
              <span>{inGame ? <Counted k="home.playing" n={inGame} /> : coming ? t("home.gettingIn") : t("home.noneInGame")}</span>
            </Swap>
          </h1>
          <p className="page-sub home-sub">
            <span className="truncate" data-tip={sub}>{sub}</span>
            {settings && !settings.autoRejoin && (
              <Button variant="ghost" size="sm" onClick={() => setSettings.mutate({ autoRejoin: true })}>
                {t("launch.multi.turnOn")}
              </Button>
            )}
          </p>
        </div>
        <div className="home-head-end">
          {/* Once the header is measured: the pill's first size is its folded one, with no spring. */}
          {fold !== null && <HomeToolbar running={running.length} fold={fold} detailed={detailed} onDetailed={density} />}
          {extra}
        </div>
      </header>
      <Trouble list={list} byId={byId} />
      <span className="sr-only" aria-live="polite">{drag.moved}</span>
      <section ref={setGrid} aria-label={t("launch.running.title")} className="home-grid">
        {shown.map((i) => (
          <ClientCard
            key={i.id}
            inst={i}
            account={byId.get(i.accountId)}
            detailed={detailed}
            width={sizes[i.accountId]}
            menuOpen={menu?.inst.id === i.id}
            onSize={(w) => setSize(i.accountId, w)}
            onSizing={hold}
            grip={grip(nameOf(i))}
            onMenu={openMenu}
            closing={closingOne.has(closeKey(i))}
            afkOn={afkOn}
            defaultFps={settings?.fpsCap ?? 0}
            fps={fps}
            now={now}
          />
        ))}
      </section>
      <ClientMenu at={menu} account={menu ? byId.get(menu.inst.accountId) : undefined} onClose={closeMenu} afkOn={afkOn} closing={closingOne} onLimits={setLimitsFor} onCloseClient={closeOne} />
      <LimitsDialog instanceId={limitsFor} name={limited ? nameOf(limited) : ""} onClose={() => setLimitsFor(null)} />
    </>
  );
}

/** Home without a tab open: the running clients, or with none the way to a first game; how the
 * client list is loading or why it failed; then what recently closed. */
function Overview({ accounts, home, extra, onAdd, onImport, onAccounts, onPickFavourite }: {
  accounts: AccountSummary[];
  home: HomeTabs;
  extra: ReactNode;
  onAdd: () => void;
  onImport: () => void;
  onAccounts: () => void;
  onPickFavourite?: (f: Favourite) => void;
}) {
  const instances = useInstances();
  const [detailed, setDetailed] = useHomeDetailed();
  const late = useLate(instances.isPending, 150);
  const failed = instances.isError && <LoadError error={instances.error} busy={instances.isFetching} onRetry={() => void instances.refetch()} />;
  if (!instances.data) return failed || (late ? <Loading detailed={detailed} /> : null);
  const list = instances.data;
  const running = list.filter(isRunning);
  return (
    <>
      {/* A refresh failed: what was there stays, with the way to ask again. */}
      {failed}
      {running.length > 0 ? (
        <Clients accounts={accounts} home={home} list={list} running={running} detailed={detailed} setDetailed={setDetailed} extra={extra} />
      ) : (
        <Welcome
          accounts={accounts}
          toolbar={<HomeToolbar running={0} fold={0} detailed={detailed} onDetailed={setDetailed} />}
          extra={extra}
          // A client that ended can still be waiting to come back (a crash restart, a rejoin's relaunch).
          notice={<Trouble list={list} byId={new Map(accounts.map((a) => [a.id, a]))} />}
          onAdd={onAdd}
          onImport={onImport}
          onAccounts={onAccounts}
          onPickFavourite={onPickFavourite}
        />
      )}
      <EndedPanel accounts={accounts} />
    </>
  );
}

/** Every running client at a glance, with a picture of its window, and the window tools; with
 * nothing running, the way to the first game. Home's tabs (News, Overview, accounts) sit above it. */
export function DashboardView({ accounts, home, onAdd, onImport, onAccounts, onPickFavourite }: {
  accounts: AccountSummary[];
  home: HomeTabs;
  onAdd: () => void;
  onImport: () => void;
  onAccounts: () => void;
  /** Opens Play with this favourite's target and accounts. */
  onPickFavourite?: (f: Favourite) => void;
}) {
  return (
    <main className="scroll flex-1 min-w-0 page flex flex-col gap-6">
      <AccountTabs accounts={accounts} home={home} onAdd={onAdd}>
        {(extra) => <Overview accounts={accounts} home={home} extra={extra} onAdd={onAdd} onImport={onImport} onAccounts={onAccounts} onPickFavourite={onPickFavourite} />}
      </AccountTabs>
    </main>
  );
}
