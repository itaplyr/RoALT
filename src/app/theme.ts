import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, type Settings } from "../api";
import { qk } from "../data";
import { prefersReducedMotion } from "../ui/motion";

/** Last applied theme, read before first paint so a light-theme start does not flash dark. */
export const THEME_KEY = "roalt.theme";
/** The user's accent, read before first paint (and by the splash). */
export const ACCENT_KEY = "roalt.accent";

/** Surfaces that turn see-through over a background image, and how much of them stays. Panels and
 * what floats keep most, so their text stays readable over any picture. */
const SEE_THROUGH: Record<string, number> = { "--ground": 50, "--surface-1": 72, "--surface-2": 84, "--surface-3": 88, "--island": 86, "--float": 92 };

/** The grounds an accent is checked against: the CSS values (tokens.css), per theme. */
const GROUND = { dark: "#0a0a0a", light: "#f3f3f3" } as const;
const ISLAND = { dark: "#141414", light: "#ffffff" } as const;
const EDGE = { dark: "#6e6e6e", light: "#7c7c7c" } as const;

const channel = (v: number) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
const rgb = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
const luminance = (hex: string) => {
  const [r, g, b] = rgb(hex).map((v) => channel(v / 255));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
export const contrast = (a: string, b: string) => {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};
const mix = (hex: string, toward: string, share: number) => {
  const [a, b] = [rgb(hex), rgb(toward)];
  return `#${a.map((v, i) => Math.round(v + (b[i] - v) * share).toString(16).padStart(2, "0")).join("")}`;
};

/** What a user accent becomes in one theme: the fill as chosen, the ink on it (whichever of near
 * black and white contrasts more), a text shade moved toward white on dark (black on light) until it
 * reads at 4.5:1 on the ground, and an edge when the fill alone is under 3:1 against the island. */
export function accentTokens(hex: string, theme: "dark" | "light") {
  const ink = contrast(hex, "#0a0a0a") >= contrast(hex, "#ffffff") ? "#0a0a0a" : "#ffffff";
  let text = hex;
  for (let s = 0.05; contrast(text, GROUND[theme]) < 4.5 && s <= 1; s += 0.05) text = mix(hex, theme === "dark" ? "#ffffff" : "#000000", s);
  const ring = contrast(hex, ISLAND[theme]) < 3 ? EDGE[theme] : "transparent";
  return { fill: hex, ink, text, ring };
}

/** Applies the theme to <html>: `data-theme` (dark or light, "system" follows Windows live), the
 * accent (`data-accent`), and the background image. A theme change cross-fades the whole window in
 * one snapshot. Returns whether it is dark. */
export function useTheme(s: Settings | undefined) {
  const [dark, setDark] = useState(() => document.documentElement.dataset.theme !== "light");
  const theme = s?.theme;
  const accent = s?.accent ?? "";
  // Less than this over a bright picture and no text stays readable.
  const dim = Math.max(40, s?.backgroundDim ?? 60);
  const bytes = useQuery({ queryKey: qk.background, queryFn: api.backgroundGet, staleTime: Infinity, enabled: !!s }).data;
  const [image, setImage] = useState<string | null>(null);
  const applied = useRef<string | null>(null);

  useEffect(() => {
    if (!bytes?.byteLength) return setImage(null);
    const url = URL.createObjectURL(new Blob([bytes]));
    setImage(url);
    return () => URL.revokeObjectURL(url);
  }, [bytes]);

  useEffect(() => {
    if (!theme) return;
    const root = document.documentElement;
    const mq = matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const t = theme === "system" ? (mq.matches ? "dark" : "light") : theme;
      root.dataset.theme = t;
      setDark(t !== "light");
      if (accent) {
        const a = accentTokens(accent, t);
        root.dataset.accent = "";
        root.style.setProperty("--user-accent", a.fill);
        root.style.setProperty("--user-on-accent", a.ink);
        root.style.setProperty("--user-accent-text", a.text);
        root.style.setProperty("--user-accent-ring", a.ring);
      } else {
        delete root.dataset.accent;
      }
      // The window's colour covers the picture by `dim`; surfaces on top are partly see-through.
      for (const v of Object.keys(SEE_THROUGH)) root.style.removeProperty(v);
      if (image) {
        const css = getComputedStyle(root);
        root.style.setProperty("--bg-dim", `color-mix(in srgb, ${css.getPropertyValue("--ground").trim()} ${dim}%, transparent)`);
        for (const [v, keep] of Object.entries(SEE_THROUGH)) root.style.setProperty(v, `color-mix(in srgb, ${css.getPropertyValue(v).trim()} ${keep}%, transparent)`);
        root.style.setProperty("--bg-image", `url("${image}")`);
        root.dataset.bg = "";
      } else {
        delete root.dataset.bg;
      }
      try {
        localStorage.setItem(THEME_KEY, t);
        if (accent) localStorage.setItem(ACCENT_KEY, accent);
        else localStorage.removeItem(ACCENT_KEY);
      } catch {
        // Storage can be unavailable; the theme still applies.
      }
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
  }, [theme, accent, image, dim]);
  return dark;
}

/** One snapshot of the window cross-fades to the new theme; nothing transitions on its own meanwhile
 * (a long list would freeze the window). Without the cross-fade (reduced motion), the switch is
 * instant all the same: thousands of rows each easing their colours would hang the window. */
function crossFade(apply: () => void) {
  const root = document.documentElement;
  const settle = () => requestAnimationFrame(() => requestAnimationFrame(() => delete root.dataset.instant));
  root.dataset.instant = "";
  if (prefersReducedMotion() || !("startViewTransition" in document)) {
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
