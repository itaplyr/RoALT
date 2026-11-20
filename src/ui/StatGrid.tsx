import { fmtNum, t, type Key } from "../i18n";

/** Label, value (null = no answer), tooltip. */
export type Stat = [Key, string | null, string?];

/** Big counts shrink to "1.2M" with the exact number as the tooltip. */
export function num(k: Key, n: number | null): Stat {
  if (n === null) return [k, null];
  if (n < 100_000) return [k, fmtNum(n)];
  return [k, fmtNum(n, { notation: "compact", maximumFractionDigits: 1 }), fmtNum(n)];
}

/** Numbers three to a row, each under its label; one Roblox didn't answer shows a dash. */
export function StatGrid({ stats }: { stats: Stat[] }) {
  return (
    <dl className="grid grid-cols-3 gap-x-3 gap-y-3">
      {stats.map(([k, v, title]) => (
        <div key={k} className="min-w-0">
          <dt className="text-[12px] text-fg-3 truncate">{t(k)}</dt>
          <dd className="mono text-[14px] text-fg font-medium truncate" title={v === null ? t("accounts.detail.noAnswer") : title}>
            {v ?? "–"}
          </dd>
        </div>
      ))}
    </dl>
  );
}
