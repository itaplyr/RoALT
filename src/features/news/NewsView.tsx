import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { fmtDate, fmtList, t } from "../../i18n";
import { Chip, Skeleton } from "../../ui/Bits";
import { Button, IconButton } from "../../ui/Button";
import { toastError } from "../../ui/Toast";
import { ago } from "../accounts/shared";
import { useUpdateInstall, useUpdateOffer } from "../settings/UpdateBanner";
import { markRead, useNewsFeed, type FeedItem } from "./news";
import { releaseNotes } from "./notes";
import "./news.css";

type Source = FeedItem["source"];
const SOURCES: Source[] = ["roalt", "devforum", "newsroom", "client"];
const LABEL = { roalt: "news.src.roalt", devforum: "news.src.devforum", newsroom: "news.src.newsroom", client: "news.src.client" } as const;
/** Lines a release shows before "All notes". */
const SHORT = 6;
const DAY = 24 * 60 * 60_000;
/** Leaving News marks what it showed as read (`NewsView`). */
let leaving = 0;

/** "3 h ago" within a week, else the date; a month alone (the newsroom) as month and year. */
function when(published: string, now: number) {
  if (!published) return "";
  const ms = Date.parse(published);
  if (Number.isNaN(ms)) return "";
  if (/^\d{4}-\d{2}$/.test(published)) return fmtDate(ms, { month: "long", year: "numeric", timeZone: "UTC" });
  return now - ms < 7 * DAY ? ago(ms, now) : fmtDate(ms, { day: "numeric", month: "short", year: "numeric" });
}

function open(item: FeedItem) {
  markRead([item.id]);
  if (item.url) openUrl(item.url).catch(toastError);
}

/** The newest RoALT release: its notes, and the update when this is the version on offer. */
function Lead({ item, now }: { item: FeedItem; now: number }) {
  const offer = useUpdateOffer();
  const update = useUpdateInstall(offer);
  const [all, setAll] = useState(false);
  const sections = releaseNotes(item.summary);
  const total = sections.reduce((n, s) => n + s.lines.length, 0);
  let left = all ? Infinity : SHORT;
  const shown = sections
    .map((s) => {
      const lines = s.lines.slice(0, Math.max(0, left));
      left -= lines.length;
      return { ...s, lines };
    })
    .filter((s) => s.lines.length);
  return (
    <article className="panel news-lead" aria-labelledby="news-lead-title">
      <p className="news-meta">
        {item.unread && <span className="news-dot" role="img" aria-label={t("news.unread")} />}
        <b>{t(LABEL.roalt)}</b>
        <span>{when(item.published, now)}</span>
      </p>
      <h2 id="news-lead-title">{item.title}</h2>
      {shown.length > 0 && (
        <div className="news-notes">
          {shown.map((s, i) => (
            <section key={i}>
              {s.title && <h3>{s.title}</h3>}
              <ul>
                {s.lines.map((l, j) => (
                  <li key={j}>{l}</li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-2">
        {offer && offer === item.version && (
          <Button variant="primary" icon="download" busy={update.busy} onClick={update.start}>
            {t("settings.updateNow")}
          </Button>
        )}
        {total > SHORT && (
          <Button size="md" aria-expanded={all} onClick={() => setAll((a) => !a)}>
            {all ? t("news.fewerNotes") : t("news.allNotes")}
          </Button>
        )}
        <Button size="md" variant="ghost" iconEnd="external" onClick={() => open(item)}>
          {t("news.releasePage")}
        </Button>
      </div>
      {update.dialog}
    </article>
  );
}

function Row({ item, now }: { item: FeedItem; now: number }) {
  const body = (
    <>
      <span className="news-dot" data-on={item.unread || undefined} role={item.unread ? "img" : undefined} aria-label={item.unread ? t("news.unread") : undefined} />
      <span className="min-w-0 flex flex-col gap-1">
        <span className="news-meta">
          <b>{t(LABEL[item.source])}</b>
          <span>{when(item.published, now)}</span>
        </span>
        <span className="news-title">{item.title}</span>
        {item.summary && <span className="news-sum">{item.summary}</span>}
      </span>
    </>
  );
  // The client update has nowhere to go: RoALT already uses it.
  if (!item.url) return <div className="news-row" data-unread={item.unread || undefined}>{body}</div>;
  return (
    <button type="button" className="news-row" data-unread={item.unread || undefined} data-tip={t("news.open")} onClick={() => open(item)}>
      {body}
    </button>
  );
}

/** Home's News tab: RoALT's newest release up front, then everything, newest first, by source.
 * Leaving it marks what it showed as read. */
export function NewsView() {
  const { items, unread, failed, query } = useNewsFeed();
  const [filter, setFilter] = useState<Source | "all">("all");
  const now = Date.now();
  const shownIds = useRef<string[]>([]);
  shownIds.current = items.map((i) => i.id);
  // A tick late, and called off by a mount in between: a remount (React's dev double mount) isn't leaving.
  useEffect(() => {
    clearTimeout(leaving);
    return () => {
      const ids = shownIds.current;
      leaving = window.setTimeout(() => markRead(ids));
    };
  }, []);

  const lead = items.find((i) => i.source === "roalt");
  const feed = items.filter((i) => i !== lead && (filter === "all" || i.source === filter));
  const count = (s: Source | "all") => items.filter((i) => i.unread && (s === "all" || i.source === s)).length || undefined;
  const sources = SOURCES.filter((s) => items.some((i) => i.source === s));
  const down = failed.map((s) => t(LABEL[s]));

  return (
    <section aria-labelledby="news-title" className="flex flex-col gap-5 min-w-0">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div className="min-w-0">
          <h1 id="news-title" className="page-title">{t("news.tab")}</h1>
          <p className="page-sub">{unread ? t("news.lead") : t("news.caughtUp")}</p>
        </div>
        <div className="flex items-center gap-2">
          <Button size="md" icon="check" disabled={!unread} onClick={() => markRead(items.map((i) => i.id))}>
            {t("news.markRead")}
          </Button>
          <IconButton icon="refresh" label={t("news.refresh")} disabled={query.isFetching} onClick={() => void query.refetch()} />
        </div>
      </header>
      {(down.length > 0 || query.isError) && (
        <div className="notice" data-tone="attention" role="status">
          <p className="alert flex-[1_1_240px] min-w-0">{t("news.failed", { sources: fmtList(down.length ? down : [t("news.tab")]) })}</p>
          <Button size="sm" busy={query.isFetching} onClick={() => void query.refetch()}>
            {t("common.tryAgain")}
          </Button>
        </div>
      )}
      <div className="news-grid">
        {lead ? <Lead item={lead} now={now} /> : query.isPending && <div className="panel news-lead"><Skeleton width="40%" height={32} /><Skeleton width="90%" /><Skeleton width="75%" /></div>}
        <div className="panel news-feed">
          {sources.length > 1 && (
            <div className="news-filters" role="group" aria-label={t("news.sources")}>
              <Chip small pressed={filter === "all"} count={count("all")} onClick={() => setFilter("all")}>{t("news.all")}</Chip>
              {sources.map((s) => (
                <Chip key={s} small pressed={filter === s} count={count(s)} onClick={() => setFilter(s)}>{t(LABEL[s])}</Chip>
              ))}
            </div>
          )}
          {query.isPending
            ? [78, 64, 84, 58].map((w) => (
                <div key={w} className="news-row" aria-hidden="true">
                  <span />
                  <span className="flex flex-col gap-2">
                    <Skeleton width={96} height={10} />
                    <Skeleton width={`${w}%`} height={13} />
                    <Skeleton width="92%" height={11} />
                  </span>
                </div>
              ))
            : feed.map((i) => <Row key={i.id} item={i} now={now} />)}
          {!query.isPending && !feed.length && <p className="news-empty">{t("news.empty")}</p>}
        </div>
      </div>
    </section>
  );
}
