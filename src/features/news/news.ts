import { useSyncExternalStore } from "react";
import type { NewsItem, NewsSource } from "../../api";
import { useNews, useRobloxClient } from "../../data";
import { t } from "../../i18n";
import { signal } from "../../ui/signal";
import { json, readStored, writeStored } from "../../ui/stored";

/** A News row: the backend's, or the Roblox client update RoALT already knows of (`source` "client"). */
export type FeedItem = Omit<NewsItem, "source" | "url"> & { source: NewsSource | "client"; url: string | null; unread: boolean };

const SEEN = "roalt.newsSeen";
/** Ids kept: far more than the sources ever list at once. */
const KEEP = 400;
/** Older than this, an item is never new (a first start doesn't open on a backlog). */
const FRESH_MS = 14 * 24 * 60 * 60_000;

/** Read items by id; on this PC only. Without storage they last until RoALT closes. */
let seen: string[] = (() => {
  const v = json(readStored(SEEN));
  return Array.isArray(v) ? v.filter((s): s is string => typeof s === "string") : [];
})();
let seenSet = new Set(seen);
const { subscribe, notify } = signal();

export function markRead(ids: string[]) {
  const fresh = ids.filter((id) => !seenSet.has(id));
  if (!fresh.length) return;
  seen = [...seen, ...fresh].slice(-KEEP);
  seenSet = new Set(seen);
  writeStored(SEEN, JSON.stringify(seen));
  notify();
}

const useSeen = () => useSyncExternalStore(subscribe, () => seenSet);

/** Within FRESH_MS; `YYYY-MM` counts from the month's first day. An empty date (the client) is fresh. */
const isFresh = (published: string, now: number) => !published || now - Date.parse(published) < FRESH_MS;

/** Every item, newest first, the client update on top while unread; the unread count; what failed. */
export function useNewsFeed() {
  const news = useNews();
  const latest = useRobloxClient().data?.latest ?? null;
  const read = useSeen();
  const now = Date.now();
  const unread = (id: string, published: string) => !read.has(id) && isFresh(published, now);
  const items: FeedItem[] = (news.data?.items ?? []).map((i) => ({ ...i, unread: unread(i.id, i.published) }));
  if (latest) {
    const id = `client:${latest}`;
    const client: FeedItem = { id, source: "client", title: t("news.clientTitle", { version: latest }), summary: t("news.clientText"), url: null, published: "", version: null, unread: unread(id, "") };
    if (client.unread) items.unshift(client);
    else items.push(client);
  }
  return {
    items,
    unread: items.filter((i) => i.unread).length,
    failed: news.data?.failed ?? [],
    query: news,
  };
}
