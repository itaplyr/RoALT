// Fails when a translation catalog is incomplete or out of step with English:
// - UI (src/i18n/locales/*.json): every key of en.json, with one entry per plural category of the
//   language for plural keys (`key_one`/`key_other` in English), no extra keys, the same
//   `{placeholders}` and `<tags>` as English.
// - Rust (src-tauri/locales/*.json): every `tr!`/`trn!` key in src-tauri/src, likewise.
// `--rust-keys` prints the Rust keys as an English catalog instead (a template for translators).
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const LANGS = ["ar", "bs", "de", "es", "fil", "fr", "id", "ja", "ko", "pl", "pt", "ru", "sv", "th", "tr", "vi", "zh"];
const PLURAL = /_(zero|one|two|few|many|other)$/;
const read = (p) => JSON.parse(readFileSync(join(root, p), "utf8"));
const categories = (lang) => [...new Set(Array.from({ length: 200 }, (_, n) => new Intl.PluralRules(lang).select(n)))].sort();
const marks = (s) => [...s.matchAll(/\{\w+\}|<\/?\w+>/g)].map((m) => m[0]).filter((m) => m !== "{count}" && m !== "{n}").sort().join(" ");

// ---- Rust keys ----
function rsFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? rsFiles(join(dir, e.name)) : e.name.endsWith(".rs") ? [join(dir, e.name)] : [],
  );
}
const lit = String.raw`"((?:[^"\\]|\\[\s\S])*)"`;
const unescape = (s) =>
  s.replace(/\\\r?\n\s*/g, "").replace(/\\(["\\nt'])/g, (_, c) => ({ n: "\n", t: "\t" })[c] ?? c);
const rust = { text: new Map(), plural: new Map() };
for (const file of rsFiles(join(root, "src-tauri", "src"))) {
  const code = readFileSync(file, "utf8").replace(/^\s*\/\/.*$/gm, "");
  for (const m of code.matchAll(new RegExp(String.raw`\btr!\(\s*` + lit, "g"))) rust.text.set(unescape(m[1]), file);
  for (const m of code.matchAll(new RegExp(String.raw`\btrn!\(\s*[^,]+,\s*` + lit + String.raw`\s*,\s*` + lit, "g")))
    rust.plural.set(unescape(m[2]), unescape(m[1]));
}
if (process.argv.includes("--rust-keys")) {
  const out = Object.fromEntries([...rust.text.keys()].map((k) => [k, k]));
  for (const [other, one] of rust.plural) out[other] = { one, other };
  console.log(JSON.stringify(out, null, 2));
  process.exit(0);
}

const problems = [];
const warnings = [];

for (const lang of LANGS) {
  const cats = categories(lang);

  // UI
  const en = read("src/i18n/locales/en.json");
  const cat = read(`src/i18n/locales/${lang}.json`);
  const want = new Map(); // key -> English text it must match
  for (const [k, v] of Object.entries(en)) {
    if (PLURAL.test(k)) {
      const base = k.replace(PLURAL, "");
      for (const c of cats) want.set(`${base}_${c}`, en[`${base}_other`] ?? v);
    } else want.set(k, v);
  }
  for (const [k, v] of want) {
    const got = cat[k];
    if (typeof got !== "string" || !got.trim()) problems.push(`${lang}.json: missing "${k}"`);
    else {
      if (marks(got) !== marks(v)) problems.push(`${lang}.json: "${k}" has ${marks(got) || "no marks"}, English has ${marks(v) || "none"}`);
      if (got === v && /[a-z]{4,}.*\s.*[a-z]{4,}/.test(v)) warnings.push(`${lang}.json: "${k}" is still English`);
    }
  }
  for (const k of Object.keys(cat)) if (!want.has(k)) problems.push(`${lang}.json: extra key "${k}"`);

  // Rust
  const rs = read(`src-tauri/locales/${lang}.json`);
  for (const [k] of rust.text) {
    const got = rs[k];
    if (typeof got !== "string" || !got.trim()) problems.push(`src-tauri/locales/${lang}.json: missing ${JSON.stringify(k)}`);
    else if (marks(got) !== marks(k)) problems.push(`src-tauri/locales/${lang}.json: ${JSON.stringify(k)} has ${marks(got) || "no marks"}`);
  }
  for (const [k] of rust.plural) {
    const got = rs[k];
    for (const c of cats) {
      if (typeof got?.[c] !== "string") problems.push(`src-tauri/locales/${lang}.json: ${JSON.stringify(k)} misses its "${c}" form`);
      else if (marks(got[c]) !== marks(k)) problems.push(`src-tauri/locales/${lang}.json: ${JSON.stringify(k)} "${c}" has ${marks(got[c]) || "no marks"}`);
    }
  }
  for (const k of Object.keys(rs)) if (!rust.text.has(k) && !rust.plural.has(k)) problems.push(`src-tauri/locales/${lang}.json: extra key ${JSON.stringify(k)}`);
}

for (const w of warnings) console.warn(`warning: ${w}`);
if (problems.length) {
  for (const p of problems.slice(0, 200)) console.error(p);
  console.error(`\n${problems.length} translation problem(s).`);
  process.exit(1);
}
console.log(`Translations complete: ${LANGS.length} languages, ${rust.text.size + rust.plural.size} Rust keys.`);
