import { installSounds } from "./ui/sounds";
import { Component, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "./theme.css";
import App from "./App";
import { accentTokens, ACCENT_KEY, applyAccent, applyLook, GROUNDS, LOOK_KEY, ROUND_KEY, THEME_KEY } from "./app/theme";
import { debugLog } from "./api";
import { loadLastLang, t } from "./i18n";
import { LayoutPreview, type PreviewData } from "./features/launch/LayoutPreview";
import { assertMotionTokens, installReactive, installReducedMotion } from "./ui/motion";

// Everything that goes wrong in the UI reaches roalt.log (the Debug tab).
let forwarding = false;
function report(level: "error" | "warn", message: string) {
  // A failure while forwarding must not forward itself again.
  if (forwarding) return;
  forwarding = true;
  try {
    debugLog(level, message);
  } finally {
    forwarding = false;
  }
}
const describe = (e: unknown) => (e instanceof Error ? `${e.name}: ${e.message}${e.stack ? `\n${e.stack}` : ""}` : String(e));
window.addEventListener("error", (e) => report("error", `uncaught ${describe(e.error ?? e.message)} (${e.filename}:${e.lineno}:${e.colno})`));
window.addEventListener("unhandledrejection", (e) => report("error", `unhandled rejection ${describe(e.reason)}`));
for (const level of ["error", "warn"] as const) {
  const original = console[level].bind(console);
  console[level] = (...args: unknown[]) => {
    original(...args);
    report(level, `console.${level}: ${args.map(describe).join(" ")}`);
  };
}

/** A render error logs and shows a way back instead of a blank window. */
class Boundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  componentDidCatch(e: unknown, info: { componentStack?: string | null }) {
    report("error", `render failed: ${describe(e)}${info.componentStack ?? ""}`);
  }
  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <div className="flex-1 flex flex-col items-center justify-center gap-3 p-10 text-center">
        <p>{t("app.crashed")}</p>
        <button className="btn" onClick={() => this.setState({ failed: false })}>
          {t("app.tryAgain")}
        </button>
      </div>
    );
  }
}

// Before first paint: the last theme, look, corners and accent, so a light or coloured start doesn't flash.
try {
  const theme = localStorage.getItem(THEME_KEY) === "light" ? "light" : "dark";
  const root = document.documentElement;
  if (theme === "light") root.dataset.theme = theme;
  // Written by useTheme only (lookCss), never from outside: it becomes a stylesheet as it is.
  const look = localStorage.getItem(LOOK_KEY);
  if (look) applyLook(look);
  const round = Number(localStorage.getItem(ROUND_KEY));
  if (round >= 0 && round <= 2 && localStorage.getItem(ROUND_KEY)) root.style.setProperty("--round", String(round));
  const accent = localStorage.getItem(ACCENT_KEY);
  if (accent && /^#[0-9a-f]{6}$/i.test(accent)) {
    // A custom look's own grounds come with its accent once settings load; the base's do until then.
    applyAccent(accent, GROUNDS[theme]);
    // The splash is always dark: its cube takes the dark-theme shade.
    root.style.setProperty("--splash-accent", accentTokens(accent, GROUNDS.dark).text);
  }
} catch {
  // No storage: the defaults (dark, monochrome) apply until settings load.
}

// The window layout preview window (`layout_preview`): its spots, not the app.
const preview = (window as { __ROALT_LAYOUT_PREVIEW__?: PreviewData }).__ROALT_LAYOUT_PREVIEW__;
// Its window is see-through: the app's own background (and its fade-in) must never paint.
if (preview) {
  for (const el of [document.documentElement, document.body]) {
    el.style.setProperty("background", "transparent", "important");
    el.style.setProperty("transition", "none", "important");
  }
}

installReducedMotion();
installReactive();

// Plain-browser preview (`npm run dev` outside Tauri), and the website's demo (`npm run build:demo`).
// Dead code in production builds.
if (!preview && (import.meta.env.DEV || import.meta.env.MODE === "demo") && !("__TAURI_INTERNALS__" in window)) {
  await import("./dev/mockIpc");
}
if (import.meta.env.DEV) assertMotionTokens();

if (!preview) installSounds();

// Dev only (?gallery): every primitive in its states. Dead code in production builds.
const gallery = import.meta.env.DEV && new URLSearchParams(location.search).has("gallery") ? await import("./dev/GalleryRoot") : null;

await loadLastLang();

createRoot(document.getElementById("root")!).render(
  preview ? (
    <LayoutPreview data={preview} />
  ) : gallery ? (
    <gallery.GalleryRoot />
  ) : (
    <StrictMode>
      <Boundary>
        <App />
      </Boundary>
    </StrictMode>
  ),
);
