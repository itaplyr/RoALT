// A Roblox player's details from Roblox's own numbers: who they are, where they are now, their
// friends, groups and Roblox badges, the games they made, and their limiteds (RAP; Rolimons values
// when that's on). Opened from Browse's toolbar on a profile page, as the account browsing.
import { useId, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, type PlayerStats } from "../../api";
import { qk, useSettings } from "../../data";
import { fmtDate, fmtNum, t } from "../../i18n";
import { Skeleton } from "../../ui/Bits";
import { Button, IconButton } from "../../ui/Button";
import { Drawer } from "../../ui/Drawer";
import { num, StatGrid, type Stat } from "../../ui/StatGrid";
import { Tabs } from "../../ui/Tabs";
import { TopItems, ValueFacts } from "../accounts/Limiteds";
import { Listed, useGameDetails } from "./GameDetails";
import { Problem } from "./Native";

/** `open(userId)` shows that player's details as `accountId`; render `dialog` once where it's used. */
export function usePlayerDetails(accountId: number) {
  const [open, setOpen] = useState(false);
  // Stays while the drawer slides out.
  const [user, setUser] = useState<number | null>(null);
  const titleId = useId();
  const game = useGameDetails();
  const close = () => setOpen(false);
  const dialog = (
    <>
      <Drawer open={open} onClose={close} labelledBy={titleId} width={600}>
        {user !== null && (
          <Details
            key={`${accountId}:${user}`}
            accountId={accountId}
            userId={user}
            titleId={titleId}
            onClose={close}
            // One sheet at a time: this one goes, the game's comes.
            onGame={(placeId) => (close(), game.open(placeId))}
          />
        )}
      </Drawer>
      {game.dialog}
    </>
  );
  return {
    open: (userId: number) => {
      setUser(userId);
      setOpen(true);
    },
    dialog,
  };
}

function Details({ accountId, userId, titleId, onClose, onGame }: { accountId: number; userId: number; titleId: string; onClose: () => void; onGame: (placeId: number) => void }) {
  // The backend keeps the profile a while; where they are now is asked on each opening.
  const stats = useQuery({ queryKey: qk.player(userId), queryFn: () => api.playerStats(accountId, userId), staleTime: 0 });
  const p = stats.data;
  return (
    <>
      <div className="dlg-head">
        {p?.headshotUrl ? <img src={p.headshotUrl} alt="" className="browse-icon" /> : <span aria-hidden="true" className="skel browse-icon" />}
        <div className="flex-1 min-w-0 flex flex-col gap-0.5">
          <h2 id={titleId} className="dlg-title truncate">
            {p?.displayName ?? t("player.detailsTip")}
            {p?.verified && <span className="tag ms-2 align-middle">{t("player.verified")}</span>}
            {p?.banned && <span className="tag ms-2 align-middle" style={{ color: "var(--attention)" }}>{t("player.banned")}</span>}
          </h2>
          {p && <span className="text-[13px] text-fg-2 truncate">@{p.name}</span>}
        </div>
        <IconButton icon="x" label={t("common.close")} onClick={onClose} />
      </div>
      <div className="drawer-main scroll gap-5 pb-5">
        {/* A refresh that failed keeps what was there. */}
        {stats.error && !p ? (
          <Problem error={stats.error} onRetry={() => stats.refetch()} />
        ) : p ? (
          <Body p={p} accountId={accountId} onGame={onGame} />
        ) : (
          <div aria-hidden="true" className="flex flex-col gap-3">
            <Skeleton width="100%" height={96} />
            <Skeleton width="100%" height={220} />
          </div>
        )}
      </div>
    </>
  );
}

const DAY = 864e5;

/** Where they are now, as the browsing account is told. */
function now(p: PlayerStats): Stat[1] {
  const s = p.presence;
  if (!s) return null;
  if (s.kind === 2) return s.place ? t("player.inGameAt", { place: s.place }) : t("player.inGame");
  return t(s.kind === 1 ? "player.online" : s.kind === 3 ? "player.inStudio" : "player.offline");
}

/** A player's numbers, for the drawer and the card on their profile page. */
export function playerFacts(p: PlayerStats): Stat[] {
  const joined = p.created ? new Date(p.created) : null;
  const since = joined && !Number.isNaN(joined.getTime()) ? Date.now() - joined.getTime() : null;
  const years = since === null ? 0 : Math.floor(since / (365.25 * DAY));
  const visits = p.games && p.games.reduce((n, g) => n + g.visits, 0);
  return [
    ["player.stat.now", now(p)],
    ["player.stat.joined", joined && since !== null ? fmtDate(joined, { dateStyle: "medium" }) : null],
    ["player.stat.age", since === null ? null : years >= 1 ? t("player.ageYears", { count: years }) : t("player.ageDays", { count: Math.floor(since / DAY) })],
    num("player.stat.friends", p.friends),
    num("player.stat.followers", p.followers),
    num("player.stat.following", p.following),
    num("player.stat.groups", p.groups?.length ?? null),
    num("player.stat.badges", p.badges?.length ?? null),
    num("player.stat.gameVisits", visits),
  ];
}

function Body({ p, accountId, onGame }: { p: PlayerStats; accountId: number; onGame: (placeId: number) => void }) {
  const [tab, setTab] = useState<"groups" | "badges" | "games">("groups");
  const panelId = useId();
  const stats = playerFacts(p);
  const compact = (n: number) => fmtNum(n, { notation: "compact", maximumFractionDigits: 1 });
  return (
    <>
      <StatGrid stats={stats} />
      <Value accountId={accountId} userId={p.userId} />
      <div className="flex flex-col gap-3">
        <Tabs
          label={t("player.detailsTip")}
          value={tab}
          onChange={setTab}
          panelId={panelId}
          items={[
            { value: "groups", label: t("player.groups"), icon: "accounts", count: p.groups?.length },
            { value: "badges", label: t("player.badges"), icon: "shield", count: p.badges?.length },
            { value: "games", label: t("player.games"), icon: "grid", count: p.games?.length },
          ]}
        />
        <div id={panelId} role="tabpanel">
          {tab === "groups" ? (
            <Listed
              list={p.groups && p.groups.map((g) => ({ ...g, iconUrl: null }))}
              more={false}
              empty={t("player.noGroups")}
              side={(g) => (
                <span className="flex-none flex flex-col items-end text-[13px]">
                  <span>{g.role}</span>
                  <span className="text-fg-3" title={fmtNum(g.members)}>{t("player.members", { count: g.members, n: compact(g.members) })}</span>
                </span>
              )}
            />
          ) : tab === "badges" ? (
            <Listed list={p.badges && p.badges.map((b) => ({ ...b, iconUrl: b.imageUrl }))} more={false} empty={t("player.noBadges")} side={() => null} />
          ) : (
            <Listed
              list={p.games && p.games.map((g) => ({ ...g, id: g.universeId, iconUrl: null }))}
              more={p.moreGames}
              empty={t("player.noGames")}
              side={(g) => (
                <span className="flex-none flex items-center gap-3">
                  <span className="mono text-[13px] text-fg-3" title={fmtNum(g.visits)}>{t("player.visits", { count: g.visits, n: compact(g.visits) })}</span>
                  <Button size="sm" variant="ghost" icon="info" onClick={() => onGame(g.placeId)}>{t("game.details")}</Button>
                </span>
              )}
            />
          )}
        </div>
      </div>
    </>
  );
}

/** Their limiteds: counted on opening (or the recent count kept), again on asking. */
function Value({ accountId, userId }: { accountId: number; userId: number }) {
  const qc = useQueryClient();
  const rolimons = useSettings().data?.rolimonsValues ?? false;
  const key = qk.playerValue(accountId, userId);
  const value = useQuery({ queryKey: key, queryFn: () => api.playerValue(accountId, userId), staleTime: Infinity, retry: false });
  const recount = useMutation({
    mutationFn: () => api.playerValue(accountId, userId, true),
    onSuccess: (v) => {
      qc.setQueryData(key, v);
      // One of your own accounts shares the count.
      void qc.invalidateQueries({ queryKey: qk.values });
    },
  });
  const r = value.data;
  const v = r?.value;
  const busy = value.isFetching || recount.isPending;
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <h3 className="label text-fg-2">{t("value.title")}</h3>
        {/* Also after "private": who may see an inventory can change (a friend, the owner's setting). */}
        {r && (
          <Button size="sm" variant="ghost" icon="refresh" busy={busy} onClick={() => !busy && recount.mutate()}>
            {v ? t("value.countAgain") : t("value.count")}
          </Button>
        )}
      </div>
      {value.error && !r ? (
        <Problem error={value.error} onRetry={() => value.refetch()} />
      ) : !r ? (
        <p className="meta" role="status">{t("player.counting")}</p>
      ) : r.busy ? (
        <p className="meta">{t("player.busy")}</p>
      ) : r.private || !v ? (
        <p className="meta">{t("player.private")}</p>
      ) : (
        <>
          <ValueFacts v={v} rolimons={rolimons} />
          {v.top.length > 0 && (
            <>
              <h4 className="label text-fg-2 pt-1">{t("value.top")}</h4>
              <TopItems items={v.top} />
            </>
          )}
        </>
      )}
    </div>
  );
}
