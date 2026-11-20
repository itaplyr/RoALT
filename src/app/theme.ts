import { useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Settings } from "../api";
import { useBackgroundStill, useBackgroundUrl } from "../data";
import { prefersReducedMotion, useMotionOff } from "../ui/motion";
import { writeStored } from "../ui/stored";
import { accentTokens, GROUNDS, lookCss, lookTokens, type Grounds } from "./look";

export { accentTokens, contrast, GROUNDS } from "./look";

/** Last applied theme, read before first paint so a light-theme start does not flash dark. */
export const THEME_KEY = "roalt.theme";
/** The user's accent, read before first paint (and by the splash). */
export const ACCENT_KEY = "roalt.accent";
/** A custom look's rule (lookCss), read before first paint. */
export const LOOK_KEY = "roalt.look";
/** Corner roundness as a factor ("1.5"), read before first paint. */
export const ROUND_KEY = "roalt.round";

/** Surfaces that turn see-through over a background image, and how much of them stays. Panels and
 * what floats keep most, so their text stays readable over any picture. */
const SEE_THROUGH: Record<string, number> = { "--ground": 50, "--surface-1": 72, "--surface-2": 84, "--surface-3": 88, "--island": 86, "--titlebar": 86, "--float": 92 };

/** Puts a user accent on <html>, checked on these grounds: `data-accent` and the --user-accent* properties. */
export function applyAccent(hex: string, g: Grounds) {
  const root = document.documentElement;
  const a = accentTokens(hex, g);
  root.dataset.accent = "";
  root.style.setProperty("--user-accent", a.fill);
  root.style.setProperty("--user-on-accent", a.ink);
  root.style.setProperty("--user-accent-text", a.text);
  root.style.setProperty("--user-accent-ring", a.ring);
}

// A constructed sheet, not a <style> tag: the built app's CSP carries hashes, so 'unsafe-inline'
// is ignored and a <style> made at runtime would be blocked. CSSOM isn't.
let lookSheet: CSSStyleSheet | null = null;
let lookText: string | null = null;

/** Puts a custom look's rule on the page (`css`), or takes it off (null); `data-look` turns it on. */
export function applyLook(css: string | null) {
  const root = document.documentElement;
  if (css === null) {
    if (lookSheet) document.adoptedStyleSheets = document.adoptedStyleSheets.filter((s) => s !== lookSheet);
    delete root.dataset.look;
    return;
  }
  lookSheet ??= new CSSStyleSheet();
  if (lookText !== css) {
    lookSheet.replaceSync(css);
    lookText = css;
  }
  if (!document.adoptedStyleSheets.includes(lookSheet)) document.adoptedStyleSheets = [...document.adoptedStyleSheets, lookSheet];
  root.dataset.look = "";
}

/** Whether RoALT's window has the focus, kept current. The window's, not the page's: the focus moving
 * into Browse's roblox.com pane (a webview of its own) leaves the window in front. */
function useFocused() {
  const [focused, setFocused] = useState(() => document.hasFocus());
  useEffect(() => {
    const un = getCurrentWindow().onFocusChanged(({ payload }) => setFocused(payload));
    return () => void un.then((f) => f());
  }, []);
  return focused;
}

/** Applies the theme to <html>: `data-theme` (dark or light, "system" follows Windows live; a custom
 * look sits on its base), the look (`data-look`), the accent (`data-accent`), the corners and the
 * background image, still while it shouldn't move. A theme change cross-fades the whole window in one
 * snapshot. Returns whether it is dark. */
export function useTheme(s: Settings | undefined) {
  const [dark, setDark] = useState(() => document.documentElement.dataset.theme !== "light");
  const theme = s?.theme;
  const accent = s?.accent ?? "";
  const look = s?.look;
  const base = look?.base ?? "dark";
  // The settings query keeps the same object until the look changes.
  const custom = useMemo(() => (theme === "custom" && look ? lookTokens(look) : null), [theme, look]);
  const css = custom && lookCss(custom.tokens);
  // Monochrome on a custom look is its own text colour, checked like any accent.
  const fill = accent || custom?.colors.fg || "";
  const round = String((s?.corners ?? 100) / 100);
  // Less than this over a bright picture and no text stays readable.
  const dim = Math.max(40, s?.backgroundDim ?? 60);
  const moving = useBackgroundUrl(!!s);
  const still = useBackgroundStill(!!s);
  const motion = s?.backgroundMotion ?? "focused";
  const focused = useFocused();
  const motionOff = useMotionOff();
  // A picture that may move shows its still frame unless it may move now.
  const plays = !!still && !motionOff && (motion === "always" || (motion === "focused" && focused));
  const image = still && !plays ? still : moving;
  const applied = useRef<string | null>(null);

  useEffect(() => {
    if (!theme) return;
    const root = document.documentElement;
    const mq = matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const t = theme === "custom" ? base : theme === "system" ? (mq.matches ? "dark" : "light") : theme;
      root.dataset.theme = t;
      setDark(t !== "light");
      applyLook(css);
      const grounds = custom?.grounds ?? GROUNDS[t];
      if (fill) applyAccent(fill, grounds);
      else delete root.dataset.accent;
      root.style.setProperty("--round", round);
      // The window's colour covers the picture by `dim`; surfaces on top are partly see-through.
      for (const v of Object.keys(SEE_THROUGH)) root.style.removeProperty(v);
      if (image) {
        const style = getComputedStyle(root);
        root.style.setProperty("--bg-dim", `color-mix(in srgb, ${style.getPropertyValue("--ground").trim()} ${dim}%, transparent)`);
        for (const [v, keep] of Object.entries(SEE_THROUGH)) root.style.setProperty(v, `color-mix(in srgb, ${style.getPropertyValue(v).trim()} ${keep}%, transparent)`);
        root.style.setProperty("--bg-image", `url("${image}")`);
        root.dataset.bg = "";
      } else {
        delete root.dataset.bg;
      }
      // Moving: nothing blurs over it (a blur redraws with every frame).
      if (image && plays) root.dataset.bgLive = "";
      else delete root.dataset.bgLive;
      writeStored(THEME_KEY, t);
      writeStored(ACCENT_KEY, fill || null);
      writeStored(LOOK_KEY, css);
      writeStored(ROUND_KEY, round === "1" ? null : round);
    };
    // Only a real switch (not the first apply, not a background change) cross-fades.
    const key = `${theme}|${mq.matches}`;
    const switching = applied.current !== null && applied.current.split("|")[0] !== theme;
    applied.current = key;
    if (switching) crossFade(apply);
    else apply();
    // Windows' own light/dark matters only while the theme follows it.
    if (theme !== "system") return;
    const onSystem = () => crossFade(apply);
    mq.addEventListener("change", onSystem);
    return () => mq.removeEventListener("change", onSystem);
  }, [theme, base, custom, css, fill, round, image, plays, dim]);
  return dark;
}

/** One snapshot of the window cross-fades to the new theme; nothing transitions on its own meanwhile
 * (a long list would freeze the window). Without the cross-fade (reduced motion), the switch is
 * instant all the same: thousands of rows each easing their colours would hang the window. */
function crossFade(apply: () => void) {
  const root = document.documentElement;
  const settle = () => requestAnimationFrame(() => requestAnimationFrame(() => delete root.dataset.instant));
  root.dataset.instant = "";
  if (
    prefersReducedMotion() ||
    !("startViewTransition" in document) ||
    (window as { __ROALT_VIEW_TRANSITIONS?: boolean }).__ROALT_VIEW_TRANSITIONS === false
  ) {
    apply();
    return settle();
  }
  root.dataset.vt = "theme";
  const vt = document.startViewTransition(apply);
  vt.finished.finally(() => {
    delete root.dataset.vt;
    settle();
  });
}
