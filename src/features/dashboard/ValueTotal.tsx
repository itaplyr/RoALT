// Home's total of every account's limiteds, from their last counts; Count all counts them again, one
// account after another, only when pressed.
import { useQueries } from "@tanstack/react-query";
import type { AccountSummary, AccountValue } from "../../api";
import { accountValueQuery, useSettings } from "../../data";
import { t } from "../../i18n";
import { Button } from "../../ui/Button";
import { num, StatGrid, type Stat } from "../../ui/StatGrid";
import { accountName, ago } from "../accounts/shared";
import { countAll, stopCount, useValueCount } from "../accounts/valueCount";

export function ValueTotal({ accounts }: { accounts: AccountSummary[] }) {
  const values = useQueries({ queries: accounts.map((a) => accountValueQuery(a.id)) });
  const run = useValueCount();
  const rolimons = useSettings().data?.rolimonsValues ?? false;
  const counted = values.map((q) => q.data).filter((v): v is AccountValue => !!v);
  const valid = accounts.filter((a) => a.status === "valid");
  if (!valid.length && !counted.length) return null;

  const sum = (f: (v: AccountValue) => number) => counted.reduce((n, v) => n + f(v), 0);
  const stats: Stat[] = [
    num("value.rap", counted.length ? sum((v) => v.rap) : null),
    num("value.items", counted.length ? sum((v) => v.count) : null),
  ];
  // Only a total of every count: one without Rolimons' values would make it look smaller.
  if (rolimons) stats.push(num("value.value", counted.length && counted.every((v) => v.value !== null) ? sum((v) => v.value!) : null));
  const oldest = counted.length ? Math.min(...counted.map((v) => v.fetchedAt)) : null;
  const current = run.current === null ? undefined : accounts.find((a) => a.id === run.current);

  return (
    <section aria-labelledby="value-title" className="panel flex flex-col gap-3 px-4 py-3">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2 min-h-10">
        <h2 id="value-title" className="section-title">{t("value.homeTitle")}</h2>
        <div className="ms-auto flex flex-wrap gap-1.5">
          {run.running ? (
            <Button size="md" icon="x" onClick={stopCount}>{t("common.cancel")}</Button>
          ) : (
            valid.length > 0 && (
              <Button size="md" icon="refresh" onClick={() => void countAll(accounts)}>{t("value.countAll")}</Button>
            )
          )}
        </div>
      </div>
      {counted.length > 0 && <StatGrid stats={stats} />}
      <div aria-live="polite" className="flex flex-col gap-1">
        {run.running && current && <p className="text-[13px] text-fg-2">{t("value.counting", { name: accountName(current), done: run.done + 1, total: run.total })}</p>}
        {!run.running && run.failed > 0 && <p className="alert text-[13px]">{t("value.failed", { count: run.failed })}</p>}
        {!run.running && run.stopped && <p className="alert text-[13px]">{run.stopped}</p>}
      </div>
      <p className="meta">
        {counted.length
          ? `${t("value.accounts", { n: counted.length, m: accounts.length })} · ${t("value.oldest", { when: ago(oldest) })}${counted.some((v) => v.partial) ? ` · ${t("value.partial")}` : ""}`
          : t("value.homeHint")}
      </p>
    </section>
  );
}
