import { useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { motion } from "motion/react";
import { useInfiniteQuery, useQuery, useQueryClient, type InfiniteData } from "@tanstack/react-query";
import { api, asError, type AccountSummary, type GameInfo, type LaunchTarget, type PrivateServer, type ServerOrder } from "../../api";
import { qk } from "../../data";
import { announce } from "../../ui/a11y";
import { Skeleton } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { Segmented, Switch } from "../../ui/controls";
import { Input } from "../../ui/Field";
import { Icon } from "../../ui/Icon";
import { Collapse, lid, SPRING, TWEEN } from "../../ui/motion";
import { MenuLabel, MenuRadioItem, MenuSeparator, Popover, useAnchor } from "../../ui/Popover";
import { friendly } from "../../ui/Toast";
import { fmtNum, t, tx, type Key } from "../../i18n";
import { accountName } from "../accounts/shared";
import { useNow } from "../launch/Running";
import { BuyPrivateServerDialog } from "./BuyPrivateServerDialog";
import { PrivateServerDialog } from "./PrivateServerDialog";
import { PrivateTable, PublicTable, short, type Pick } from "./ServerTables";
import "./servers.css";

const PAGE = 25;
const SIGNED_IN: ServerOrder[] = ["recommended", "bestLatency"];
const ORDERS: { value: ServerOrder; label: Key }[] = [
  { value: "bestLatency", label: "servers.orderBestPing" },
  { value: "occupancyDesc", label: "servers.orderMostFull" },
  { value: "occupancyAsc", label: "servers.orderMostEmpty" },
  { value: "recommended", label: "servers.orderRecommended" },
];

/** The server a browser has picked, for whoever joins it. */
export type ServerChoice = {
  /** What joining launches; null until a server is picked (a private one also needs its access code). */
  target: LaunchTarget | null;
  /** Free slots in the picked server, when known. */
  free: number | null;
  /** "1a2b3c4d…9f0e · 12 free", a private server's name, or "No server picked"; never an access code. */
  summary: string;
  /** The asking account can't join the picked private server. */
  restricted: boolean;
};

/** New content (a game, a list) fades in where the old one was. */
const FadeIn = ({ className, children }: { className?: string; children: ReactNode }) => (
  <motion.div className={className} initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={TWEEN.fade}>
    {children}
  </motion.div>
);

/** Finds a server of a game, public or private, and reports the pick. Roblox rate-limits the list,
 * so pages load only when asked for. The Servers view and Play's server drawer share it and its
 * query keys: one cache, one rate limit. It fills its parent's height (a flex column). */
export function ServerBrowser({ accounts, asker, place, setPlace, onChoice, variant = "page", onSubmit }: {
  accounts: AccountSummary[];
  /** The account that asks Roblox (signed-in sorts, private servers); undefined: guest requests only. */
  asker: number | undefined;
  place: number | null;
  setPlace: (placeId: number) => void;
  /** Called when the choice changes (not on every render), and once on mount. */
  onChoice: (c: ServerChoice) => void;
  /** drawer: a compact header, ids suffixed "-drawer" (servers-game-drawer), no page-only chrome. */
  variant?: "page" | "drawer";
  /** Ctrl+Enter inside the browser (Servers: join; the drawer: use this server). */
  onSubmit?: () => void;
}) {
  const qc = useQueryClient();
  const drawer = variant === "drawer";
  const field = drawer ? "servers-game-drawer" : "servers-game";
  const radio = useId();
  const reasonId = useId();
  const noteId = useId();
  const [text, setText] = useState(place === null ? "" : String(place));
  const [textError, setTextError] = useState<string | null>(null);
  const [view, setView] = useState<"public" | "private">("public");
  // null: the default, Fastest when an account asks.
  const [order, setOrder] = useState<ServerOrder | null>(null);
  const [hideFull, setHideFull] = useState(true);
  const [pick, setPick] = useState<Pick>(null);
  const [loading, setLoading] = useState(false);
  const [nudges, setNudges] = useState(0);
  const [manage, setManage] = useState<{ server: PrivateServer; owner: AccountSummary } | null>(null);
  const [buying, setBuying] = useState(false);
  // Another game from outside (Play re-seeding its drawer) starts over: a server picked in one game
  // must not travel to the next.
  const [shown, setShown] = useState(place);
  // An account that starts asking switches the default sort to Fastest; a server already picked from
  // the list keeps that list instead.
  const [asked, setAsked] = useState(asker !== undefined);
  if (place !== shown) {
    setShown(place);
    setText(place === null ? "" : String(place));
    setTextError(null);
    setView("public");
    setPick(null);
  } else if ((asker !== undefined) !== asked) {
    setAsked(asker !== undefined);
    if (order === null && pick?.kind === "public" && asker !== undefined) setOrder("occupancyDesc");
  }

  const valid = accounts.filter((a) => a.status === "valid");
  const asking = asker === undefined ? undefined : accounts.find((a) => a.id === asker);
  const askerName = asking ? accountName(asking) : "";
  const effOrder: ServerOrder = order ?? (asker !== undefined ? "bestLatency" : "occupancyDesc");
  const signedIn = SIGNED_IN.includes(effOrder);
  // Recommended and Best ping need an account: without one, nothing is asked for.
  const needsAccount = asker === undefined && signedIn;
  const locked = (o: ServerOrder) => asker === undefined && SIGNED_IN.includes(o);
  // A private server is yours when a valid vault account owns it; that account manages it.
  const owners = new Map(valid.map((a) => [a.userId, a]));

  // A game without its icon (failed or still being made) is asked for again next time.
  const game = useQuery({
    queryKey: qk.game(place),
    queryFn: () => api.gameInfo(place!),
    enabled: place !== null,
    staleTime: (q) => (q.state.data?.iconUrl ? Infinity : 0),
  });
  const g = game.data;
  const waiting = place !== null && game.isPending;
  // Not gated on g.privateServers: that guest hint is false for games whose servers exist anyway.
  // Signed-in sorts are per account, so the asker is in their key; guest sorts don't reload when
  // other accounts are picked.
  const pubKey = [...qk.servers, place, effOrder, hideFull, signedIn ? asker : null];
  const pub = useInfiniteQuery({
    queryKey: pubKey,
    // Every fetch path (Load next, Try again, refetch) goes through here: no guest request for a
    // signed-in sort.
    queryFn: ({ pageParam }) =>
      needsAccount
        ? Promise.reject({ kind: "invalid", message: t("servers.needAccountSort") })
        : api.serversList(place!, effOrder, hideFull, PAGE, pageParam ?? undefined, asker),
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
  const joinRestricted = !!priv.data?.pages.some((p) => p.joinRestricted);

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
  const choice: ServerChoice = { target, free, summary, restricted: !!privPick && joinRestricted };
  const report = useRef(onChoice);
  report.current = onChoice;
  // A fresh object each render: reported only when what it says changes.
  const said = JSON.stringify(choice);
  useEffect(() => {
    report.current(choice);
  }, [said]);

  // A lookup that finishes after a newer Load (or after leaving the view) is dropped.
  const loads = useRef(0);
  useEffect(() => () => void loads.current++, []);

  async function load(e: FormEvent) {
    e.preventDefault();
    if (loading) return;
    const mine = ++loads.current;
    const p = await api.targetParse(text).catch(() => null);
    if (mine !== loads.current) return;
    const tgt = p?.kind === "target" ? p.target : null;
    if (!tgt || tgt.kind === "followUser") {
      setTextError(p && "reason" in p ? p.reason : t("servers.pasteLink"));
      return;
    }
    // Universe and share links need a lookup (share links with the asking account).
    setLoading(true);
    try {
      const id = "placeId" in tgt ? tgt.placeId : await api.targetPlace(tgt, asker);
      if (mine !== loads.current) return;
      // Loading the game already shown asks for its icon again if it was missing.
      if (id === place && g && !g.iconUrl) void game.refetch();
      setShown(id);
      setPlace(id);
      setTextError(null);
      setView("public");
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
  const privChanged = () => void qc.invalidateQueries({ queryKey: [...qk.privateServers, place] });

  const pickOrder = (o: ServerOrder) => {
    if (!locked(o)) return setOrder(o);
    // Why it can't sort that way is already on screen: point at it.
    setNudges((n) => n + 1);
    announce(t("servers.needAccountSort"));
  };

  const submitKeys = (e: KeyboardEvent) => {
    if (!onSubmit || e.key !== "Enter" || !(e.ctrlKey || e.metaKey)) return;
    e.preventDefault();
    // The view around it (Play) may have its own Ctrl+Enter.
    e.stopPropagation();
    onSubmit();
  };

  const gameError = game.error && asError(game.error);
  const firstPage = (q: { isPending: boolean; isFetching: boolean }) => q.isPending && q.isFetching;
  const status = waiting ? t("servers.loadingGame") : !g ? "" : list.isFetching ? t("servers.loadingServers") : list.isSuccess ? t("servers.serversListed", { count }) : "";
  const shake = nudges ? (nudges % 2 ? "shake" : "shake2") : "";

  const listing =
    view === "public" ? (
      needsAccount && !pub.data ? (
        // The sort picked needs the account that went away: why, and the way on without one.
        <div className="grid place-items-center gap-4 py-10 px-6 text-center">
          <p id={reasonId} className={`flex items-center gap-2 text-fg-2 ${shake}`}>
            <Icon name="lock" size={14} />
            {t("servers.needAccountSort")}
          </p>
          <Button
            size="md"
            onClick={(e) => {
              const srv = e.currentTarget.closest(".srv")!;
              flushSync(() => setOrder("occupancyDesc"));
              // This button goes with the reason: the focus moves to the sort it picked.
              [...srv.querySelectorAll<HTMLElement>(".srv-sorts [aria-pressed=true], .srv-sort-menu")].find((el) => el.checkVisibility())?.focus();
            }}
          >
            {t("servers.sortBy", { order: t("servers.orderMostFull") })}
          </Button>
        </div>
      ) : (
        <>
          {effOrder === "bestLatency" && servers.length > 0 && <p className="meta px-5 pt-3">{t("servers.samePcNote")}</p>}
          <PublicTable
            servers={firstPage(pub) ? null : servers}
            pick={pubPick}
            fastest={effOrder === "bestLatency"}
            radio={radio}
            onPick={(id) => setPick({ kind: "public", id })}
          />
        </>
      )
    ) : asker === undefined ? (
      <p id={noteId} className="py-10 px-6 text-center text-fg-2">{valid.length ? t("servers.pickAccountPrivate") : t("servers.noValidAccount")}</p>
    ) : (
      <>
        <p className="meta px-5 pt-3">{t("servers.asName", { name: askerName })}</p>
        {joinRestricted && <p className="alert mx-5 mt-2">{t("servers.noJoin", { name: askerName })}</p>}
        <PrivateTable
          servers={firstPage(priv) ? null : privates}
          pick={privPick?.vipServerId ?? null}
          radio={radio}
          onPick={(id) => setPick({ kind: "private", id })}
          owners={owners}
          onManage={(server, owner) => setManage({ server, owner })}
        />
      </>
    );

  return (
    <>
      {/* As tall as the room it gets, never shorter than its header, toolbar and smallest table (servers.css). */}
      <div className="srv flex-1 flex flex-col gap-4" onKeyDown={submitKeys}>
        <div className="srv-head">
          <FadeIn key={g?.placeId ?? (waiting ? "wait" : "none")} className="flex-[1_1_320px] min-w-0">
            {g ? (
              <GameHead game={g} drawer={drawer} />
            ) : (
              <>
                {waiting && <GameHeadGhost drawer={drawer} />}
                {!drawer && <h1 className={waiting ? "sr-only" : "page-title"}>{t("servers.title")}</h1>}
              </>
            )}
          </FadeIn>
          <form className={`flex flex-col min-w-[220px] ${drawer ? "flex-[1_1_320px]" : "ms-auto flex-[0_1_320px]"}`} onSubmit={(e) => void load(e)}>
            <div className="flex gap-2">
              <label htmlFor={field} className="sr-only">{t("servers.gameInput")}</label>
              <Input
                id={field}
                mono
                autoFocus={!drawer && place === null}
                data-find={drawer ? undefined : ""}
                className="flex-1 min-w-0"
                placeholder={t("servers.gameInput")}
                autoComplete="off"
                value={text}
                invalid={!!textError}
                aria-describedby={textError ? `${field}-error` : undefined}
                onChange={(e) => {
                  setText(e.target.value);
                  setTextError(null);
                }}
              />
              {/* Busy buttons stay focusable (aria-disabled, and a guard): disabled would drop the focus of whoever pressed them. */}
              <Button type="submit" busy={loading} aria-disabled={loading || undefined}>{t("servers.load")}</Button>
            </div>
            <Collapse open={!!textError}>
              <p id={`${field}-error`} role="alert" className="alert text-[13px] pt-1.5">{textError}</p>
            </Collapse>
          </form>
        </div>

        {place === null ? (
          <div className="empty flex-1 content-center">
            <Icon name="servers" size={22} className="text-fg-3" />
            <p>{t("servers.pastePrompt")}</p>
          </div>
        ) : waiting ? (
          <>
            <div aria-hidden="true" className="flex items-center gap-3">
              <Skeleton width={148} height={34} radius={999} />
              <div className="srv-sorts">
                <Skeleton width={360} height={34} radius={999} />
              </div>
            </div>
            <div className="panel srv-panel scroll flex-1 min-h-[160px] px-2 pb-2">
              <PublicTable servers={null} pick={null} fastest={false} radio={radio} onPick={() => {}} />
            </div>
          </>
        ) : !g ? (
          <div className="empty flex-1 content-center">
            <p role="alert" className="alert">{gameError?.kind === "notFound" ? t("servers.noGame") : gameError && friendly(gameError)}</p>
            {gameError?.kind !== "notFound" && (
              <Button size="md" icon="refresh" busy={game.isFetching} aria-disabled={game.isFetching || undefined} onClick={() => !game.isFetching && void game.refetch()}>{t("common.tryAgain")}</Button>
            )}
          </div>
        ) : (
          <>
            <FadeIn className="flex flex-col">
              <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
                <Segmented
                  label={t("servers.serverType")}
                  value={view}
                  options={[{ value: "public", label: t("servers.public") }, { value: "private", label: t("servers.private") }]}
                  onChange={(v) => {
                    setView(v);
                    setPick(null);
                  }}
                />
                {view === "public" ? (
                  <>
                    <Sorts value={effOrder} locked={locked} reasonId={reasonId} fastestFor={asking?.username} onPick={pickOrder} />
                    <label className="flex items-center gap-2.5 text-[13px] font-medium text-fg-2">
                      <Switch label={t("servers.hideFullServers")} checked={hideFull} onChange={setHideFull} />
                      <span aria-hidden="true">{t("servers.hideFull")}</span>
                    </label>
                  </>
                ) : (
                  <Button size="md" icon="plus" disabled={!valid.length} aria-describedby={valid.length ? undefined : noteId} onClick={() => setBuying(true)}>
                    {t("servers.buy")}
                  </Button>
                )}
                <span className="flex items-center gap-3 ms-auto">
                  <span className="srv-updated meta tabular-nums">
                    {list.isFetching ? t("common.loading") : list.dataUpdatedAt > 0 && <Updated at={list.dataUpdatedAt} />}
                  </span>
                  <Button
                    size="md"
                    icon="refresh"
                    busy={list.isFetching}
                    aria-disabled={list.isFetching || undefined}
                    disabled={asker === undefined && (view === "private" || needsAccount)}
                    onClick={() => !list.isFetching && refresh()}
                  >
                    {t("servers.refresh")}
                  </Button>
                </span>
              </div>
              <Collapse open={view === "public" && asker === undefined && !needsAccount}>
                <p id={reasonId} className={`srv-sorts-why px-1 pt-2.5 text-[13px] text-fg-3 ${shake}`}>
                  <span className="inline-flex items-center gap-2">
                    <Icon name="lock" size={13} />
                    {t("servers.needAccountSort")}
                  </span>
                </p>
              </Collapse>
            </FadeIn>

            <div className="panel srv-panel scroll flex-1 min-h-[160px] px-2 pb-2">
              <FadeIn key={JSON.stringify(view === "public" ? pubKey : privKey)}>
                {listing}
                <div className="flex flex-col items-center gap-3 p-4 text-center text-fg-2 empty:hidden">
                  {list.isError ? (
                    <>
                      <p role="alert" className="alert text-start">{friendly(asError(list.error))}</p>
                      <Button
                        size="md"
                        icon="refresh"
                        busy={list.isFetching}
                        aria-disabled={list.isFetching || undefined}
                        disabled={view === "public" && needsAccount}
                        onClick={() => !list.isFetching && void (list.isFetchNextPageError ? list.fetchNextPage() : refresh())}
                      >
                        {t("common.tryAgain")}
                      </Button>
                    </>
                  ) : list.isSuccess && count === 0 ? (
                    <p>{view === "private" ? t("servers.noPrivate", { name: askerName }) : hideFull ? t("servers.noRoom") : t("servers.noServers")}</p>
                  ) : (
                    list.hasNextPage && (
                      <Button
                        size="md"
                        busy={list.isFetchingNextPage}
                        aria-disabled={list.isFetching || undefined}
                        disabled={view === "public" && needsAccount}
                        onClick={() => !list.isFetching && void list.fetchNextPage()}
                      >
                        {t("servers.loadNext", { n: PAGE })}
                      </Button>
                    )
                  )}
                </div>
              </FadeIn>
            </div>
          </>
        )}
        <span role="status" className="sr-only">{status}</span>
      </div>
      <PrivateServerDialog
        server={manage?.server ?? null}
        owner={manage?.owner ?? null}
        accounts={valid}
        onClose={() => setManage(null)}
        onChanged={privChanged}
      />
      {g && (
        <BuyPrivateServerDialog
          open={buying}
          onClose={() => setBuying(false)}
          game={g}
          accounts={valid}
          payer={asker}
          onBought={privChanged}
        />
      )}
    </>
  );
}

/** The sorts: a segmented row, or a "Sort: …" menu where the browser is narrow (servers.css). A sort
 * that needs a signed-in account stays focusable but locked, and points at the reason shown under
 * the toolbar. */
function Sorts({ value, locked, reasonId, fastestFor, onPick }: {
  value: ServerOrder;
  locked: (o: ServerOrder) => boolean;
  reasonId: string;
  /** The asking account's username: Best ping reads "Fastest for @name". */
  fastestFor: string | undefined;
  onPick: (o: ServerOrder) => void;
}) {
  const id = useId();
  const anchor = useAnchor();
  const menu = `${id}sorts`;
  const label = (o: (typeof ORDERS)[number]) => (o.value === "bestLatency" && fastestFor ? t("servers.orderFastestFor", { name: fastestFor }) : t(o.label));
  const current = ORDERS.find((o) => o.value === value)!;
  return (
    <>
      <div role="group" aria-label={t("servers.sortServers")} className="seg-group srv-sorts" data-thumb>
        {ORDERS.map((o) => {
          const on = o.value === value;
          const lock = locked(o.value);
          return (
            <button
              key={o.value}
              type="button"
              className="seg inline-flex items-center gap-1.5"
              data-choice
              aria-pressed={on}
              aria-disabled={lock || undefined}
              aria-describedby={lock ? reasonId : undefined}
              onClick={() => !on && onPick(o.value)}
            >
              {on && <motion.span layoutId={lid("srv-sort", id, "thumb")} className="seg-thumb" style={{ inset: 0, zIndex: -1, borderRadius: 999 }} transition={SPRING.snappy} />}
              {lock && <Icon name="lock" size={12} />}
              {label(o)}
            </button>
          );
        })}
      </div>
      <button type="button" className="btn btn-sm srv-sort-menu" popoverTarget={menu} style={anchor.style}>
        {t("servers.sortBy", { order: label(current) })}
        <Icon name="chevron" size={14} />
      </button>
      <Popover
        id={menu}
        anchor={anchor.name}
        role="menu"
        label={t("servers.sortServers")}
        onToggle={(open) => open && document.getElementById(menu)?.querySelector<HTMLElement>("[aria-checked=true]")?.focus()}
      >
        {ORDERS.map((o) => (
          <MenuRadioItem
            key={o.value}
            checked={o.value === value}
            disabled={locked(o.value)}
            onSelect={() => {
              onPick(o.value);
              document.getElementById(menu)?.hidePopover();
            }}
          >
            {label(o)}
          </MenuRadioItem>
        ))}
        {ORDERS.some((o) => locked(o.value)) && (
          <>
            <MenuSeparator />
            <MenuLabel>{t("servers.needAccountSort")}</MenuLabel>
          </>
        )}
      </Popover>
    </>
  );
}

function Updated({ at }: { at: number }) {
  const s = Math.max(0, Math.round((useNow(1000) - at) / 1000));
  return <>{s < 60 ? t("servers.updatedSec", { s }) : t("servers.updatedMin", { m: Math.floor(s / 60) })}</>;
}

function GameHead({ game: g, drawer }: { game: GameInfo; drawer: boolean }) {
  const Name = drawer ? "h3" : "h1";
  return (
    <div className="flex items-center gap-4 min-w-0">
      {g.iconUrl ? (
        <img src={g.iconUrl} alt="" className="srv-icon object-cover" />
      ) : (
        <span aria-hidden="true" className="srv-icon srv-icon-none">
          <Icon name="grid" size={20} />
        </span>
      )}
      <div className="flex-1 flex flex-col gap-0.5 min-w-0">
        <div className="flex items-baseline gap-2 min-w-0">
          <Name className={`${drawer ? "section-title" : "page-title"} truncate min-w-0`}>{g.name}</Name>
          <span className="text-fg-2 truncate flex-none max-w-[40%]">{t("servers.byCreator", { creator: g.creator })}</span>
        </div>
        {!drawer && <span className="srv-ids mono meta selectable">{t("servers.placeUniverse", { place: g.placeId, universe: g.universeId })}</span>}
        <span className="flex items-center gap-2 text-[13px]">
          <span aria-hidden="true" className="status-mark text-ok" />
          {tx(
            "servers.playing",
            { count: g.playing, num: <span className="mono">{fmtNum(g.playing)}</span> },
            { dim: (s) => <span className="text-fg-3">{s}</span> },
          )}
        </span>
      </div>
    </div>
  );
}

/** GameHead's shape while the game loads. */
function GameHeadGhost({ drawer }: { drawer: boolean }) {
  return (
    <div aria-hidden="true" className="flex items-center gap-4">
      <span className="skel srv-icon" style={{ borderRadius: "var(--r-md)" }} />
      <div className="flex flex-col gap-2">
        <Skeleton width={drawer ? 160 : 220} height={drawer ? 16 : 24} />
        {!drawer && (
          <div className="srv-ids">
            <Skeleton width={250} />
          </div>
        )}
        <Skeleton width={110} />
      </div>
    </div>
  );
}
