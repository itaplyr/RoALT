// Browse › Limiteds: every limited with its RAP, value, demand and trend, from Rolimons' public
// item list (only with Rolimons values on). Pictures come from Roblox, a page at a time.
import { useEffect, useState } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
import { api, type RolimonsItem } from "../../api";
import { qk } from "../../data";
import { fmtNum, t } from "../../i18n";
import { Skeleton } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { Input, Select } from "../../ui/Field";
import { jumpTo } from "../settings/jumpTo";
import { ago } from "../accounts/shared";
import { DEMAND, TREND } from "../accounts/Limiteds";
import { GameIcon, Problem } from "./Native";

const PAGE = 50;
type Sort = "value" | "rap" | "demand" | "name";
type Row = [number, RolimonsItem];

const worth = (r: RolimonsItem) => r.value ?? r.rap ?? 0;
const ORDER: Record<Sort, (a: Row, b: Row) => number> = {
  value: ([, a], [, b]) => worth(b) - worth(a),
  rap: ([, a], [, b]) => (b.rap ?? 0) - (a.rap ?? 0),
  demand: ([, a], [, b]) => (b.demand ?? -1) - (a.demand ?? -1) || worth(b) - worth(a),
  name: ([, a], [, b]) => a.name.localeCompare(b.name),
};

/** `onOpen`: the item's page on roblox.com, here in Browse. */
export function LimitedsTab({ onOpen }: { onOpen: (path: string) => void }) {
  const q = useQuery({ queryKey: qk.market, queryFn: api.marketItems, staleTime: 5 * 60_000 });
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>("value");
  const [shown, setShown] = useState(PAGE);
  // The search applies once typing pauses: each result asks Roblox for its pictures.
  const [find, setFind] = useState("");
  useEffect(() => {
    const timer = setTimeout(() => setFind(query.trim().toLowerCase()), 250);
    return () => clearTimeout(timer);
  }, [query]);

  const all = q.data?.items ?? [];
  const list = all.filter(([, r]) => !find || r.name.toLowerCase().includes(find) || r.acronym?.toLowerCase() === find).sort(ORDER[sort]);
  const page = list.slice(0, shown);
  // Pictures by page: asking again for the pages already shown costs nothing (cached).
  const chunks = Array.from({ length: Math.ceil(page.length / PAGE) }, (_, i) => page.slice(i * PAGE, (i + 1) * PAGE).map(([id]) => id));
  const thumbs = useQueries({
    queries: chunks.map((ids) => ({ queryKey: ["assetThumbs", ids], queryFn: () => api.assetThumbnails(ids), staleTime: Infinity, retry: false })),
  });
  const pic = new Map(thumbs.flatMap((x) => x.data ?? []));

  if (q.error) return <Problem error={q.error} onRetry={() => q.refetch()} />;
  if (!q.data && q.isPending) return <Skeleton width="100%" height={180} />;
  if (!q.data)
    return (
      <div className="flex flex-col items-start gap-3 max-w-prose">
        <p className="text-fg-2">{t("market.off")}</p>
        <Button size="md" icon="settings" onClick={() => jumpTo("rolimons")}>{t("market.turnOn")}</Button>
      </div>
    );
  if (q.data.fetchedAt === null)
    return (
      <div className="flex flex-col items-start gap-3">
        <p role="alert" className="alert">{t("market.unreachable")}</p>
        <Button size="md" icon="refresh" busy={q.isFetching} onClick={() => q.refetch()}>{t("common.tryAgain")}</Button>
      </div>
    );

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <Input
          leading="search"
          className="w-64"
          aria-label={t("market.search")}
          placeholder={t("market.search")}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setShown(PAGE);
          }}
        />
        <Select
          aria-label={t("market.sortBy")}
          value={sort}
          onChange={(v) => {
            setSort(v);
            setShown(PAGE);
          }}
          options={[
            { value: "value", label: t("value.value") },
            { value: "rap", label: t("value.rap") },
            { value: "demand", label: t("market.demand") },
            { value: "name", label: t("market.name") },
          ]}
        />
        <span className="meta ms-auto">{t("market.source", { when: ago(q.data.fetchedAt) })}</span>
      </div>
      {list.length === 0 ? (
        <p className="text-fg-2">{t("market.none")}</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {page.map(([id, r]) => (
            <li key={id} className="panel browse-row items-center">
              <GameIcon url={pic.get(id)} />
              <div className="flex-1 min-w-0 flex flex-col gap-1">
                <h3 className="font-medium truncate">
                  {r.name}
                  {r.acronym && <span className="text-fg-3"> ({r.acronym})</span>}
                </h3>
                <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[13px] text-fg-2">
                  <span>{t("value.rap")} <span className="mono">{r.rap === null ? "—" : fmtNum(r.rap)}</span></span>
                  <span>{t("value.value")} <span className="mono">{r.value === null ? "—" : fmtNum(r.value)}</span></span>
                  {r.demand !== null && <span>{t("value.demandIs", { level: t(DEMAND[r.demand]) })}</span>}
                  {r.trend !== null && <span>{t("value.trendIs", { level: t(TREND[r.trend]) })}</span>}
                  {r.projected && (
                    <span className="tag" style={{ color: "var(--attention)" }} title={t("value.projectedHint")}>
                      {t("value.projected")}
                    </span>
                  )}
                  {r.hyped && <span className="tag">{t("value.hyped")}</span>}
                  {r.rare && <span className="tag">{t("value.rare")}</span>}
                </div>
              </div>
              <Button size="sm" variant="ghost" icon="globe" className="flex-none" onClick={() => onOpen(`catalog/${id}`)}>{t("browse.open")}</Button>
            </li>
          ))}
        </ul>
      )}
      {list.length > shown && (
        <Button size="md" variant="ghost" className="self-start" onClick={() => setShown((n) => n + PAGE)}>
          {t("market.more", { shown: page.length, total: list.length })}
        </Button>
      )}
    </div>
  );
}
