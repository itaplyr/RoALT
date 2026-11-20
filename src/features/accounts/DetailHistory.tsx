// An account's history: its Robux over the last month with the newest transactions, and how its
// clients ended.
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, type AccountSummary } from "../../api";
import { browseAs, requestNavigation } from "../../app/navigation";
import type { RobuxKind } from "../../bindings/RobuxKind";
import { qk, useCrashes } from "../../data";
import { fmtNum, t, type Key } from "../../i18n";
import { Skeleton } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { Select } from "../../ui/Field";
import { num, StatGrid } from "../../ui/StatGrid";
import { Problem } from "../browse/Native";
import { CrashCard } from "../crashes/CrashesView";
import { ago } from "./shared";

const KINDS: Record<RobuxKind, Key> = {
  purchase: "history.kind.purchase",
  sale: "history.kind.sale",
  groupPayout: "history.kind.groupPayout",
  premiumStipend: "history.kind.premiumStipend",
};
/** Its ends shown here; the rest are in Crash history. */
const CRASHES = 5;

export function DetailHistory({ account: a }: { account: AccountSummary }) {
  return (
    <div className="flex flex-col gap-7 p-5">
      <Robux account={a} />
      <Ends account={a} />
    </div>
  );
}

/** Asked of Roblox when this tab opens and on Refresh, never on a timer. */
function Robux({ account: a }: { account: AccountSummary }) {
  const [kind, setKind] = useState<RobuxKind>("purchase");
  const valid = a.status === "valid";
  const totals = useQuery({ queryKey: qk.robuxTotals(a.id), queryFn: () => api.robuxTotals(a.id), enabled: valid, staleTime: 5 * 60_000 });
  const rows = useQuery({ queryKey: qk.robux(a.id, kind), queryFn: () => api.robuxTransactions(a.id, kind), enabled: valid, staleTime: 5 * 60_000 });
  const m = totals.data;
  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <h4 className="label text-fg-2">{t("history.robux")}</h4>
        {valid && (
          <Button size="sm" variant="ghost" icon="refresh" busy={totals.isFetching || rows.isFetching} onClick={() => void Promise.all([totals.refetch(), rows.refetch()])}>
            {t("ui.refresh")}
          </Button>
        )}
      </div>
      {!valid ? (
        <p className="meta">{t("history.needsSignIn")}</p>
      ) : (
        <>
          {totals.error ? (
            <Problem error={totals.error} onRetry={() => void totals.refetch()} />
          ) : (
            // Roblox sends what went out as a negative number.
            <StatGrid stats={[num("history.in", m?.incoming ?? null), num("history.out", m?.outgoing == null ? null : Math.abs(m.outgoing)), num("history.pending", m?.pending ?? null)]} />
          )}
          <div className="flex flex-wrap items-center justify-between gap-2">
            <Select
              aria-label={t("history.kindLabel")}
              value={kind}
              onChange={(v) => setKind(v as RobuxKind)}
              options={(Object.keys(KINDS) as RobuxKind[]).map((k) => ({ value: k, label: t(KINDS[k]) }))}
            />
            <Button size="sm" variant="ghost" icon="globe" onClick={() => browseAs(a.id, "transactions")}>
              {t("servers.openOnRoblox")}
            </Button>
          </div>
          {rows.error ? (
            <Problem error={rows.error} onRetry={() => void rows.refetch()} />
          ) : !rows.data ? (
            <Skeleton width="100%" height={96} />
          ) : rows.data.length === 0 ? (
            <p className="meta">{t("history.none")}</p>
          ) : (
            <ul className="flex flex-col">
              {rows.data.map((r, i) => (
                // By place: Roblox gives the lines no id RoALT reads.
                <li key={i} className="flex items-baseline gap-2 py-2 border-b border-line last:border-b-0 min-w-0">
                  <span className="flex-1 min-w-0 flex flex-col">
                    <span className="truncate">{r.what || r.who || t(KINDS[kind])}</span>
                    <span className="text-[12px] text-fg-3 truncate">
                      {[r.what && r.who, r.created && ago(Date.parse(r.created)), r.pending && t("history.pending")].filter(Boolean).join(" · ")}
                    </span>
                  </span>
                  <span className="mono flex-none" style={r.amount > 0 ? { color: "var(--ok)" } : undefined}>
                    {r.amount > 0 ? "+" : ""}
                    {fmtNum(r.amount)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </section>
  );
}

function Ends({ account: a }: { account: AccountSummary }) {
  const crashes = (useCrashes().data ?? []).filter((c) => c.accountId === a.id);
  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <h4 className="label text-fg-2">{t("history.ends")}</h4>
        {crashes.length > CRASHES && (
          <Button size="sm" variant="ghost" onClick={() => requestNavigation("crashes")}>
            {t("history.allEnds", { n: crashes.length })}
          </Button>
        )}
      </div>
      {crashes.length === 0 ? (
        <p className="meta">{t("history.noEnds")}</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {crashes.slice(0, CRASHES).map((c) => (
            <CrashCard key={c.id} crash={c} />
          ))}
        </ul>
      )}
    </section>
  );
}
