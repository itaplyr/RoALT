// The Accounts list's optional columns and its sort: what each shows, and what it sorts by.
import type { AccountSummary } from "../../api";
import { fmtNum, t, type Key } from "../../i18n";
import { json, useStored } from "../../ui/stored";
import { uptime } from "../launch/Running";
import { NONE } from "../monitor/format";
import { ago, formatDate } from "./shared";

export type Col = "added" | "checked" | "fps" | "playtime" | "robux" | "rap";
/** "order": the user's own (dragged) order; "name": the Account column. */
export type Sort = { by: "order" | "name" | Col; desc: boolean };

/** In menu and screen order. Robux is Roblox's own word in every language. */
export const COLS: readonly { id: Col; label: Key | { text: string } }[] = [
  { id: "added", label: "accounts.col.added" },
  { id: "checked", label: "accounts.col.checked" },
  { id: "fps", label: "accounts.detail.fpsCap" },
  { id: "playtime", label: "browse.tab.playtime" },
  { id: "robux", label: { text: "Robux" } },
  { id: "rap", label: "value.rap" },
];
export const colLabel = (c: Col) => {
  const l = COLS.find((x) => x.id === c)!.label;
  return typeof l === "string" ? t(l) : l.text;
};

/** What the columns that aren't on the account itself read from: each by account id; a missing
 * entry shows as none. */
export type Extra = { playtime: Map<number, number>; robux: Map<number, number>; rap: Map<number, number> };

/** A column's number for an account (what it sorts by); null: none, sorted last either way. */
export function colValue(c: Col, a: AccountSummary, x: Extra): number | null {
  switch (c) {
    case "added": return a.createdAt;
    case "checked": return a.statusCheckedAt;
    case "fps": return a.fpsCap;
    default: return x[c].get(a.id) ?? null;
  }
}

/** A column's cell for an account, in words. */
export function colText(c: Col, a: AccountSummary, x: Extra, now: number): string {
  const v = colValue(c, a, x);
  if (v === null) return NONE;
  switch (c) {
    case "added": return formatDate(v);
    case "checked": return ago(v, now);
    case "playtime": return uptime(0, v);
    default: return fmtNum(v);
  }
}

/** `list` in the sort's order (a copy; the user's own order as it came). Accounts without a value
 * go last, and ties keep the user's order. */
export function sorted(list: AccountSummary[], sort: Sort, x: Extra, name: (a: AccountSummary) => string): AccountSummary[] {
  if (sort.by === "order") return list;
  const dir = sort.desc ? -1 : 1;
  if (sort.by === "name") return [...list].sort((a, b) => dir * name(a).localeCompare(name(b), undefined, { sensitivity: "base", numeric: true }));
  const by = sort.by;
  return [...list].sort((a, b) => {
    const [va, vb] = [colValue(by, a, x), colValue(by, b, x)];
    if (va === null || vb === null) return va === vb ? 0 : va === null ? 1 : -1;
    return dir * (va - vb);
  });
}

/** A header click: sort by it (numbers biggest first, names A to Z), then the other way, then back
 * to the user's own order. */
export function nextSort(sort: Sort, by: "name" | Col): Sort {
  const first = by !== "name";
  if (sort.by !== by) return { by, desc: first };
  return sort.desc === first ? { by, desc: !first } : { by: "order", desc: false };
}

const isCol = (v: unknown): v is Col => COLS.some((c) => c.id === v);

/** The columns shown, kept on this PC. */
export const useCols = () =>
  useStored<Col[]>("roalt.accountCols", (s) => { const v = json(s); return Array.isArray(v) ? v.filter(isCol) : []; }, JSON.stringify);

/** The sort, kept on this PC. */
export const useSort = () =>
  useStored<Sort>(
    "roalt.accountSort",
    (s) => {
      const v = json(s) as Partial<Sort> | null;
      return v && (v.by === "order" || v.by === "name" || isCol(v.by)) ? { by: v.by, desc: !!v.desc } : { by: "order", desc: false };
    },
    JSON.stringify,
  );
