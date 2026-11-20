import "./crashes.css";
import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { api, type AccountSummary, type Crash, type CrashKind } from "../../api";
import { qk, useAccounts, useCrashes, useGame } from "../../data";
import { fmtDate, t } from "../../i18n";
import { Skeleton } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { Select } from "../../ui/Field";
import { toastError } from "../../ui/Toast";
import { accountName, ago, Avatar } from "../accounts/shared";
import { Problem } from "../browse/Native";
import { uptime } from "../launch/Running";
import { exportCsv } from "./exportCsv";
import { CRASH_KINDS, verdictOf } from "./kinds";

const PAGE = 50;



/** One recorded end: how, when, in which game, what its log said, what it points to, its log.
 * `account`: whose it was, where the list holds several accounts' ends. */
export function CrashCard({ crash: c, account }: { crash: Crash; account?: AccountSummary }) {
  const game = useGame(c.placeId).data;
  const verdict = verdictOf(c);
  return (
    <li className="panel crashes-card">
      <div className="crashes-card-head">
        {account && (
          <span className="crashes-who">
            <Avatar account={account} size={20} />
            <span className="truncate">{accountName(account)}</span>
          </span>
        )}
        <h2 className="font-medium truncate flex-1 min-w-0">{t(CRASH_KINDS[c.kind])}</h2>
        <span className="meta" title={fmtDate(c.at, { day: "numeric", month: "short", year: "numeric", hour: "numeric", minute: "2-digit" })}>{ago(c.at)}</span>
      </div>
      <p className="text-[13px] text-fg-2">
        {c.placeId !== null && <>{game?.name ?? t("dashboard.place", { id: c.placeId })} · </>}
        {t("launch.crash.after", { time: uptime(0, c.uptimeMs) })}
      </p>
      {c.reason && <p className="mono text-xs text-fg-2 break-words selectable">{c.reason}</p>}
      {verdict && <p className="text-[13px]">{verdict}</p>}
      {c.hasLog && (
        <Button size="sm" iconEnd="external" className="self-start" data-tip={t("crashes.rawLogHint")} onClick={() => api.crashOpenLog(c.id).catch(toastError)}>
          {t("launch.crash.openLog")}
        </Button>
      )}
    </li>
  );
}

/** Crash history: how the clients RoALT followed ended (the newest 200), with what each end points
 * to. Read from this PC; nothing is asked of Roblox. */
export function CrashesView() {
  const qc = useQueryClient();
  const accounts = useAccounts().data ?? [];
  const crashes = useCrashes();
  const [only, setOnly] = useState("");
  const [kind, setKind] = useState<CrashKind | "">("");
  const [shown, setShown] = useState(PAGE);

  const byId = new Map(accounts.map((a) => [a.id, a]));
  const all = crashes.data ?? [];
  const had = accounts.filter((a) => all.some((c) => c.accountId === a.id));
  const picked = had.some((a) => String(a.id) === only) ? only : "";
  const rows = all.filter((c) => (!picked || String(c.accountId) === picked) && (!kind || c.kind === kind));
  const pick = <T,>(set: (v: T) => void) => (v: T) => {
    set(v);
    setShown(PAGE);
  };

  return (
    <main className="scroll flex-1 min-w-0 page">
      <div className="crashes-page">
        <header className="crashes-head">
          <div className="crashes-bar">
            <h1 className="page-title">{t("app.nav.crashes")}</h1>
            <span className="flex flex-wrap gap-2">
              {all.length > 0 && (
                <Button icon="download" data-tip={t("crashes.rawCsvHint")} onClick={() => void exportCsv("RoALT-crashes", api.crashesExport)}>
                  {t("common.exportCsv")}
                </Button>
              )}
              <Button icon="refresh" busy={crashes.isFetching} onClick={() => void qc.invalidateQueries({ queryKey: qk.crashes })}>
                {t("ui.refresh")}
              </Button>
            </span>
          </div>
          <p className="page-sub">{t("crashes.lead")}</p>
          {all.length > 0 && (
            <div className="crashes-bar">
              <Select
                aria-label={t("crashes.kind")}
                className="crashes-select"
                value={kind}
                onChange={pick<string>((v) => setKind(v as CrashKind | ""))}
                options={[{ value: "", label: t("crashes.allKinds") }, ...(Object.keys(CRASH_KINDS) as CrashKind[]).map((k) => ({ value: k, label: t(CRASH_KINDS[k]) }))]}
              />
              {had.length > 1 && (
                <Select
                  aria-label={t("monitor.account")}
                  className="crashes-select"
                  value={picked}
                  onChange={pick(setOnly)}
                  options={[{ value: "", label: t("common.allAccounts") }, ...had.map((a) => ({ value: String(a.id), label: accountName(a) }))]}
                />
              )}
            </div>
          )}
        </header>

        {crashes.error ? (
          <Problem error={crashes.error} onRetry={() => void crashes.refetch()} />
        ) : !crashes.data ? (
          <Skeleton width="100%" height={180} />
        ) : rows.length === 0 ? (
          <div className="empty">
            <p className="font-semibold text-fg">{t("crashes.emptyTitle")}</p>
            <p>{t(all.length ? "crashes.emptyFiltered" : "crashes.emptyText")}</p>
          </div>
        ) : (
          <ul className="flex flex-col gap-3">
            {rows.slice(0, shown).map((c) => (
              <CrashCard key={c.id} crash={c} account={byId.get(c.accountId)} />
            ))}
          </ul>
        )}
        {rows.length > shown && (
          <Button size="md" variant="ghost" className="self-start" onClick={() => setShown((n) => n + PAGE)}>
            {t("market.more", { shown, total: rows.length })}
          </Button>
        )}
      </div>
    </main>
  );
}
