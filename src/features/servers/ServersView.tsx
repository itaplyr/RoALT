import { useEffect, useRef, useState, type Dispatch, type FormEvent, type ReactNode, type SetStateAction } from "react";
import { useInfiniteQuery, useQuery, useQueryClient, type InfiniteData } from "@tanstack/react-query";
import { api, asError, type AccountSummary, type GameInfo, type LaunchTarget, type ServerOrder } from "../../api";
import { qk, useBulk, useInstances, useMultiStatus, useRobloxClient, useSettings } from "../../data";
import { Segmented, Switch } from "../../ui/controls";
import { Icon } from "../../ui/Icon";
import { friendly } from "../../ui/Toast";
import { fmtNum, t, tx, type Key } from "../../i18n";
import { accountName, avatarColor } from "../accounts/shared";
import { launchBlocked, launched, launchFailed } from "../launch/LaunchView";
import { isRunning, MultiNotice, useNow } from "../launch/Running";
import { PrivateTable, PublicTable, short, type Pick } from "./ServerTables";

const PAGE = 25;
const SIGNED_IN: ServerOrder[] = ["recommended", "bestLatency"];
const ORDERS: { value: ServerOrder; label: Key }[] = [
  { value: "occupancyDesc", label: "servers.orderMostFull" },
  { value: "occupancyAsc", label: "servers.orderMostEmpty" },
  { value: "recommended", label: "servers.orderRecommended" },
  { value: "bestLatency", label: "servers.orderBestPing" },
];

/** The server browser. Roblox rate-limits the list, so pages load only when asked for. */
export function ServersView({ accounts, picked, setPicked, place, setPlace, onBatch, step, setStep }: {
  accounts: AccountSummary[];
  picked: number[];
  setPicked: Dispatch<SetStateAction<number[]>>;
  place: number | null;
  setPlace: (placeId: number) => void;
  /** A bulk launch started; its progress shows in the Launch view. */
  onBatch: () => void;
  /** Shared with the Launch view: one launch at a time, also across views. */
  step: string | null;
  setStep: (step: string | null) => void;
}) {
  const qc = useQueryClient();
  const multi = useMultiStatus().data;
  const batch = useBulk().data;
  const instances = useInstances().data ?? [];
  const delayMs = useSettings().data?.launchDelayMs ?? 6000;
  const [text, setText] = useState(place === null ? "" : String(place));
  const [textError, setTextError] = useState<string | null>(null);
  const [tab, setTab] = useState<"public" | "private">("public");
  const [order, setOrder] = useState<ServerOrder>("occupancyDesc");
  const [hideFull, setHideFull] = useState(true);
  const [pick, setPick] = useState<Pick>(null);
  const [loading, setLoading] = useState(false);

  const valid = accounts.filter((a) => a.status === "valid");
  // In list order, which is also the bulk launch order; the first one asks Roblox.
  const chosen = valid.filter((a) => picked.includes(a.id));
  const asker = chosen[0]?.id;
  // Recommended and Best ping need an account: without one, nothing is asked for.
  const needsAccount = asker === undefined && SIGNED_IN.includes(order);

  // A game without its icon (failed or still being made) is asked for again next time.
  const game = useQuery({
    queryKey: qk.game(place),
    queryFn: () => api.gameInfo(place!),
    enabled: place !== null,
    staleTime: (q) => (q.state.data?.iconUrl ? Infinity : 0),
  });
  const g = game.data;
  // Not gated on g.privateServers: that guest hint is false for games whose servers exist anyway.
  const view = tab;
  const pubKey = [...qk.servers, place, order, hideFull];
  const pub = useInfiniteQuery({
    queryKey: pubKey,
    // The asking account isn't in the key: picking other accounts doesn't reload the list.
    // Every fetch path (Load next, Try again, refetch) goes through here: no guest request for a
    // signed-in sort.
    queryFn: ({ pageParam }) =>
      needsAccount
        ? Promise.reject({ kind: "invalid", message: t("servers.needAccountSort") })
        : api.serversList(place!, order, hideFull, PAGE, pageParam ?? undefined, asker),
    initialPageParam: null as string | null,
    getNextPageParam: (p) => p.nextCursor,
    enabled: !!g && view === "public" && !needsAccount,
  });
  const privKey = [...qk.privateServers, place, asker];
  const priv = useInfiniteQuery({
    queryKey: privKey,
    queryFn: ({ pageParam }) => api.privateServersList(place!, asker!, pageParam ?? undefined),
    initialPageParam: null as string | null,
    getNextPageParam: (p) => p.nextCursor,
    enabled: !!g && view === "private" && asker !== undefined,
  });
  const list = view === "public" ? pub : priv;

  // Pages repeat servers as players come and go; the newest copy wins.
  const servers = [...new Map(pub.data?.pages.flatMap((p) => p.servers.map((s) => [s.id, s] as const)) ?? []).values()];
  const privates = priv.data?.pages.flatMap((p) => p.servers) ?? [];
  const count = view === "public" ? servers.length : privates.length;

  const pubPick = pick?.kind === "public" ? pick.id : null;
  const privPick = pick?.kind === "private" ? privates.find((s) => s.vipServerId === pick.id) : undefined;
  const room = pubPick ? servers.find((s) => s.id === pubPick) : privPick;
  const free = room ? Math.max(0, room.maxPlayers - room.playing) : null;
  const target: LaunchTarget | null =
    place === null ? null
    : pubPick ? { kind: "server", placeId: place, jobId: pubPick }
    : privPick?.accessCode ? { kind: "privateAccess", placeId: place, accessCode: privPick.accessCode }
    : null;
  const summary = pubPick
    ? free === null ? short(pubPick) : t("servers.summaryFree", { name: short(pubPick), count: free })
    : privPick ? t("servers.summaryFree", { name: privPick.name || t("servers.privateServer"), count: free ?? 0 }) : t("servers.noServerPicked");
  const n = chosen.length;
  const client = useRobloxClient().data;
  const askerName = chosen[0] ? accountName(chosen[0]) : "";
  const restricted = !!privPick && !!priv.data?.pages.some((p) => p.joinRestricted);
  const noJoin = t("servers.noJoin", { name: askerName });
  const replaces = n === 1 && instances.some((i) => isRunning(i) && i.accountId !== chosen[0].id);
  const blocked = launchBlocked(n > 1 || (replaces && !!multi?.enabled), multi, batch) ?? (restricted && n === 1 ? noJoin : null);
  // Multi-instance has its own notice, with the way out.
  const warning =
    [
      (batch && !batch.done ? t("launch.batchRunning") : null) ?? (restricted ? noJoin : null),
      privPick && n > 1 ? t("servers.accessCheckedFor", { name: askerName }) : null,
      free !== null && n > free ? t("servers.onlyFreeSlots", { count: free, accounts: t("servers.nAccounts", { count: n }) }) : null,
      n > 1 && client?.outdated ? t("servers.outdated") : null,
    ]
      .filter(Boolean)
      .join(" ") || null;
  const canJoin = !!target && n > 0 && !step && !blocked;

  // A lookup that finishes after a newer Load (or after leaving the view) is dropped.
  const loads = useRef(0);
  useEffect(() => () => void loads.current++, []);

  async function load(e: FormEvent) {
    e.preventDefault();
    const mine = ++loads.current;
    const p = await api.targetParse(text).catch(() => null);
    if (mine !== loads.current) return;
    const tgt = p?.kind === "target" ? p.target : null;
    if (!tgt || tgt.kind === "followUser") {
      setTextError(p && "reason" in p ? p.reason : t("servers.pasteLink"));
      return;
    }
    // Universe and share links need a lookup (share links with the first picked account).
    setLoading(true);
    try {
      const id = "placeId" in tgt ? tgt.placeId : await api.targetPlace(tgt, asker);
      if (mine !== loads.current) return;
      // Loading the game already shown asks for its icon again if it was missing.
      if (id === place && g && !g.iconUrl) void game.refetch();
      setPlace(id);
      setTextError(null);
      setTab("public");
      setPick(tgt.kind === "server" ? { kind: "public", id: tgt.jobId } : null);
    } catch (e) {
      if (mine === loads.current) setTextError(friendly(asError(e)));
    } finally {
      if (mine === loads.current) setLoading(false);
    }
  }

  // Page 1 again, without asking for every page loaded so far.
  const refresh = () => {
    qc.setQueryData<InfiniteData<unknown>>(
      view === "public" ? pubKey : privKey,
      (d) => d && { pages: d.pages.slice(0, 1), pageParams: d.pageParams.slice(0, 1) },
      { updatedAt: list.dataUpdatedAt },
    );
    void list.refetch();
  };

  async function join() {
    if (!canJoin || !target) return;
    setStep(n > 1 ? t("servers.startingBatch") : t("servers.gettingTicket"));
    try {
      if (n > 1) {
        await api.bulkLaunch({ accountIds: chosen.map((a) => a.id), target, delayMs, sameServer: "off", favouriteId: null, fpsCap: null });
        onBatch();
      } else launched(chosen[0], await api.launch(chosen[0].id, target));
    } catch (e) {
      launchFailed(e);
    } finally {
      setStep(null);
    }
  }

  const toggle = (id: number) => setPicked((p) => (p.includes(id) ? p.filter((x) => x !== id) : [...p, id]));
  const gameError = game.error && asError(game.error);

  return (
    <main className="scroll flex-1 min-w-0 flex flex-col">
      <div className="flex-none flex flex-wrap items-center gap-x-5 gap-y-3 px-8 pt-3 pb-4">
        {g ? <GameHead game={g} /> : <h1 className="page-title">{t("servers.title")}</h1>}
        <form className="flex flex-col gap-1.5 ms-auto flex-[0_1_260px] min-w-[220px]" onSubmit={(e) => void load(e)}>
          <div className="flex gap-2">
            <label htmlFor="servers-game" className="sr-only">{t("servers.gameInput")}</label>
            <input
              id="servers-game"
              autoFocus={place === null}
              className="field mono flex-1 min-w-0"
              placeholder={t("servers.gameInput")}
              spellCheck={false}
              autoComplete="off"
              value={text}
              aria-invalid={!!textError}
              aria-describedby={textError ? "servers-game-error" : undefined}
              onChange={(e) => {
                setText(e.target.value);
                setTextError(null);
              }}
            />
            <button className="btn" disabled={loading}>{loading ? t("common.loading") : t("servers.load")}</button>
          </div>
          {textError && <p id="servers-game-error" role="alert" className="alert">{textError}</p>}
        </form>
      </div>

      {place === null ? (
        <Note>{t("servers.pastePrompt")}</Note>
      ) : game.isPending ? (
        <Note>
          <span className="flex items-center gap-2"><span aria-hidden="true" className="w-2 h-2 rounded-full bg-dim pulse" />{t("servers.loadingGame")}</span>
        </Note>
      ) : gameError ? (
        <Note>
          <span className="flex flex-col items-start gap-3">
            <span role="alert" className="alert">{gameError.kind === "notFound" ? t("servers.noGame") : friendly(gameError)}</span>
            {gameError.kind !== "notFound" && <button className="btn" onClick={() => void game.refetch()}>{t("common.tryAgain")}</button>}
          </span>
        </Note>
      ) : (
        <>
          <div className="flex-none flex flex-wrap items-center gap-3 px-8 pb-4">
            <Segmented
              label={t("servers.serverType")}
              value={view}
              options={[{ value: "public", label: t("servers.public") }, { value: "private", label: t("servers.private") }]}
              onChange={(v) => {
                setTab(v);
                setPick(null);
              }}
            />
            {view === "public" && (
              <>
                <Segmented
                  label={t("servers.sortServers")}
                  value={order}
                  options={ORDERS.map((o) => {
                    const opt = { value: o.value, label: t(o.label) };
                    return asker === undefined && SIGNED_IN.includes(o.value) ? { ...opt, disabled: true, title: t("servers.needAccountSort") } : opt;
                  })}
                  onChange={setOrder}
                />
                <span className="flex items-center gap-2.5">
                  <Switch label={t("servers.hideFullServers")} checked={hideFull} onChange={setHideFull} />
                  <span aria-hidden="true">{t("servers.hideFull")}</span>
                </span>
              </>
            )}
            <span className="flex items-center gap-3 ms-auto">
              {list.isFetching ? (
                <span className="mono text-xs text-dim">{t("servers.loadingLower")}</span>
              ) : (
                list.dataUpdatedAt > 0 && <Updated at={list.dataUpdatedAt} />
              )}
              <button className="btn" disabled={list.isFetching || asker === undefined && (view === "private" || needsAccount)} onClick={refresh}>
                <Icon name="refresh" size={14} className={list.isFetching ? "spin" : ""} />
                {t("servers.refresh")}
              </button>
            </span>
            <span role="status" className="sr-only">
              {list.isFetching ? t("servers.loadingServers") : list.isSuccess ? t("servers.serversListed", { count }) : ""}
            </span>
          </div>

          <div className="panel scroll flex-1 min-h-[160px] mx-8 p-2">
            {view === "public" && needsAccount && !pub.data ? (
              <p className="px-6 py-4 text-dim">{t("servers.pickAccountSort")}</p>
            ) : view === "public" ? (
              <PublicTable servers={servers} pick={pubPick} onPick={(id) => setPick({ kind: "public", id })} />
            ) : asker === undefined ? (
              <p className="px-6 py-4 text-dim">{t("servers.pickAccountPrivate")}</p>
            ) : (
              <>
                <p className="px-6 pt-3 text-xs text-dim">{t("servers.asName", { name: askerName })}</p>
                {priv.data?.pages.some((p) => p.joinRestricted) && (
                  <p className="alert mx-6 my-3">{noJoin}</p>
                )}
                <PrivateTable servers={privates} pick={privPick?.vipServerId ?? null} onPick={(id) => setPick({ kind: "private", id })} />
              </>
            )}
            <div className="flex flex-col items-center gap-3 p-3.5 text-dim">
              {list.isError ? (
                <>
                  <p role="alert" className="alert">{friendly(asError(list.error))}</p>
                  <button className="btn" disabled={list.isFetching || (view === "public" && needsAccount)} onClick={() => void (list.isFetchNextPageError ? list.fetchNextPage() : refresh())}>
                    {t("common.tryAgain")}
                  </button>
                </>
              ) : list.isFetching && !list.isFetchingNextPage && count === 0 ? (
                t("servers.loadingServersEllipsis")
              ) : list.isSuccess && count === 0 ? (
                view === "private" ? t("servers.noPrivate", { name: askerName }) : hideFull ? t("servers.noRoom") : t("servers.noServers")
              ) : (
                list.hasNextPage && (
                  <button className="btn" disabled={list.isFetching || (view === "public" && needsAccount)} onClick={() => void list.fetchNextPage()}>
                    {list.isFetchingNextPage ? t("common.loading") : t("servers.loadNext", { n: PAGE })}
                  </button>
                )
              )}
            </div>
          </div>

          <MultiNotice wanted={n > 1} replaces={replaces} className="flex-none mx-8 mt-4" />
          <div className="well flex-none mx-8 mt-4 mb-6 px-4 py-3">
            <div className="flex flex-wrap items-center gap-2.5">
              <span className="mono font-medium">{summary}</span>
              <span aria-hidden="true" className="text-dim">{t("servers.send")}</span>
              {valid.length ? (
                <div role="group" aria-label={t("servers.accountsToSend")} className="scroll flex flex-wrap gap-1.5 flex-[1_1_240px] min-w-0 max-h-[84px] p-1 -m-1">
                  {valid.map((a) => (
                    <button key={a.id} type="button" className="chip chip-sm" aria-pressed={picked.includes(a.id)} onClick={() => toggle(a.id)}>
                      <span aria-hidden="true" className="w-2 h-2 rounded-full" style={{ background: avatarColor(a) }} />
                      {accountName(a)}
                    </button>
                  ))}
                </div>
              ) : (
                <span className="text-dim">{t("servers.noValidAccount")}</span>
              )}
              <button
                className="btn btn-primary ms-auto"
                disabled={!canJoin}
                title={target ? (blocked ?? undefined) : t("servers.pickServerFirst")}
                onClick={() => void join()}
              >
                <Icon name="play" size={12} fill />
                {step ? t("servers.joining") : t("servers.joinWith", { count: n })}
              </button>
            </div>
            <div aria-live="polite">{warning && <p className="alert mt-2">{warning}</p>}</div>
          </div>
        </>
      )}
    </main>
  );
}

function Note({ children }: { children: ReactNode }) {
  return <div className="flex-1 grid place-items-center p-8 text-dim">{children}</div>;
}

function Updated({ at }: { at: number }) {
  const s = Math.max(0, Math.round((useNow(1000) - at) / 1000));
  return <span className="mono text-xs text-dim">{s < 60 ? t("servers.updatedSec", { s }) : t("servers.updatedMin", { m: Math.floor(s / 60) })}</span>;
}

function GameHead({ game: g }: { game: GameInfo }) {
  return (
    <div className="flex items-center gap-5 min-w-0">
      {g.iconUrl ? (
        <img src={g.iconUrl} alt="" className="w-16 h-16 flex-none rounded-2xl object-cover" />
      ) : (
        <div aria-hidden="true" className="well w-16 h-16 flex-none grid place-content-center">
          <span className="mono text-[10px] text-dim">{t("servers.icon")}</span>
        </div>
      )}
      <div className="flex flex-col gap-1 min-w-0">
        <div className="flex items-baseline gap-2 min-w-0">
          <h1 className="page-title truncate min-w-0">{g.name}</h1>
          <span className="text-dim truncate flex-none max-w-[40%]">{t("servers.byCreator", { creator: g.creator })}</span>
        </div>
        <span className="mono text-xs text-dim selectable">{t("servers.placeUniverse", { place: g.placeId, universe: g.universeId })}</span>
        <span className="flex items-center gap-1.5">
          <span aria-hidden="true" className="w-1.5 h-1.5 bg-accent-text" />
          {tx(
            "servers.playing",
            { count: g.playing, num: <span className="mono">{fmtNum(g.playing)}</span> },
            { dim: (s) => <span className="text-dim">{s}</span> },
          )}
        </span>
      </div>
    </div>
  );
}
