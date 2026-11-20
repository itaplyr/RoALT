// A game's details from Roblox's own numbers: playing, visits, favourites, likes, its passes with
// their prices and its badges with how rare they are. Opened from Browse (Popular, Playtime, its
// toolbar on a game page) and the Servers view.
import { useId, useState, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, type GameBadge, type GamePass, type GameStats } from "../../api";
import { qk } from "../../data";
import { fmtDate, fmtNum, t } from "../../i18n";
import { Skeleton } from "../../ui/Bits";
import { IconButton } from "../../ui/Button";
import { Drawer } from "../../ui/Drawer";
import { num, StatGrid, type Stat } from "../../ui/StatGrid";
import { Tabs } from "../../ui/Tabs";
import { GameIcon, Problem } from "./Native";

/** `open(placeId)` shows that game's details; render `dialog` once where it's used. */
export function useGameDetails() {
  const [open, setOpen] = useState(false);
  // Stays while the drawer slides out.
  const [place, setPlace] = useState<number | null>(null);
  const titleId = useId();
  const close = () => setOpen(false);
  const dialog = (
    <Drawer open={open} onClose={close} labelledBy={titleId} width={600}>
      {place !== null && <Details key={place} placeId={place} titleId={titleId} onClose={close} />}
    </Drawer>
  );
  return {
    open: (placeId: number) => {
      setPlace(placeId);
      setOpen(true);
    },
    dialog,
  };
}

function Details({ placeId, titleId, onClose }: { placeId: number; titleId: string; onClose: () => void }) {
  // As the Servers view asks it: a game without its icon is asked for again.
  const game = useQuery({ queryKey: qk.game(placeId), queryFn: () => api.gameInfo(placeId), staleTime: (q) => (q.state.data?.iconUrl ? Infinity : 0) });
  const g = game.data;
  // The backend keeps a full answer for a while: asking on each opening is cheap.
  const stats = useQuery({ queryKey: qk.gameStats(g?.universeId ?? 0), queryFn: () => api.gameStats(g!.universeId), enabled: !!g, staleTime: 0 });
  const s = stats.data;
  const genre = s?.genre && (s.subgenre ? `${s.genre} · ${s.subgenre}` : s.genre);
  return (
    <>
      <div className="dlg-head">
        {g ? <GameIcon url={g.iconUrl} /> : <span aria-hidden="true" className="skel browse-icon" />}
        <div className="flex-1 min-w-0 flex flex-col gap-0.5">
          <h2 id={titleId} className="dlg-title truncate">
            {g?.name ?? t("game.detailsTip")}
            {s?.archived && <span className="tag ms-2 align-middle">{t("game.archived")}</span>}
          </h2>
          {g && <span className="text-[13px] text-fg-2 truncate">{genre ? `${t("servers.byCreator", { creator: g.creator })} · ${genre}` : t("servers.byCreator", { creator: g.creator })}</span>}
        </div>
        <IconButton icon="x" label={t("common.close")} onClick={onClose} />
      </div>
      <div className="drawer-main scroll gap-5 pb-5">
        {/* A refresh of the game that failed keeps what was there. */}
        {game.error && !g ? (
          <Problem error={game.error} onRetry={() => game.refetch()} />
        ) : stats.error && !s ? (
          <Problem error={stats.error} onRetry={() => stats.refetch()} />
        ) : s ? (
          <Body s={s} />
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

/** An ISO date as a short day ("Jan 16, 2019"); null when there's none. */
function day(iso: string | null): Stat[1] {
  const d = iso ? new Date(iso) : null;
  return d && !Number.isNaN(d.getTime()) ? fmtDate(d, { dateStyle: "medium" }) : null;
}

/** Roblox's avatar setting, as players know it. */
const avatar = (v: string) => (v === "MorphToR6" ? "R6" : v === "MorphToR15" ? "R15" : v === "PlayerChoice" ? t("game.avatarChoice") : v);

/** "Mild · 9+": Roblox's rating in its own words (English only), and the age it's for. */
function age(s: GameStats): Stat[1] {
  const r = s.ageRating;
  if (!r) return null;
  return r.minimumAge ? `${r.label} · ${t("game.ageFrom", { n: r.minimumAge })}` : r.label;
}

function Body({ s }: { s: GameStats }) {
  const [tab, setTab] = useState<"passes" | "badges">("passes");
  const panelId = useId();
  const votes = s.upVotes !== null && s.downVotes !== null ? s.upVotes + s.downVotes : null;
  const stats: Stat[] = [
    num("game.stat.playing", s.playing),
    num("game.stat.visits", s.visits),
    num("game.stat.favourites", s.favourites),
    [
      "game.stat.likes",
      votes === null ? null : votes ? t("settings.percent", { n: Math.round((100 * s.upVotes!) / votes) }) : "–",
      votes === null ? undefined : t("game.votes", { up: fmtNum(s.upVotes!), down: fmtNum(s.downVotes!) }),
    ],
    num("game.stat.maxPlayers", s.maxPlayers),
    ["game.stat.servers", s.servers === null ? null : t("game.about", { n: fmtNum(s.servers) }), t("game.serversHint")],
    ["game.stat.genre", s.genre, s.subgenre ?? undefined],
    ["game.stat.age", age(s), s.ageRating?.descriptors.join(", ") || undefined],
    ["game.stat.avatar", s.avatarType && avatar(s.avatarType)],
    ["game.stat.copying", s.copying === null ? null : t(s.copying ? "common.on" : "common.off")],
    num("game.stat.places", s.places),
    ...(s.groupMembers === null ? [] : [num("game.stat.groupMembers", s.groupMembers)]),
    ["game.stat.created", day(s.created)],
    ["game.stat.updated", day(s.updated)],
  ];
  return (
    <>
      <StatGrid stats={stats} />
      <div className="flex flex-col gap-3">
        <Tabs
          label={t("game.detailsTip")}
          value={tab}
          onChange={setTab}
          panelId={panelId}
          items={[
            { value: "passes", label: t("game.passes"), icon: "star", count: s.passes?.length },
            { value: "badges", label: t("game.badges"), icon: "shield", count: s.badges?.length },
          ]}
        />
        <div id={panelId} role="tabpanel">
          {tab === "passes" ? <Passes list={s.passes} more={s.morePasses} /> : <Badges list={s.badges} more={s.moreBadges} />}
        </div>
      </div>
    </>
  );
}

export function Listed<T extends { id: number; name: string; iconUrl: string | null }>({ list, more, empty, side }: {
  list: T[] | null;
  more: boolean;
  empty: string;
  side: (item: T) => ReactNode;
}) {
  if (list === null) return <p className="text-fg-2">{t("accounts.detail.noAnswer")}</p>;
  if (!list.length) return <p className="text-fg-2">{empty}</p>;
  return (
    <>
      <ul className="flex flex-col">
        {list.map((i) => (
          <li key={i.id} className="flex items-center gap-3 py-2 border-b border-line last:border-b-0 min-w-0">
            {i.iconUrl ? <img src={i.iconUrl} alt="" className="w-9 h-9 rounded-[var(--r-sm)] flex-none" loading="lazy" /> : <span aria-hidden="true" className="w-9 h-9 flex-none" />}
            <span className="flex-1 min-w-0 truncate">{i.name}</span>
            {side(i)}
          </li>
        ))}
      </ul>
      {more && <p className="meta pt-2">{t("game.listCut")}</p>}
    </>
  );
}

function Passes({ list, more }: { list: GamePass[] | null; more: boolean }) {
  // For sale first, cheapest first; then the ones off sale.
  const sorted = list && [...list].sort((a, b) => (a.price ?? Infinity) - (b.price ?? Infinity));
  return (
    <Listed
      list={sorted}
      more={more}
      empty={t("game.noPasses")}
      side={(p) => <span className={`mono flex-none ${p.price === null ? "text-fg-3" : ""}`}>{p.price === null ? t("game.offSale") : t("game.price", { price: fmtNum(p.price) })}</span>}
    />
  );
}

function Badges({ list, more }: { list: GameBadge[] | null; more: boolean }) {
  const compact = (n: number) => fmtNum(n, { notation: "compact", maximumFractionDigits: 1 });
  return (
    <Listed
      list={list}
      more={more}
      empty={t("game.noBadges")}
      side={(b) => (
        <span className="flex-none flex flex-col items-end text-[13px]">
          <span className="mono">{t("game.rarity", { pct: fmtNum(b.winRate, { style: "percent", maximumSignificantDigits: 2 }) })}</span>
          <span className="text-fg-3" title={t("game.won", { day: fmtNum(b.awardedYesterday), ever: fmtNum(b.awarded) })}>
            {t("game.won", { day: compact(b.awardedYesterday), ever: compact(b.awarded) })}
          </span>
        </span>
      )}
    />
  );
}
