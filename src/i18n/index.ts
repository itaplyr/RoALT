// UI translations: one flat JSON catalog per language in ./locales, English (en.json) the
// reference. `t` reads the current language from module state; Root sets it from
// Settings.language on every render, so a change re-renders the whole tree in the new language.
//
// Catalog strings use `{name}` placeholders. A plural key has one entry per CLDR category of its
// language (`key_one`, `key_few`, `key_many`, `key_other`); `t(key, { count })` picks one with
// Intl.PluralRules. `scripts/check-i18n.mjs` fails the build when a catalog misses a key.
import { Fragment, createElement, type ReactNode } from "react";
import ar from "./locales/ar.json";
import bs from "./locales/bs.json";
import de from "./locales/de.json";
import en from "./locales/en.json";
import es from "./locales/es.json";
import fil from "./locales/fil.json";
import fr from "./locales/fr.json";
import id from "./locales/id.json";
import ja from "./locales/ja.json";
import ko from "./locales/ko.json";
import pl from "./locales/pl.json";
import pt from "./locales/pt.json";
import ru from "./locales/ru.json";
import sv from "./locales/sv.json";
import th from "./locales/th.json";
import tr from "./locales/tr.json";
import vi from "./locales/vi.json";
import zh from "./locales/zh.json";

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
const catalogs: Record<Lang, Catalog> = { en, ar, bs, de, es, fil, fr, id, ja, ko, pl, pt, ru, sv, th, tr, vi, zh };

type PluralSuffix = "_one" | "_few" | "_many" | "_other";
type Base<K> = K extends `${infer B}${PluralSuffix}` ? B : K;
export type Key = Base<keyof typeof en>;
type Params = Record<string, string | number>;

let lang: Lang = "en";
let rules = new Intl.PluralRules("en");
let numbers = new Intl.NumberFormat("en");

const isLang = (l: string): l is Lang => l in catalogs;

/** A Settings.language value ("" follows Windows) to a supported language; else English. */
export function resolveLang(pref: string | undefined): Lang {
  if (pref && isLang(pref)) return pref;
  for (const tag of navigator.languages ?? [navigator.language]) {
    const base = tag.toLowerCase().split("-")[0];
    if (isLang(base)) return base;
  }
  return "en";
}

/** Switches the UI language; cheap when it is already current. */
export function setLang(pref: string | undefined) {
  const next = resolveLang(pref);
  if (next === lang) return;
  lang = next;
  rules = new Intl.PluralRules(lang);
  numbers = new Intl.NumberFormat(lang);
  document.documentElement.lang = lang;
  document.documentElement.dir = lang === "ar" ? "rtl" : "ltr";
}

export const getLang = () => lang;
/** The layout runs right to left: screen X in pointer and arrow-key logic is mirrored. */
export const isRtl = () => lang === "ar";

function lookup(key: Key, p?: Params): string {
  const cat = catalogs[lang];
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

/** A number in the current language (grouping, decimals). */
export const fmtNum = (n: number, o?: Intl.NumberFormatOptions) => n.toLocaleString(lang, o);
/** A date, time or both in the current language. */
export const fmtDate = (d: Date | number, o?: Intl.DateTimeFormatOptions) => new Date(d).toLocaleString(lang, o);
/** A list like "a, b and c" in the current language. */
export const fmtList = (items: string[], type: Intl.ListFormatType = "conjunction") => new Intl.ListFormat(lang, { type }).format(items);
