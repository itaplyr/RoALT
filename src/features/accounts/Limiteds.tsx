// An account's limiteds: what they're worth by Roblox's RAP (and Rolimons' values when that's on),
// counted only when asked: counting reads the whole inventory, slowly.
import { useIsMutating, useMutation } from "@tanstack/react-query";
import type { AccountSummary, AccountValue, LimitedItem } from "../../api";
import { useAccountValue, useSettings } from "../../data";
import { fmtNum, t, type Key } from "../../i18n";
import { Disclosure } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { num, StatGrid, type Stat } from "../../ui/StatGrid";
import { toast } from "../../ui/Toast";
import { ago } from "./shared";
import { countOne, rereadOthers, useValueCount } from "./valueCount";

const DEMAND: Key[] = ["value.demand0", "value.demand1", "value.demand2", "value.demand3", "value.demand4"];
const TREND: Key[] = ["value.trend0", "value.trend1", "value.trend2", "value.trend3", "value.trend4"];

export function Limiteds({ account: a }: { account: AccountSummary }) {
  const value = useAccountValue(a.id);
  const all = useValueCount();
  const rolimons = useSettings().data?.rolimonsValues ?? false;
  // Keyed, so a count still running shows as busy after the panel was left and opened again.
  const key = ["valueCount", a.id];
  const count = useMutation({
    mutationKey: key,
    mutationFn: () => countOne(a.id),
    onSuccess: (fresh) => {
      if (!fresh) toast(t("value.notUpdated"), "error");
      rereadOthers(new Set([a.id]));
    },
  });
  const v = value.data;
  const busy = useIsMutating({ mutationKey: key }) > 0 || all.current === a.id;
  // One count at a time: Home's Count all reads inventories too.
  const blocked = all.running && !busy;
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <h4 className="label text-fg-2">{t("value.title")}</h4>
        {a.status === "valid" && (
          <Button size="sm" variant="ghost" icon="refresh" busy={busy} aria-disabled={blocked || undefined} onClick={() => !busy && !blocked && count.mutate()}>
            {v ? t("value.countAgain") : t("value.count")}
          </Button>
        )}
      </div>
      {v ? (
        <>
          <ValueFacts v={v} rolimons={rolimons} />
          {v.top.length > 0 && (
            <Disclosure summary={t("value.top")}>
              <TopItems items={v.top} />
            </Disclosure>
          )}
        </>
      ) : (
        <p className="meta">{value.isPending ? t("common.loading") : `${t("value.never")} ${t("value.slow")}`}</p>
      )}
    </div>
  );
}

/** A count's totals, when it was made and what it lacks (an account's, or a player's in Browse). */
export function ValueFacts({ v, rolimons }: { v: AccountValue; rolimons: boolean }) {
  const stats: Stat[] = [num("value.rap", v.rap), num("value.items", v.count), ...(rolimons ? [num("value.value", v.value)] : [])];
  return (
    <>
      <StatGrid stats={stats} />
      <p className="meta">
        {t("value.counted", { when: ago(v.fetchedAt) })}
        {v.partial && ` · ${t("value.partial")}`}
      </p>
      {rolimons && v.value === null && <p className="meta">{t("value.noRolimons")}</p>}
    </>
  );
}

export function TopItems({ items }: { items: LimitedItem[] }) {
  return (
    <ul className="flex flex-col pt-1">
      {items.map((i) => {
        const r = i.rolimons;
        return (
          <li key={i.assetId} className="flex flex-col gap-1 py-2 border-b border-line last:border-b-0 min-w-0">
            <div className="flex items-baseline gap-2 min-w-0">
              <span className="flex-1 min-w-0 truncate">
                {i.name}
                {r?.acronym && <span className="text-fg-3"> ({r.acronym})</span>}
              </span>
              {i.copies > 1 && <span className="mono text-fg-3 flex-none">×{i.copies}</span>}
              <span className="mono flex-none">{fmtNum(r?.value ?? i.rap)}</span>
            </div>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[12px] text-fg-3">
              <span>{t("value.rapEach", { n: fmtNum(i.rap) })}</span>
              {r?.demand != null && <span>{t("value.demandIs", { level: t(DEMAND[r.demand]) })}</span>}
              {r?.trend != null && <span>{t("value.trendIs", { level: t(TREND[r.trend]) })}</span>}
              {r?.projected && (
                <span className="tag" style={{ color: "var(--attention)" }} title={t("value.projectedHint")}>
                  {t("value.projected")}
                </span>
              )}
              {r?.hyped && <span className="tag">{t("value.hyped")}</span>}
              {r?.rare && <span className="tag">{t("value.rare")}</span>}
            </div>
          </li>
        );
      })}
    </ul>
  );
}
