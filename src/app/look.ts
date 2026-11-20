// The user's own colours: which parts can change, the shades worked out between them, and the pass
// that keeps text readable on whatever was picked. Pure (no React, no app imports): the shell applies
// it, and scripts/test-look.mjs checks it in Node.
import type { Look } from "../bindings/Look";
import type { Key } from "../i18n";

// ---- Colour math (sRGB hex) ----

const channel = (v: number) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
const rgb = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
const luminance = (hex: string) => {
  const [r, g, b] = rgb(hex).map((v) => channel(v / 255));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
/** The WCAG contrast ratio of two `#rrggbb` colours. */
export const contrast = (a: string, b: string) => {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};
/** `hex` moved `share` (0..1) of the way to `toward`. */
export const mix = (hex: string, toward: string, share: number) => {
  const [a, b] = [rgb(hex), rgb(toward)];
  return `#${a.map((v, i) => Math.round(v + (b[i] - v) * share).toString(16).padStart(2, "0")).join("")}`;
};
export const isColour = (c: unknown): c is string => typeof c === "string" && /^#[0-9a-f]{6}$/i.test(c);

/** `c`, or the least move of it toward white or black that reads at `min`:1 on every one of `on`.
 * Where no shade reads on all of them (on both black and white), the one that reads best. */
export function readable(c: string, on: readonly string[], min: number): string {
  const worst = (x: string) => Math.min(...on.map((o) => contrast(x, o)));
  if (worst(c) >= min) return c;
  let best: string | null = null;
  let bestStep = Infinity;
  for (const to of ["#ffffff", "#000000"])
    for (let i = 1; i <= 20; i++) {
      const x = mix(c, to, i / 20);
      if (worst(x) >= min) {
        if (i < bestStep) [best, bestStep] = [x, i];
        break;
      }
    }
  return best ?? (worst("#ffffff") >= worst("#000000") ? "#ffffff" : "#000000");
}

// ---- What can change ----

/** The grounds an accent is checked against. */
export type Grounds = { ground: string; island: string; edge: string };
/** The default themes' (tokens.css). */
export const GROUNDS: Record<"dark" | "light", Grounds> = {
  dark: { ground: "#0a0a0a", island: "#141414", edge: "#6e6e6e" },
  light: { ground: "#f3f3f3", island: "#ffffff", edge: "#7c7c7c" },
};

/** What a user accent becomes on these grounds: the fill as chosen, the ink on it (whichever of near
 * black and white contrasts more), a text shade that reads at 4.5:1 on the ground, and an edge when
 * the fill alone is under 3:1 against the island. */
export function accentTokens(hex: string, g: Grounds) {
  const ink = contrast(hex, "#0a0a0a") >= contrast(hex, "#ffffff") ? "#0a0a0a" : "#ffffff";
  const text = readable(hex, [g.ground], 4.5);
  const ring = contrast(hex, g.island) < 3 ? g.edge : "transparent";
  return { fill: hex, ink, text, ring };
}

/** The ids a look stores (`--{id}` is the token), the plain parts first, in the editor's order. */
export const PARTS = ["ground", "titlebar", "island", "surface-1", "surface-2", "float", "thumb", "fg", "edge"] as const;
/** The rest of the solid tokens, behind "Advanced". */
export const ADVANCED = ["surface-3", "island-inset", "fg-2", "fg-3", "fg-disabled", "on-fg", "line", "ok", "info", "attention", "warn", "err"] as const;
export type Part = (typeof PARTS)[number] | (typeof ADVANCED)[number];
const KNOWN = new Set<string>([...PARTS, ...ADVANCED]);
export const partLabel = (id: Part) => `look.part.${id}` as Key;

type Base = Look["base"];
/** Each base's solid colours (tokens.css). */
const BASE: Record<Base, Record<Exclude<Part, "titlebar" | "line">, string>> = {
  dark: {
    ground: "#0a0a0a", "surface-1": "#111111", "surface-2": "#191919", "surface-3": "#222222", island: "#141414", "island-inset": "#222222", float: "#161616",
    thumb: "#303030", edge: "#6e6e6e", fg: "#ededed", "fg-2": "#b0b0b0", "fg-3": "#949494", "fg-disabled": "#6a6a6a", "on-fg": "#0a0a0a",
    ok: "#4ade80", info: "#60a5fa", attention: "#fb923c", warn: "#fbbf24", err: "#f87171",
  },
  light: {
    ground: "#f3f3f3", "surface-1": "#ffffff", "surface-2": "#ececec", "surface-3": "#e2e2e2", island: "#ffffff", "island-inset": "#ececec", float: "#ffffff",
    thumb: "#ffffff", edge: "#7c7c7c", fg: "#111111", "fg-2": "#4d4d4d", "fg-3": "#616161", "fg-disabled": "#9c9c9c", "on-fg": "#f3f3f3",
    ok: "#166534", info: "#1d4ed8", attention: "#9a3412", warn: "#854d0e", err: "#b91c1c",
  },
};
/** The tints that follow the text colour, % of it: as tokens.css's per base. */
const TINTS: Record<Base, Record<string, number>> = {
  dark: { hover: 7, press: 11, sel: 9, pick: 15, "pick-ring": 26, line: 8, scrollbar: 18 },
  light: { hover: 5, press: 9, sel: 6, pick: 8, "pick-ring": 28, line: 9, scrollbar: 22 },
};


/** Every token a look sets, by id (no `--`), and the grounds its accent is checked on. A part nobody
 * picked keeps the base's colour, unless one it is worked out from was picked: then it follows that
 * one (panels from the window background, fields from panels, …). Then text, borders and status
 * colours move just enough to read on the surfaces they sit on. */
export function lookTokens(look: Look) {
  const base = BASE[look.base] ?? BASE.dark;
  const picks = new Map(Object.entries(look.colors).filter(([id, c]) => KNOWN.has(id) && isColour(c)).map(([id, c]) => [id, c.toLowerCase()]));
  const out: Record<string, string> = {};
  const touched = new Set<string>();
  /** A part: picked, else worked out from `from` (lifted `by` toward the text) once that changed. */
  const part = (id: Exclude<Part, "titlebar" | "line">, from?: string, by = 0) => {
    const pick = picks.get(id);
    if (pick) touched.add(id);
    else if (from && touched.has(from)) touched.add(id);
    out[id] = pick ?? (from && touched.has(from) ? mix(out[from], out.fg, by) : base[id]);
  };
  part("fg");
  part("ground");
  part("surface-1", "ground", 0.03);
  part("surface-2", "surface-1", 0.04);
  part("surface-3", "surface-2", 0.04);
  part("island", "ground", 0.04);
  part("island-inset", "island", 0.07);
  part("float", "surface-1", 0.02);
  part("thumb", "surface-2", 0.1);
  // Text and edges lean toward the panels they sit on; they follow either one.
  const follows = touched.has("fg") || touched.has("surface-1");
  for (const [id, share] of [["fg-2", 0.28], ["fg-3", 0.4], ["fg-disabled", 0.6], ["edge", 0.5]] as const)
    out[id] = picks.get(id) ?? (follows ? mix(out.fg, out["surface-1"], share) : base[id]);
  out["on-fg"] = picks.get("on-fg") ?? (touched.has("fg") || touched.has("ground") ? out.ground : base["on-fg"]);
  for (const id of ["ok", "info", "attention", "warn", "err"] as const) out[id] = picks.get(id) ?? base[id];
  const titlebar = picks.get("titlebar");

  // Readable whatever was picked: the swatch keeps the pick, the screen gets these.
  const under = [out.ground, out["surface-1"], out["surface-2"], out["surface-3"], out.float, out.island, out.thumb, ...(titlebar ? [titlebar] : [])];
  out.fg = readable(out.fg, under, 4.5);
  out["fg-2"] = readable(out["fg-2"], [out.ground, out["surface-1"], out["surface-2"], out.float], 4.5);
  out["fg-3"] = readable(out["fg-3"], [out.ground, out["surface-1"], out.float], 4.5);
  out.edge = readable(out.edge, [out.ground, out["surface-1"], out["surface-2"], out.island], 3);
  out["on-fg"] = readable(out["on-fg"], [out.fg], 4.5);
  for (const id of ["ok", "info", "attention", "warn", "err"]) out[id] = readable(out[id], [out.ground, out["surface-1"], out.float], 4.5);

  const tokens: Record<string, string> = { ...out, titlebar: titlebar ?? "transparent" };
  for (const [id, pct] of Object.entries(TINTS[look.base] ?? TINTS.dark)) tokens[id] = `color-mix(in srgb, ${out.fg} ${pct}%, transparent)`;
  const line = picks.get("line");
  if (line) tokens.line = line;
  const grounds: Grounds = { ground: out.ground, island: out.island, edge: out.edge };
  // What the editor's swatches show for the two parts that aren't solid until picked.
  out.titlebar = titlebar ?? out.ground;
  out.line = line ?? mix(out.ground, out.fg, (TINTS[look.base] ?? TINTS.dark).line / 100);
  return { colors: out, tokens, grounds };
}

/** The look's tokens as one rule, for the stylesheet the shell adopts (theme.ts applyLook). */
export const lookCss = (tokens: Record<string, string>) =>
  `@layer tokens{:root[data-look]{${Object.entries(tokens).map(([id, v]) => `--${id}:${v};`).join("")}}}`;

// ---- Themes that come with RoALT ----

export type Builtin = { id: string; label: Key; look: Look; accent: string; corners: number };
export const BUILTINS: readonly Builtin[] = [
  {
    id: "midnight", label: "look.theme.midnight", accent: "#7aa2ff", corners: 100,
    look: { base: "dark", colors: { ground: "#0b1020", "surface-1": "#121a33", "surface-2": "#1a2444", island: "#141d3a", float: "#172042", fg: "#e6ecff" } },
  },
  {
    id: "sakura", label: "look.theme.sakura", accent: "#ff8fb8", corners: 150,
    look: { base: "dark", colors: { ground: "#1a0f14", "surface-1": "#24151c", "surface-2": "#2e1b24", island: "#2a1820", float: "#2c1922", fg: "#ffe4ee" } },
  },
  {
    id: "terminal", label: "look.theme.terminal", accent: "#39ff6a", corners: 0,
    look: { base: "dark", colors: { ground: "#000000", "surface-1": "#050805", "surface-2": "#0b120b", island: "#070b07", float: "#081008", fg: "#39ff6a", edge: "#1f7a35" } },
  },
  {
    id: "ocean", label: "look.theme.ocean", accent: "#2ee6c5", corners: 100,
    look: { base: "dark", colors: { ground: "#04161a", "surface-1": "#082128", "surface-2": "#0d2c35", island: "#0a2730", float: "#0c2a33", fg: "#dff8ff" } },
  },
  {
    id: "paper", label: "look.theme.paper", accent: "#b5562b", corners: 50,
    look: { base: "light", colors: { ground: "#f4efe4", "surface-1": "#fbf8f1", "surface-2": "#ece5d6", island: "#fbf8f1", float: "#fffdf8", fg: "#2b2418" } },
  },
];

const sameColours = (a: Look, b: Look) => {
  const ka = Object.keys(a.colors);
  return a.base === b.base && ka.length === Object.keys(b.colors).length && ka.every((k) => a.colors[k] === b.colors[k]);
};
/** The built-in theme this look is, if any (an edit makes it the user's own). */
export const builtinOf = (look: Look) => BUILTINS.find((b) => sameColours(b.look, look)) ?? null;

// ---- Share codes ----

/** A theme as it travels in a share code. */
export type Shared = { v: 1; base: Base; colors: Record<string, string>; accent: string; corners: number };
export const CORNERS = [0, 50, 100, 150, 200] as const;

/** A shared theme checked as the backend would take it, with the ids this version knows; null when it
 * isn't one. */
export function readShared(json: string): Shared | null {
  try {
    const v = JSON.parse(json) as Partial<Shared>;
    if (v.v !== 1 || (v.base !== "dark" && v.base !== "light") || typeof v.colors !== "object" || !v.colors) return null;
    if (!(v.accent === "" || isColour(v.accent)) || !CORNERS.includes(v.corners as (typeof CORNERS)[number])) return null;
    const colors = Object.fromEntries(Object.entries(v.colors).filter(([id, c]) => KNOWN.has(id) && isColour(c)).map(([id, c]) => [id, (c as string).toLowerCase()]));
    return { v: 1, base: v.base, colors, accent: (v.accent as string).toLowerCase(), corners: v.corners as number };
  } catch {
    return null;
  }
}
