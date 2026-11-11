import { useEffect, useState, type SetStateAction } from "react";
import { api, type AccountSummary, type Instance, type InstanceState } from "../../api";
import type { Tone } from "../../ui/Status";
import type { LaunchActivity } from "../launch/launchQueue";
import { isLive } from "../launch/Running";
import { EMPTY_DRAFT, readDraft, targetText, type Draft } from "../launch/target";
import { useNewsFeed } from "../news/news";

/** How Home lays out its clients; remembered on this PC, apart from the accounts' launch order. */
const DETAILED = "roalt.homeDetailed";
const ORDER = "roalt.homeOrder";
const SIZES = "roalt.homeSizes";
const TABS = "roalt.homeTabs";
const SECTIONS = "roalt.sectionOrder";
/** When this PC keeps no storage, the choices last until RoALT closes. */
let unsavedSections: string[] | null = null;
let unsavedDetailed: boolean | null = null;
let unsavedOrder: number[] | null = null;
let unsavedSizes: Record<number, number> | null = null;
let unsavedTabs: StoredTab[] | null = null;

/** A tab of Home: one account, its own game. `status` is its launch, while one is under way or failed. */
export type HomeTab = { accountId: number; draft: Draft; status?: "queued" | "launching" | "failed" };
type StoredTab = { accountId: number; text: string };

// The unsaved value first: storage that still reads but no longer writes holds an older one.
function storedDetailed(): boolean {
  if (unsavedDetailed !== null) return unsavedDetailed;
  try {
    return localStorage.getItem(DETAILED) !== "0";
  } catch {
    return true;
  }
}

function storedOrder(): number[] {
  if (unsavedOrder) return unsavedOrder;
  try {
    const v: unknown = JSON.parse(localStorage.getItem(ORDER) ?? "null");
    return Array.isArray(v) ? v.filter((n): n is number => typeof n === "number") : [];
  } catch {
    return [];
  }
}

function storedSections(): string[] {
  if (unsavedSections) return unsavedSections;
  try {
    const v: unknown = JSON.parse(localStorage.getItem(SECTIONS) ?? "null");
    return Array.isArray(v) ? v.filter((s): s is string => typeof s === "string") : [];
  } catch {
    return [];
  }
}

function storedSizes(): Record<number, number> {
  if (unsavedSizes) return unsavedSizes;
  try {
    const v: unknown = JSON.parse(localStorage.getItem(SIZES) ?? "null");
    return v && typeof v === "object" && !Array.isArray(v) ? Object.fromEntries(Object.entries(v).filter(([, w]) => typeof w === "number")) : {};
  } catch {
    return {};
  }
}

/** Card widths the user chose, by account id as with the order; the others share a row's space. */
export function useHomeSizes(): [Record<number, number>, (id: number, width: number | null) => void] {
  const [sizes, setSizes] = useState(storedSizes);
  return [sizes, (id, width) => {
    const next = { ...sizes };
    if (width === null) delete next[id];
    else next[id] = width;
    unsavedSizes = next;
    setSizes(next);
    try {
      localStorage.setItem(SIZES, JSON.stringify(next));
    } catch {
      // No storage: kept in `unsavedSizes`.
    }
  }];
}

/** Detailed cards, or only the picture and the username. */
export function useHomeDetailed(): [boolean, (on: boolean) => void] {
  const [on, setOn] = useState(storedDetailed);
  return [on, (next) => {
    unsavedDetailed = next;
    setOn(next);
    try {
      localStorage.setItem(DETAILED, next ? "1" : "0");
    } catch {
      // No storage: kept in `unsavedDetailed`.
    }
  }];
}

/** Account ids in the order Home shows them. By account because an instance's id changes on rejoin. */
export function useHomeOrder(): [number[], (ids: number[]) => void] {
  const [order, setOrder] = useState(storedOrder);
  return [order, (next) => {
    unsavedOrder = next;
    setOrder(next);
    try {
      localStorage.setItem(ORDER, JSON.stringify(next));
    } catch {
      // No storage: kept in `unsavedOrder`.
    }
  }];
}

/** `list` in the remembered order; accounts it doesn't know yet follow, as they came. */
export function homeSorted<T extends { accountId: number }>(list: T[], order: number[]): T[] {
  const at = new Map(order.map((id, n) => [id, n]));
  const rank = (i: T) => at.get(i.accountId) ?? order.length;
  return [...list].sort((a, b) => rank(a) - rank(b));
}

/** `ids` in the remembered order; ones it doesn't know yet follow, as they came. */
export function byOrder<T>(ids: readonly T[], order: T[]): T[] {
  const at = new Map(order.map((id, n) => [id, n]));
  const rank = (id: T) => at.get(id) ?? order.length;
  return [...ids].sort((a, b) => rank(a) - rank(b));
}

/** RoALT's sections in the order the user dragged them to, by name. */
export function useSectionOrder(): [string[], (ids: string[]) => void] {
  const [order, setOrder] = useState(storedSections);
  return [order, (next) => {
    unsavedSections = next;
    setOrder(next);
    try {
      localStorage.setItem(SECTIONS, JSON.stringify(next));
    } catch {
      // No storage: kept in `unsavedSections`.
    }
  }];
}

/** The order after moving shown[from] before shown[over] (or after the last shown, at the end).
 * `shown` holds one id per tile (an account's, for clients, tabs and rows); ids not shown now keep
 * their place in `order`, and two tiles of one account move together. */
export function reordered<T>(order: T[], shown: T[], from: number, over: number): T[] {
  const id = shown[from];
  const ids = [...new Set([...order, ...shown])].filter((x) => x !== id);
  const next = shown.slice(over).find((x) => x !== id);
  const others = shown.filter((x) => x !== id);
  ids.splice(next !== undefined ? ids.indexOf(next) : ids.indexOf(others[others.length - 1]) + 1, 0, id);
  return ids;
}

/** What of a draft may be written to disk. Anyone with a private server's or a share link's code
 * can join, so those stay in memory until RoALT closes. Only known-harmless parses pass, rebuilt
 * from the parse and not the typed text: text that doesn't parse may be a mistyped link with a code,
 * and a bare job ID reads the same as a private server's access code. */
export function storedText(d: Draft): string {
  const p = d.parsed?.text === d.text ? d.parsed.value : null;
  switch (p?.kind) {
    case "target": {
      const g = p.target;
      const harmless = g.kind === "server" || g.kind === "universe" || g.kind === "followUser" || (g.kind === "place" && !g.launchData);
      return harmless ? targetText(g) : "";
    }
    case "username":
      return `@${p.name}`;
    default:
      return "";
  }
}

function storedTabs(): HomeTab[] {
  let v: unknown = unsavedTabs;
  if (!v) {
    try {
      v = JSON.parse(localStorage.getItem(TABS) ?? "null");
    } catch {
      // No storage, nothing saved.
    }
  }
  if (!Array.isArray(v)) return [];
  return (v as Partial<StoredTab>[])
    .filter((s) => typeof s?.accountId === "number" && typeof s.text === "string")
    .map((s) => ({ accountId: s.accountId!, draft: { ...EMPTY_DRAFT, text: s.text! } }));
}

const unparsed = (d: Draft) => !!d.text.trim() && d.parsed?.text !== d.text;

/** Home's open tab: an account's id, "news", or null for Overview. */
export type HomeActive = number | "news" | null;
/** Home opens on News once per start, the first time it knows of something unread. */
let landed = false;

/** Home's tabs. Shell owns them, so they and a launch in flight survive leaving Home, and a link
 * code in one goes with everything else when the vault locks. */
export function useHomeTabs(accounts: AccountSummary[] | undefined) {
  const [tabs, setTabs] = useState(storedTabs);
  const [active, setActive] = useState<HomeActive>(null);
  const { unread, query } = useNewsFeed();
  // Only while Home is still on Overview: a tab picked meanwhile stays.
  useEffect(() => {
    if (landed || !query.isFetched) return;
    landed = true;
    if (unread) setActive((a) => (a === null ? "news" : a));
  }, [query.isFetched, unread]);
  // One order for an account's client, tab and row, so sorting one sorts them all.
  const [order, setOrder] = useHomeOrder();
  const setDraft =(id: number, f: SetStateAction<Draft>) =>
    setTabs((list) =>
      list.map((tab) => {
        if (tab.accountId !== id) return tab;
        const draft = typeof f === "function" ? f(tab.draft) : f;
        // Another game: what failed was the last one.
        return { ...tab, draft, status: tab.status === "failed" && draft.text !== tab.draft.text ? undefined : tab.status };
      }),
    );

  // A removed account's id can come back for another account.
  useEffect(() => {
    if (accounts) setTabs((list) => (list.every((tab) => accounts.some((a) => a.id === tab.accountId)) ? list : list.filter((tab) => accounts.some((a) => a.id === tab.accountId))));
  }, [accounts]);

  // Every tab's parse, the ones not on screen and the ones read back from disk too.
  useEffect(() => {
    const stale = tabs.filter((tab) => unparsed(tab.draft));
    if (!stale.length) {
      // Only now: until it is parsed, text may hold a link code. What was saved stays till then.
      const saved = tabs.map((tab): StoredTab => ({ accountId: tab.accountId, text: storedText(tab.draft) }));
      unsavedTabs = saved;
      try {
        localStorage.setItem(TABS, JSON.stringify(saved));
      } catch {
        // No storage: kept in `unsavedTabs`.
      }
      return;
    }
    const timer = setTimeout(() => {
      for (const { accountId, draft: { text } } of stale)
        api.targetParse(text).then(
          (value) => setDraft(accountId, (d) => (d.text === text ? { ...d, parsed: { text, value } } : d)),
          () => {}, // launching parses again and shows the error
        );
    }, 150);
    return () => clearTimeout(timer);
  }, [tabs]); // eslint-disable-line react-hooks/exhaustive-deps

  return {
    tabs,
    order,
    setOrder,
    active: active === "news" || tabs.some((tab) => tab.accountId === active) ? active : null,
    setActive,
    setDraft,
    add: (id: number) => {
      setTabs((list) => (list.some((tab) => tab.accountId === id) ? list : [...list, { accountId: id, draft: EMPTY_DRAFT }]));
      setActive(id);
    },
    close: (id: number) => {
      setTabs((list) => list.filter((tab) => tab.accountId !== id));
      setActive((a) => (a === id ? null : a));
    },
    setStatus: (ids: number[], status: HomeTab["status"]) =>
      setTabs((list) => list.map((tab) => (ids.includes(tab.accountId) ? { ...tab, status } : tab))),
  };
}
export type HomeTabs = ReturnType<typeof useHomeTabs>;

/** A client's state as a status tone (shape and hue), the way cards, tabs and rail rows show it. */
export const STATE_TONE: Record<InstanceState, Tone> = { starting: "neutral", joining: "info", inGame: "ok", disconnected: "attention", exited: "neutral" };

/** The tabs "Play with N" starts: a signed-in account, a target that reads (or is still being read)
 * and no client of its own on its way or in a game. */
export function readyTabs(tabs: HomeTab[], byId: Map<number, AccountSummary>, instances: Instance[]) {
  return tabs.filter((tab) => {
    const { usable, pending } = readDraft(tab.draft);
    return byId.get(tab.accountId)?.status === "valid" && (usable || pending) && !instances.some((i) => i.accountId === tab.accountId && isLive(i));
  });
}

/** Where an account is in the launch on show: waiting for its turn, launching, or it didn't start
 * (and has no client on its way or in a game). */
export function tabLaunch(activity: LaunchActivity | null, accountId: number, live: boolean): HomeTab["status"] {
  const kind = activity?.rows.find((r) => r.accountId === accountId && r.account)?.stage.kind;
  if (kind === "queued") return activity!.done ? undefined : "queued";
  if (kind === "ticket") return "launching";
  return kind === "failed" && !live ? "failed" : undefined;
}
