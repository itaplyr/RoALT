import { t, type Key } from "../../i18n";
import { VIEW_NAME } from "../../app/navigation";
import { AREAS } from "./areas";
import { dyn, ENTRIES, entry, paragraphs, type Ctx, type Entry, type Place } from "./registry";

/** Which of an entry's texts matched, best first. */
export type Where = "label" | "hint" | "more" | "alias" | "place";
/** `row`: what to show (a child shows as its parent's row); `via`: the entry that matched. */
export type Hit = { row: Entry; via: Entry; where: Where };
export type HitGroup = { place: Place; hits: Hit[] };

const LIMIT = 60;

/** Lower case, without accents or other marks: "Élan" finds "elan". */
export const fold = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

// Settings rows take their group from the area that lists them.
const GROUP = new Map<string, Key | undefined>(AREAS.flatMap((a) => a.groups.flatMap((g) => g.ids.map((id) => [id, g.label] as const))));
const root = (e: Entry): Entry => (e.parent ? root(entry(e.parent)) : e);
const placeOf = (row: Entry): Place => (row.place.view === "settings" ? { ...row.place, group: GROUP.get(row.id) } : row.place);
const screenOf = (p: Place) => (p.view === "settings" ? `settings:${p.area}` : p.view);
/** A place's stable key: its screen and group. */
export const keyOf = (p: Place) => `${screenOf(p)}|${"group" in p ? (p.group ?? "") : ""}`;

// Island order: Settings' areas, then Performance, Discord, then the views links go to.
const SCREENS = [...AREAS.map((a) => `settings:${a.id}`), "performance", "discord", "accounts", "launch", "dashboard"];
const ORDER = new Map(ENTRIES.map((e, i) => [e.id, i]));
const GROUP_ORDER = new Map<string, number>();
ENTRIES.forEach((e, i) => {
  const k = keyOf(placeOf(root(e)));
  if (!GROUP_ORDER.has(k)) GROUP_ORDER.set(k, i);
});

/** The names a place goes by: its area or view, then its group. */
export function placeNames(p: Place): string[] {
  const where = t(p.view === "settings" ? AREAS.find((a) => a.id === p.area)!.label : VIEW_NAME[p.view]);
  return "group" in p && p.group ? [where, t(p.group)] : [where];
}
const placeWords = (p: Place) => placeNames(p).join(" ");

/** The row a hit on `e` shows as: a child in its parent's row, a hidden row as its fallback; or none. */
function rowFor(e: Entry, ctx: Ctx): Entry | null {
  if (e.parent) return rowFor(entry(e.parent), ctx);
  if (e.when && !e.when(ctx)) return e.fallback ? rowFor(entry(e.fallback), ctx) : null;
  return e;
}

if (import.meta.env.DEV) {
  const windows = { vault: { mode: "dpapi" } } as Ctx;
  console.assert(rowFor(entry("masterPassword"), windows) === entry("encryption"), "search: a hidden row shows as its fallback");
  console.assert(rowFor(entry("antiAfkEvery"), windows) === entry("antiAfk"), "search: a child shows as its parent's row");
}

/** Every setting that has all of the query's words, in the current language: in its label, hint,
 * "More about" text, aliases or place names. Best first (the label starting with the query, then
 * the label, hint, more, alias, place), at most 60, grouped by place in island order. */
export function search(q: string, ctx: Ctx): HitGroup[] {
  const words = fold(q).split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  const all = (text: string) => words.every((w) => text.includes(w));
  const best = new Map<string, { hit: Hit; score: number }>();
  for (const e of ENTRIES) {
    const row = rowFor(e, ctx);
    if (!row) continue;
    const texts: [Where, string][] = [
      ["label", fold(t(e.label))],
      ["hint", e.hint ? fold(dyn(e.hint, ctx)) : ""],
      ["more", fold(paragraphs(e, ctx).join("\n"))],
      ["alias", (e.aliases ?? []).join("\n")],
      ["place", fold(placeWords(placeOf(row)))],
    ];
    let i = texts.findIndex(([, text]) => all(text));
    // Words spread over several texts still match, after every single-text match.
    const spread = i < 0;
    if (spread && !all(texts.map(([, text]) => text).join("\n"))) continue;
    if (spread) i = texts.findIndex(([, text]) => words.some((w) => text.includes(w)));
    const score = spread ? texts.length : i === 0 && texts[0][1].startsWith(words.join(" ")) ? -1 : i;
    const had = best.get(row.id);
    if (!had || score < had.score) best.set(row.id, { hit: { row, via: e, where: texts[i][0] }, score });
  }
  const groups = new Map<string, HitGroup>();
  for (const { hit } of [...best.values()].sort((a, b) => a.score - b.score || ORDER.get(a.hit.row.id)! - ORDER.get(b.hit.row.id)!).slice(0, LIMIT)) {
    const place = placeOf(hit.row);
    const k = keyOf(place);
    if (!groups.has(k)) groups.set(k, { place, hits: [] });
    groups.get(k)!.hits.push(hit);
  }
  return [...groups.entries()]
    .sort(([a, ga], [b, gb]) => SCREENS.indexOf(screenOf(ga.place)) - SCREENS.indexOf(screenOf(gb.place)) || GROUP_ORDER.get(a)! - GROUP_ORDER.get(b)!)
    .map(([, g]) => g);
}
