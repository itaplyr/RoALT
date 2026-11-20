// Browse's own tabs, drawn by RoALT rather than roblox.com: Popular (Roblox's Charts) and Playtime.
import { useState } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
import { api, asError, type AccountSummary, type GameInfo, type PopularGame } from "../../api";
import { qk } from "../../data";
import { fmtNum, t, tx } from "../../i18n";
import { Skeleton } from "../../ui/Bits";
import { Button, IconButton } from "../../ui/Button";
import { Segmented } from "../../ui/controls";
import { Select } from "../../ui/Field";
import { Icon } from "../../ui/Icon";
import { friendly } from "../../ui/Toast";
import { accountName, ago, Avatar, formatDate } from "../accounts/shared";
import { exportCsv } from "../crashes/exportCsv";
import { uptime } from "../launch/Running";
import { copyRoaltLink } from "../launch/target";
// GameIcon is also drawn by Servers and Play's game details, which load without BrowseView.
import "./browse.css";

/** What a game's card can do: open its page here, play it as the account browsing, or show its details. */
type Actions = { onOpen: (rootPlaceId: number) => void; onPlay: (placeId: number) => void; onDetails: (placeId: number) => void };

export function GameIcon({ url }: { url: string | null | undefined }) {
  return url ? (
    <img src={url} alt="" className="browse-icon" loading="lazy" />
  ) : (
    <span aria-hidden="true" className="browse-icon browse-icon-none">
      <Icon name="grid" size={20} />
    </span>
  );
}

export function Problem({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  return (
    <div className="flex flex-col items-start gap-3">
      <p role="alert" className="alert">{friendly(asError(error))}</p>
      <Button size="md" icon="refresh" onClick={onRetry}>{t("common.tryAgain")}</Button>
    </div>
  );
}

/** Roblox's Charts (Top Trending, Up-and-Coming, ...) without the ads, as cards. */
export function PopularTab(actions: Actions) {
  const q = useQuery({ queryKey: ["popular"], queryFn: api.gamesPopular, staleTime: 10 * 60_000 });
  const [sort, setSort] = useState<string | null>(null);
  if (q.error) return <Problem error={q.error} onRetry={() => q.refetch()} />;
  if (!q.data) return <Skeleton width="100%" height={180} />;
  if (!q.data.length) return <p className="text-fg-2">{t("browse.popularNone")}</p>;
  const current = q.data.find((s) => s.id === sort) ?? q.data[0];
  return (
    <div className="flex flex-col gap-4">
      <div role="group" aria-label={t("browse.lists")} className="flex flex-wrap gap-1.5">
        {q.data.map((s) => (
          <button key={s.id} type="button" className="chip" aria-pressed={s.id === current.id} onClick={() => setSort(s.id)}>
            {s.name}
          </button>
        ))}
      </div>
      <ul className="browse-grid">
        {current.games.map((g) => (
          <li key={g.universeId}>
            <PopularCard game={g} {...actions} />
          </li>
        ))}
      </ul>
    </div>
  );
}

function PopularCard({ game: g, onOpen, onPlay, onDetails }: { game: PopularGame } & Actions) {
  const votes = g.upVotes + g.downVotes;
  return (
    <article className="panel browse-card">
      <div className="flex items-center gap-3 min-w-0">
        <GameIcon url={g.iconUrl} />
        <div className="flex flex-col gap-0.5 min-w-0">
          <h3 className="font-medium truncate">{g.name}</h3>
          <span className="text-[13px] text-fg-2 truncate">
            {tx("servers.playing", { count: g.playing, num: <span className="mono">{fmtNum(g.playing)}</span> }, { dim: (s) => <span className="text-fg-3">{s}</span> })}
            {votes > 0 && <> · {t("browse.liked", { n: Math.round((100 * g.upVotes) / votes) })}</>}
          </span>
        </div>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button size="sm" icon="play" onClick={() => onPlay(g.rootPlaceId)}>{t("browse.play")}</Button>
        <Button size="sm" variant="ghost" icon="globe" onClick={() => onOpen(g.rootPlaceId)}>{t("browse.open")}</Button>
        <Button size="sm" variant="ghost" icon="info" onClick={() => onDetails(g.rootPlaceId)}>{t("game.details")}</Button>
        <IconButton icon="link" size="sm" label={t("share.copyLink")} onClick={() => copyRoaltLink({ kind: "place", placeId: g.rootPlaceId, launchData: null })} />
      </div>
    </article>
  );
}

/** A game's time: its places together (a teleport moves to another place of the same game). */
type Played = { key: string; game: GameInfo | undefined; placeId: number; ms: number; last: number; byAccount: Map<number, number> };

/** How long each account spent in each game, from the clients RoALT followed. */
export function PlaytimeTab({ accounts, onOpen, onPlay, onDetails }: { accounts: AccountSummary[] } & Actions) {
  const q = useQuery({ queryKey: ["playtime"], queryFn: api.playtimeList, refetchInterval: 60_000 });
  const places = [...new Set((q.data ?? []).map((r) => r.placeId))];
  const games = useQueries({
    queries: places.map((placeId) => ({ queryKey: qk.game(placeId), queryFn: () => api.gameInfo(placeId), staleTime: 10 * 60_000, retry: false })),
  });
  if (q.error) return <Problem error={q.error} onRetry={() => q.refetch()} />;
  if (!q.data) return <Skeleton width="100%" height={180} />;
  if (!q.data.length) return <p className="text-fg-2 max-w-prose">{t("browse.playtimeNone")}</p>;

  const info = new Map(places.map((p, i) => [p, games[i]?.data]));
  const byGame = new Map<string, Played>();
  for (const r of q.data) {
    const game = info.get(r.placeId);
    // Grouped by game once its universe is known; by place until then.
    const key = game ? `u${game.universeId}` : `p${r.placeId}`;
    const g = byGame.get(key) ?? { key, game, placeId: game?.rootPlaceId ?? r.placeId, ms: 0, last: 0, byAccount: new Map() };
    g.ms += r.ms;
    g.last = Math.max(g.last, r.lastPlayed);
    g.byAccount.set(r.accountId, (g.byAccount.get(r.accountId) ?? 0) + r.ms);
    byGame.set(key, g);
  }
  const list = [...byGame.values()].sort((a, b) => b.ms - a.ms);
  const name = new Map(accounts.map((a) => [a.id, a]));

  return (
    <div className="flex flex-col gap-3">
      <p className="text-[13px] text-fg-3 max-w-prose">{t("browse.playtimeHint")}</p>
      <PlaytimeDays accounts={accounts} />
      <ul className="flex flex-col gap-2">
        {list.map((g) => (
          <li key={g.key} className="panel browse-row">
            <GameIcon url={g.game?.iconUrl} />
            <div className="flex-1 min-w-0 flex flex-col gap-1">
              <div className="flex items-baseline gap-2 min-w-0">
                <h3 className="font-medium truncate">{g.game?.name ?? t("browse.place", { id: g.placeId })}</h3>
                <span className="mono text-fg-2 flex-none">{uptime(0, g.ms)}</span>
              </div>
              <div className="flex flex-wrap gap-1.5">
                {[...g.byAccount.entries()]
                  .sort((x, y) => y[1] - x[1])
                  .map(([id, ms]) => {
                    const a = name.get(id);
                    return (
                      a && (
                        <span key={id} className="chip chip-sm">
                          <Avatar account={a} size={16} />
                          <span className="truncate">{accountName(a)}</span>
                          <span className="mono text-fg-3">{uptime(0, ms)}</span>
                        </span>
                      )
                    );
                  })}
              </div>
              <span className="text-[13px] text-fg-3">{t("browse.lastPlayed", { when: ago(g.last) })}</span>
            </div>
            <div className="flex flex-col gap-1.5 flex-none">
              <Button size="sm" icon="play" onClick={() => onPlay(g.placeId)}>{t("browse.play")}</Button>
              <Button size="sm" variant="ghost" icon="globe" onClick={() => onOpen(g.placeId)}>{t("browse.open")}</Button>
              <Button size="sm" variant="ghost" icon="info" onClick={() => onDetails(g.placeId)}>{t("game.details")}</Button>
              <Button size="sm" variant="ghost" icon="link" onClick={() => copyRoaltLink({ kind: "place", placeId: g.placeId, launchData: null })}>{t("share.copyLink")}</Button>
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}

const SPANS = [7, 30, 90] as const;
/** A local day as the backend keeps it (YYYYMMDD), and back. */
const dayOf = (d: Date) => d.getFullYear() * 10_000 + (d.getMonth() + 1) * 100 + d.getDate();
const dateOf = (day: number) => new Date(Math.floor(day / 10_000), (Math.floor(day / 100) % 100) - 1, day % 100).getTime();

/** Time in games per day, of every account or one: a bar a day, and the days as a CSV file. Nothing
 * shows until a day was counted (RoALT counts days from the version that brought this). */
function PlaytimeDays({ accounts }: { accounts: AccountSummary[] }) {
  const q = useQuery({ queryKey: ["playtimeDays"], queryFn: api.playtimeDays, refetchInterval: 60_000 });
  const [span, setSpan] = useState<(typeof SPANS)[number]>(30);
  const [only, setOnly] = useState("");
  if (!q.data?.length) return null;

  const had = accounts.filter((a) => q.data.some((r) => r.accountId === a.id));
  const picked = had.some((a) => String(a.id) === only) ? only : "";
  const sum = new Map<number, number>();
  for (const r of q.data) if (!picked || String(r.accountId) === picked) sum.set(r.day, (sum.get(r.day) ?? 0) + r.ms);
  const days = Array.from({ length: span }, (_, i) => {
    const d = new Date();
    d.setDate(d.getDate() - (span - 1 - i));
    return dayOf(d);
  });
  const most = Math.max(1, ...days.map((d) => sum.get(d) ?? 0));
  const total = days.reduce((n, d) => n + (sum.get(d) ?? 0), 0);
  const summary = t("playtime.totalIn", { time: uptime(0, total), n: span });

  return (
    <section aria-label={t("playtime.byDay")} className="panel flex flex-col gap-3 p-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="font-medium">{t("playtime.byDay")}</h3>
        <span className="flex flex-wrap items-center gap-2">
          <Segmented label={t("playtime.span")} value={span} onChange={setSpan} options={SPANS.map((n) => ({ value: n, label: t("playtime.days", { n }) }))} />
          {had.length > 1 && (
            <Select
              aria-label={t("monitor.account")}
              value={picked}
              onChange={setOnly}
              options={[{ value: "", label: t("common.allAccounts") }, ...had.map((a) => ({ value: String(a.id), label: accountName(a) }))]}
            />
          )}
          <Button size="sm" variant="ghost" icon="download" onClick={() => void exportCsv("RoALT-playtime", api.playtimeExport)}>
            {t("common.exportCsv")}
          </Button>
        </span>
      </div>
      {/* One picture to a screen reader, with its total; the days themselves are in the CSV. */}
      <div role="img" aria-label={summary} className="playtime-bars">
        {days.map((d) => {
          const ms = sum.get(d) ?? 0;
          return <span key={d} className="playtime-bar" data-empty={ms ? undefined : ""} style={{ height: ms ? `${Math.max(4, (100 * ms) / most)}%` : undefined }} data-tip={`${formatDate(dateOf(d))} · ${uptime(0, ms)}`} />;
        })}
      </div>
      <p className="meta">
        {summary} · {t("playtime.kept")}
      </p>
    </section>
  );
}