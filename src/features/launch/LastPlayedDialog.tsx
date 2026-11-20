import { useQuery } from "@tanstack/react-query";
import { api, asError, type AccountSummary, type LaunchTarget, type PlayedServer } from "../../api";
import { useGame } from "../../data";
import { t } from "../../i18n";
import { Skeleton } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { friendly } from "../../ui/Toast";
import { accountName, ago } from "../accounts/shared";
import "./launch.css";

/** The servers the accounts' clients were last in (what RoALT followed on this PC), newest first:
 * one of them, or just its game, becomes Play's target. Asked each time it opens. */
export function LastPlayedDialog({ open, onClose, accounts, onPick }: {
  open: boolean;
  onClose: () => void;
  accounts: AccountSummary[];
  onPick: (target: LaunchTarget) => void;
}) {
  const list = useQuery({ queryKey: ["serverHistory", "recent"], queryFn: api.serverRecent, enabled: open, staleTime: 0 });
  const ready = !!list.data && !list.isFetching;
  return (
    <Dialog open={open} onClose={onClose} title={t("launch.last.title")} width={520}>
      <div className="scroll max-h-[min(420px,60vh)] px-[22px] pb-2">
        {list.isError ? (
          <div className="flex flex-col items-start gap-3 py-4">
            <p role="alert" className="alert">{friendly(asError(list.error))}</p>
            <Button size="md" icon="refresh" busy={list.isFetching} disabled={list.isFetching} onClick={() => void list.refetch()}>
              {t("common.tryAgain")}
            </Button>
          </div>
        ) : !ready ? (
          <div aria-busy="true" aria-label={t("common.loading")}>
            {[160, 120, 140].map((w) => (
              <div key={w} className="flex items-center gap-3 min-h-[52px]">
                <Skeleton width={28} height={28} radius={6} />
                <div className="flex flex-col gap-1.5">
                  <Skeleton width={w} height={12} />
                  <Skeleton width={w * 0.6} height={10} />
                </div>
              </div>
            ))}
          </div>
        ) : list.data.length === 0 ? (
          <p className="py-4 text-fg-2">{t("launch.last.none")}</p>
        ) : (
          <ul className="flex flex-col">
            {list.data.map((s) => (
              <Row key={`${s.placeId}-${s.jobId}`} s={s} accounts={accounts} onPick={onPick} />
            ))}
          </ul>
        )}
      </div>
      <p className="meta px-[22px] py-3 border-t border-line text-pretty">{t("launch.last.footer")}</p>
    </Dialog>
  );
}

function Row({ s, accounts, onPick }: { s: PlayedServer; accounts: AccountSummary[]; onPick: (target: LaunchTarget) => void }) {
  const g = useGame(s.placeId).data;
  const who = accounts.filter((a) => s.accountIds.includes(a.id)).map(accountName);
  return (
    <li className="flex flex-wrap items-center gap-x-3 gap-y-1.5 min-h-[52px] py-1.5">
      {g?.iconUrl ? <img src={g.iconUrl} alt="" width={28} height={28} className="fav-icon" loading="lazy" /> : <span aria-hidden="true" className="grid place-content-center w-7 h-7 flex-none text-fg-3"><Icon name="clock" size={18} /></span>}
      <span className="flex flex-col min-w-0 flex-[1_1_160px] leading-tight">
        <span className="font-semibold truncate">{g?.name ?? t("dashboard.place", { id: s.placeId })}</span>
        <span className="text-xs text-fg-3 truncate">{[ago(s.joinedAt), ...who.slice(0, 2), who.length > 2 ? `+${who.length - 2}` : ""].filter(Boolean).join(" · ")}</span>
      </span>
      <span className="flex flex-none items-center gap-1.5">
        {s.private ? (
          <span className="text-[12px] text-fg-3">{t("accounts.history.private")}</span>
        ) : (
          <Button size="sm" onClick={() => onPick({ kind: "server", placeId: s.placeId, jobId: s.jobId })}>{t("launch.last.thisServer")}</Button>
        )}
        <Button size="sm" variant="ghost" onClick={() => onPick({ kind: "place", placeId: s.placeId, launchData: null })}>{t("launch.last.thisGame")}</Button>
      </span>
    </li>
  );
}
