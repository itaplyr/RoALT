import { useId } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "../../api";
import { useGame } from "../../data";
import { t } from "../../i18n";
import { Icon } from "../../ui/Icon";
import "./launch.css";

/** How many games it offers. */
const SHOWN = 6;

/** One recent game: its icon and name once RoALT knows them, its place id until then. */
function Game({ placeId, onPick }: { placeId: number; onPick: (placeId: number) => void }) {
  const g = useGame(placeId).data;
  return (
    <li>
      <button type="button" className="chip" onClick={() => onPick(placeId)}>
        {g?.iconUrl ? <img src={g.iconUrl} alt="" width={18} height={18} className="fav-icon" loading="lazy" /> : <Icon name="clock" size={13} />}
        <span className="truncate max-w-[220px]">{g?.name ?? t("dashboard.place", { id: placeId })}</span>
      </button>
    </li>
  );
}

/** The games the accounts played last (from the playtime RoALT counts on this PC), newest first: a
 * click picks one. Nothing shows until a game was played. `skip`: a place already on show. `onMore`:
 * a way to the servers they were in ("Last played…"), there even when `skip` leaves no game. */
export function RecentGames({ onPick, skip = null, onMore }: { onPick: (placeId: number) => void; skip?: number | null; onMore?: () => void }) {
  const rows = useQuery({ queryKey: ["playtime"], queryFn: api.playtimeList, staleTime: 60_000 }).data ?? [];
  const last = new Map<number, number>();
  for (const r of rows) last.set(r.placeId, Math.max(last.get(r.placeId) ?? 0, r.lastPlayed));
  const places = [...last].filter(([p]) => p !== skip).sort((a, b) => b[1] - a[1]).slice(0, SHOWN).map(([p]) => p);
  // Its own id: Play shows this, and so can the server browser it opens over itself.
  const labelId = useId();
  if (!places.length && !(onMore && last.size)) return null;
  return (
    <div className="flex flex-col gap-2">
      <span id={labelId} className="label text-fg-2">{t("launch.recent")}</span>
      <ul aria-labelledby={labelId} className="launch-favs">
        {places.map((p) => (
          <Game key={p} placeId={p} onPick={onPick} />
        ))}
        {onMore && (
          <li>
            <button type="button" className="chip" aria-haspopup="dialog" onClick={onMore}>
              <Icon name="clock" size={13} />
              {t("launch.last.open")}
            </button>
          </li>
        )}
      </ul>
    </div>
  );
}
