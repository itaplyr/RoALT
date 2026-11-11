import { useEffect, useId, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import { AnimatePresence, animate, motion, type AnimationPlaybackControls } from "motion/react";
import { announce, overlayOpen } from "../../ui/a11y";
import { Kbd } from "../../ui/Bits";
import { Icon, type IconName } from "../../ui/Icon";
import { Collapse, LAYOUT_IDS, SPRING, Swap, TWEEN, useReducedMotionFlag } from "../../ui/motion";
import { holdIsland, launchActivityId, launchStep, useLaunchActivityId, useLaunchStep } from "./launchQueue";
import "./launch.css";

/** The pill stays where the button was this long before it flies: its morph, and a moment to read. */
const HOLD_MS = 560;

// The pill in flight. It lives outside the button, which its view often swaps out as the launch starts.
type Flight = { key: number; rect: DOMRect; text: string; gone: boolean };
let flight: Flight | null = null;
let hosts = 0;
let flights = 0;
const flightSubs = new Set<() => void>();
function setFlight(f: Flight | null) {
  flight = f;
  flightSubs.forEach((s) => s());
}
const subscribeFlight = (f: () => void) => {
  flightSubs.add(f);
  return () => void flightSubs.delete(f);
};

/** Hands the pill to LaunchFlight; false when none is mounted. */
function fly(rect: DOMRect, text: string) {
  if (!hosts) return false;
  setFlight({ key: ++flights, rect, text, gone: false });
  return true;
}

/** Where LaunchButton's pill takes off into the island's launch capsule (the same layoutId). Mount it
 * once, outside the views (Shell), so a flight outlives the button that started it. */
export function LaunchFlight() {
  const f = useSyncExternalStore(subscribeFlight, () => flight);
  useEffect(() => {
    hosts++;
    return () => void hosts--;
  }, []);
  // One frame in the layoutId's stack before the capsule mounts, so the capsule flies from it.
  useEffect(() => {
    if (!f || f.gone) return;
    const frame = requestAnimationFrame(() => {
      setFlight({ ...f, gone: true });
      holdIsland(false);
    });
    return () => cancelAnimationFrame(frame);
  }, [f]);
  return createPortal(
    <AnimatePresence onExitComplete={() => flight?.gone && setFlight(null)}>
      {f && !f.gone && (
        <div key={f.key} className="launch-flight" style={{ left: f.rect.left, top: f.rect.top, width: f.rect.width, height: f.rect.height }}>
          <motion.span layoutId={LAYOUT_IDS.launchLive} className="launch-pill" style={{ borderRadius: 999 }} exit={{ opacity: 0, transition: TWEEN.exit }}>
            <motion.span layout="position" className="launch-face">
              <Icon name="refresh" size={14} className="spin" />
              <span className="truncate">{f.text}</span>
            </motion.span>
          </motion.span>
        </div>
      )}
    </AnimatePresence>,
    document.body,
  );
}

/** The primary launch control (Play, Servers' Join, Home's "Play with …"). Pressed, it morphs into a
 * pill saying what the launch is doing, then the pill flies into the island (LaunchFlight) as the
 * launch shows there; reduced motion: it cross-fades back in place. `onLaunch` starts the launch
 * (launchOne, launchSame or launchEach). Disabled (or while another launch starts) it stays focusable
 * and `reason` shows under it; pressing it says the reason again. `noted`: the reason when a notice
 * beside it already shows it (no line under it, but pressing it says it). `shortcut`: Ctrl+Enter
 * presses it (one per screen; not while a dialog or menu is open). `big`: Play's own, full width. */
export function LaunchButton({ label, onLaunch, disabled = false, reason, noted, big, shortcut, icon = "play", className = "" }: {
  label: string;
  onLaunch: () => void;
  disabled?: boolean;
  reason?: string | null;
  noted?: string | null;
  big?: boolean;
  shortcut?: boolean;
  icon?: IconName;
  className?: string;
}) {
  const step = useLaunchStep();
  const shownId = useLaunchActivityId();
  const reduced = useReducedMotionFlag();
  const btn = useRef<HTMLButtonElement>(null);
  const reasonId = useId();
  // This button's own launch, from its press until the island has it.
  const [pill, setPill] = useState<{ before: string | null; at: number } | null>(null);
  const holding = useRef(false);
  const said = useRef({ step: "", why: "" });
  if (step) said.current.step = step;
  const [nudge, setNudge] = useState(0);
  const blocked = disabled || (step !== null && !pill);
  const why = pill ? null : step ?? (disabled ? (reason ?? null) : null);
  if (why) said.current.why = why;

  const release = () => {
    if (!holding.current) return;
    holding.current = false;
    holdIsland(false);
  };
  const press = () => {
    if (pill) return;
    if (blocked) {
      announce(why ?? (disabled && noted ? noted : label), "assertive");
      setNudge((x) => x + 1);
      return;
    }
    const before = launchActivityId();
    onLaunch();
    if (launchStep() === null) return;
    holding.current = true;
    holdIsland(true);
    setPill({ before, at: performance.now() });
  };
  const pressed = useRef(press);
  pressed.current = press;

  // The launch is on show: after the morph, the pill takes off. It ended without one: back in place.
  useEffect(() => {
    if (!pill) return;
    if (shownId !== null && shownId !== pill.before) {
      const timer = setTimeout(() => {
        const r = btn.current?.getBoundingClientRect();
        holding.current = false;
        setPill(null);
        if (reduced || !r || !fly(r, said.current.step)) holdIsland(false);
      }, Math.max(0, pill.at + HOLD_MS - performance.now()));
      return () => clearTimeout(timer);
    }
    if (step === null) {
      setPill(null);
      release();
    }
  }, [pill, shownId, step, reduced]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => release, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!shortcut) return;
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key === "Enter" && !overlayOpen()) {
        e.preventDefault();
        pressed.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [shortcut]);

  // Its size springs from the old face's to the new one's; at rest CSS sizes it (full width when big).
  const face = pill ? `pill:${said.current.step}` : `idle:${label}`;
  const size = useRef<{ w: number; h: number } | null>(null);
  const run = useRef<AnimationPlaybackControls | null>(null);
  useLayoutEffect(() => {
    const el = btn.current!;
    const ro = new ResizeObserver(() => (size.current = { w: el.offsetWidth, h: el.offsetHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  useLayoutEffect(() => {
    const el = btn.current;
    const from = size.current;
    if (!el || !from) return;
    run.current?.stop();
    run.current = null;
    el.style.width = el.style.height = "";
    if (reduced) return;
    const to = { w: el.offsetWidth, h: el.offsetHeight };
    if (Math.abs(to.w - from.w) < 1 && Math.abs(to.h - from.h) < 1) return;
    el.style.width = `${from.w}px`;
    el.style.height = `${from.h}px`;
    const r = animate(el, { width: [`${from.w}px`, `${to.w}px`], height: [`${from.h}px`, `${to.h}px`] }, SPRING.smooth);
    run.current = r;
    void r.then(() => {
      if (run.current !== r) return;
      run.current = null;
      el.style.width = el.style.height = "";
    });
  }, [face]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className={`launch-slot ${big ? "launch-slot-big" : ""} ${nudge ? (nudge % 2 ? "shake" : "shake2") : ""} ${className}`}>
      <button
        ref={btn}
        type="button"
        className={pill ? "launch-pill launch-btn" : `btn btn-primary launch-btn ${big ? "btn-play" : ""}`}
        aria-disabled={blocked || !!pill || undefined}
        aria-busy={!!pill || undefined}
        aria-describedby={why ? reasonId : undefined}
        aria-keyshortcuts={shortcut ? "Control+Enter" : undefined}
        onClick={press}
      >
        <span className="launch-face">
          <Swap k={pill ? "pill" : "idle"} variant="blur" className="min-w-0 max-w-full">
            {pill ? (
              <>
                <Icon name="refresh" size={14} className="spin" />
                <span className="truncate">{said.current.step}</span>
              </>
            ) : (
              <>
                <Icon name={icon} size={big ? 20 : 14} fill />
                <span className="truncate">{label}</span>
                {shortcut && (
                  <span aria-hidden="true" className="launch-kbd">
                    <Kbd keys="Ctrl+Enter" />
                  </span>
                )}
              </>
            )}
          </Swap>
        </span>
      </button>
      <div aria-live="polite" className="launch-reason-wrap">
        {pill && <span className="sr-only">{said.current.step}</span>}
        <Collapse open={!!why}>
          <p id={reasonId} className="launch-reason">{why ?? said.current.why}</p>
        </Collapse>
      </div>
    </div>
  );
}
