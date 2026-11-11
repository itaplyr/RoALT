import { useEffect, useState } from "react";
import { api } from "../../api";
import { setSplashPhase, type SplashPhase } from "./phase";
import "./splash.css";

/** When the window grows: the animation settles at 3 s (79% of 3.8 s), then holds 2 s. */
const GROW_MS = 3000 + 2000;
/** Set once the splash has run: a webview reload doesn't play it again. */
const KEY = "roalt.splash";

/** The RoALT tile (`Logo` in ui/Icon), with its hole as an evenodd cut-out. */
const TILE =
  "M8.5 7.5h6a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-6a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1z M10.7 10.3h1.6a.4.4 0 0 1 .4.4v1.6a.4.4 0 0 1-.4.4h-1.6a.4.4 0 0 1-.4-.4v-1.6a.4.4 0 0 1 .4-.4z";

function Tile({ className, fill }: { className: string; fill: string }) {
  return (
    <svg className={className} width="120" height="120" viewBox="0 0 18 18" aria-hidden="true">
      <path transform="rotate(15 11.5 11.5)" fillRule="evenodd" fill={fill} d={TILE} />
    </svg>
  );
}

function seen() {
  // A dev preview with ?splash=1 plays it again.
  if (import.meta.env.DEV && new URLSearchParams(location.search).has("splash")) return false;
  // Settings.skip_splash, from `main_window`: the window is already full size.
  if ((window as { __ROALT_SKIP_SPLASH?: boolean }).__ROALT_SKIP_SPLASH) return true;
  try {
    return sessionStorage.getItem(KEY) !== null;
  } catch {
    return false;
  }
}

// Decided before anything renders, so the island knows to wait for it.
const FIRST: SplashPhase = seen() ? "gone" : "play";
setSplashPhase(FIRST);

/** The start-up splash over the app: the tile glitches into the RoALT mark, then the window
 * (opened small by `splash::start`) grows into the app and this fades away. */
export function Splash() {
  const [state, setState] = useState<SplashPhase>(FIRST);

  useEffect(() => setSplashPhase(state), [state]);

  useEffect(() => {
    if (state !== "play") return;
    // Plays even with Windows' animation effects off (theme.css leaves the splash out).
    const grow = () => {
      try {
        sessionStorage.setItem(KEY, "1");
      } catch {
        // No storage: a reload plays it again.
      }
      // Fades once the window has grown, even if growing failed.
      api.splashDone().finally(() => setState("out"));
    };
    const t = setTimeout(grow, GROW_MS);
    return () => clearTimeout(t);
  }, [state]);

  if (state === "gone") return null;
  return (
    <div
      className={`splash ${state === "out" ? "out" : ""}`}
      data-tauri-drag-region
      onTransitionEnd={(e) => e.target === e.currentTarget && setState("gone")}
      aria-hidden="true"
    >
      <div className="mark" data-tauri-drag-region>
        <div className="logo">
          <div className="cube" />
          <div className="tile">
            <Tile className="red" fill="#ff2a4d" />
            <Tile className="cyan" fill="#22e8ff" />
            <Tile className="core" fill="#949494" />
            <Tile className="slice-a" fill="#ededed" />
            <Tile className="slice-b" fill="#22e8ff" />
          </div>
        </div>
      </div>
      <div className="mark">
        <div className="word">
          <b>ROALT</b>
          <span>v{import.meta.env.APP_VERSION}</span>
        </div>
      </div>
      <div className="scan" />
    </div>
  );
}
