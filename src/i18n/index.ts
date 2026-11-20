// UI translations: one flat JSON catalog per language in ./locales, English (en.json) the
// reference and built in. Every other catalog is its own chunk, loaded when its language is
// chosen: main.tsx loads the last one before the first render, and Root (`useLang`) follows
// Settings.language, re-rendering the whole tree once the new catalog is in.
//
// Catalog strings use `{name}` placeholders. A plural key has one entry per CLDR category of its
// language (`key_one`, `key_few`, `key_many`, `key_other`); `t(key, { count })` picks one with
// Intl.PluralRules. `scripts/check-i18n.mjs` fails the build when a catalog misses a key.
import { Fragment, createElement, useEffect, useReducer, type ReactNode } from "react";
import { readStored, writeStored } from "../ui/stored";
import en from "./locales/en.json";

/** Language codes with their own name, in picker order. */
export const LANGUAGES = [
  ["en", "English"],
  ["ar", "العربية"],
  ["bs", "Bosanski"],
  ["de", "Deutsch"],
  ["es", "Español"],
  ["fil", "Filipino"],
  ["fr", "Français"],
  ["id", "Bahasa Indonesia"],
  ["ja", "日本語"],
  ["ko", "한국어"],
  ["pl", "Polski"],
  ["pt", "Português (Brasil)"],
  ["ru", "Русский"],
  ["sv", "Svenska"],
  ["th", "ไทย"],
  ["tr", "Türkçe"],
  ["vi", "Tiếng Việt"],
  ["zh", "简体中文"],
] as const;
export type Lang = (typeof LANGUAGES)[number][0];

type Catalog = Record<string, string>;
const loaders = import.meta.glob<Catalog>(["./locales/*.json", "!./locales/en.json"], { import: "default" });
const catalogs: Partial<Record<Lang, Catalog>> = { en };
/** The last language shown, so the next start loads it before the first render. */
const LANG_KEY = "roalt.lang";

type PluralSuffix = "_one" | "_few" | "_many" | "_other";
type Base<K> = K extends `${infer B}${PluralSuffix}` ? B : K;
export type Key = Base<keyof typeof en>;
type Params = Record<string, string | number>;

let lang: Lang = "en";
let rules = new Intl.PluralRules("en");
let numbers = new Intl.NumberFormat("en");

const isLang = (l: string): l is Lang => LANGUAGES.some(([code]) => code === l);

/** A Settings.language value ("" follows Windows) to a supported language; else English. */
export function resolveLang(pref: string | undefined): Lang {
  if (pref && isLang(pref)) return pref;
  for (const tag of navigator.languages ?? [navigator.language]) {
    const base = tag.toLowerCase().split("-")[0];
    if (isLang(base)) return base;
  }
  return "en";
}

/** Switches to a language whose catalog is loaded; cheap when it is already current. */
function apply(next: Lang) {
  if (next === lang) return;
  lang = next;
  rules = new Intl.PluralRules(lang);
  numbers = new Intl.NumberFormat(lang);
  document.documentElement.lang = lang;
  document.documentElement.dir = lang === "ar" ? "rtl" : "ltr";
  // Without storage, the next start shows English until settings load.
  writeStored(LANG_KEY, lang);
}

/** Loads `next`'s catalog; false when it couldn't be (the current language stays). */
async function load(next: Lang) {
  try {
    catalogs[next] ??= await loaders[`./locales/${next}.json`]();
    return true;
  } catch (e) {
    console.error(`language ${next} failed to load: ${e}`);
    return false;
  }
}

/** Before the first render: the language shown last, so the start doesn't flash English. */
export async function loadLastLang() {
  // No storage: the Windows language.
  const next = resolveLang(readStored(LANG_KEY) ?? undefined);
  if (await load(next)) apply(next);
}

/** Shows Settings.language (`undefined` while settings load: what is shown stays). At once when
 *  its catalog is loaded; else the current language stays until it is, then the tree re-renders. */
export function useLang(pref: string | undefined) {
  const [, loaded] = useReducer((n: number) => n + 1, 0);
  const next = pref === undefined ? null : resolveLang(pref);
  if (next && catalogs[next]) apply(next);
  useEffect(() => {
    if (next && !catalogs[next]) void load(next).then((ok) => ok && loaded());
  }, [next]);
}

export const getLang = () => lang;
/** The layout runs right to left: screen X in pointer and arrow-key logic is mirrored. */
export const isRtl = () => lang === "ar";

function lookup(key: Key, p?: Params): string {
  const cat = catalogs[lang] ?? (en as Catalog);
  const plural = typeof p?.count === "number" ? `${key}_${rules.select(p.count)}` : null;
  return (plural && (cat[plural] ?? cat[`${key}_other`])) ?? cat[key] ?? (en as Catalog)[key] ?? (en as Catalog)[`${key}_other`] ?? key;
}

/** `count` is shown as a localized number; other numbers as they are (ports, ids, years). */
const show = (name: string, v: string | number) => (name === "count" && typeof v === "number" ? numbers.format(v) : String(v));

/** The translated string for `key`, with `{name}` placeholders filled from `p`. */
export function t(key: Key, p?: Params): string {
  return lookup(key, p).replace(/\{(\w+)\}/g, (m, n: string) => (p && n in p ? show(n, p[n]) : m));
}

/** Like `t`, for strings with markup: `{name}` may be a React node, and `<b>text</b>` calls
 *  `tags.b("text")`. Tags don't nest. */
export function tx(key: Key, p: Record<string, ReactNode | string | number> = {}, tags: Record<string, (s: string) => ReactNode> = {}): ReactNode {
  const text = lookup(key, p as Params);
  const out: ReactNode[] = [];
  let last = 0;
  for (const m of text.matchAll(/<(\w+)>(.*?)<\/\1>|\{(\w+)\}/g)) {
    out.push(text.slice(last, m.index));
    if (m[1]) out.push(tags[m[1]] ? tags[m[1]](m[2]) : m[2]);
    else {
      const v = p[m[3]];
      out.push(v === undefined ? m[0] : typeof v === "number" ? show(m[3], v) : v);
    }
    last = m.index + m[0].length;
  }
  out.push(text.slice(last));
  return createElement(Fragment, null, ...out);
}

// Formatters by language + options: toLocaleString builds a new one per call when given options,
// and Num formats every animation frame. The key holds the language, so a switch never reuses one.
const formats = new Map<string, Intl.NumberFormat>();
/** A number in the current language (grouping, decimals). */
export const fmtNum = (n: number, o?: Intl.NumberFormatOptions) => {
  if (!o) return n.toLocaleString(lang);
  const key = lang + JSON.stringify(o);
  let f = formats.get(key);
  if (!f) formats.set(key, (f = new Intl.NumberFormat(lang, o)));
  return f.format(n);
};
/** A date, time or both in the current language. */
export const fmtDate = (d: Date | number, o?: Intl.DateTimeFormatOptions) => new Date(d).toLocaleString(lang, o);
/** A list like "a, b and c" in the current language. */
export const fmtList = (items: string[], type: Intl.ListFormatType = "conjunction") => new Intl.ListFormat(lang, { type }).format(items);
