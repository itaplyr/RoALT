import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import { DUR, EASE, Morph, SPRING, TWEEN, useReducedMotionFlag } from "../motion";

const FOCUSABLE = ":is(button, input, select, textarea, a[href], [tabindex]:not([tabindex='-1'])):not(:disabled)";
import { useAnchor } from "../Popover";

/** A live activity growing out of the island (a launch, running clients, a notice). It opens its
 * sheet below; the sheet's keys and focus are the popover's own, and focus comes back here when it
 * closes. `show` false: it shrinks back into the island. */
export function Capsule({ show, label, layoutId, sheetLabel, sheet, children }: {
  show: boolean;
  /** The capsule's accessible name (its contents are visual). */
  label: string;
  /** For a shared element that flies in (the launch: `live:launch`). */
  layoutId?: string;
  sheetLabel: string;
  sheet: (close: () => void) => ReactNode;
  children: ReactNode;
}) {
  const anchor = useAnchor();
  const [open, setOpen] = useState(false);
  // While its button flies in from elsewhere (a layout handoff), the clip that lets it grow out of
  // the island would hide the flight. Not while leaving: an exiting child keeps its old callbacks.
  const [flying, setFlying] = useState(false);
  const showing = useRef(show);
  showing.current = show;
  const reduced = useReducedMotionFlag();
  const id = anchor.name.slice(2);
  // The sheet is a dialog: opening it takes the focus in (closing gives it back, natively).
  useEffect(() => {
    if (!open) return;
    const raf = requestAnimationFrame(() => {
      const sheet = document.getElementById(id);
      if (!sheet || sheet.contains(document.activeElement)) return;
      const to = sheet.querySelector<HTMLElement>("[data-autofocus]") ?? sheet.querySelector<HTMLElement>(FOCUSABLE);
      if (!to) sheet.tabIndex = -1;
      (to ?? sheet).focus();
    });
    return () => cancelAnimationFrame(raf);
  }, [open, id]);
  return (
    <AnimatePresence initial={false}>
      {show && (
        <motion.span
          key="capsule"
          className="inline-flex"
          style={{ overflow: flying ? "visible" : "clip", overflowClipMargin: 6 }}
          initial={{ width: 0, opacity: 0 }}
          animate={{ width: "auto", opacity: 1, transition: { width: reduced ? { duration: 0 } : SPRING.size, opacity: TWEEN.fade } }}
          // Leaving is shorter than arriving: a short tween, a little past the fade.
          exit={{ width: 0, opacity: 0, transition: { width: reduced ? { duration: 0 } : { duration: DUR.exit / 1000 + 0.06, ease: EASE.in }, opacity: TWEEN.exit } }}
        >
          <span className="island-sep" aria-hidden="true" />
          <motion.button
            type="button"
            layoutId={layoutId}
            // Measured only when it mounts (the fly-in), never as its text changes: a layout
            // animation would scale the button, squashing its text and ends at every step. Unique
            // per capsule: motion skips a handoff between two elements with the same dependency.
            layoutDependency={anchor.name}
            onLayoutAnimationStart={() => showing.current && setFlying(true)}
            onLayoutAnimationComplete={() => setFlying(false)}
            // The fly-in travels on the gentle spring (the only layout animation it runs).
            transition={{ layout: SPRING.gentle }}
            className="island-capsule"
            data-rove
            tabIndex={-1}
            aria-label={label}
            aria-haspopup="dialog"
            aria-expanded={open}
            popoverTarget={id}
            style={anchor.style}
          >
            {/* Its words change as it goes (a count, a step): the width springs, so the centred
                island glides rather than jumps. */}
            <Morph axis="width" innerClassName="gap-2">
              {children}
            </Morph>
          </motion.button>
          <div
            id={id}
            popover="auto"
            role="dialog"
            aria-label={sheetLabel}
            className="island-sheet"
            data-tauri-drag-region="false"
            style={{ positionAnchor: anchor.name } as CSSProperties}
            onToggle={(e) => setOpen((e as unknown as ToggleEvent).newState === "open")}
          >
            {open && sheet(() => document.getElementById(id)?.hidePopover())}
          </div>
        </motion.span>
      )}
    </AnimatePresence>
  );
}

/** A progress ring for a capsule (a batch's share done). */
export function Ring({ value, size = 16 }: { value: number; size?: number }) {
  const r = (size - 3) / 2;
  const c = 2 * Math.PI * r;
  return (
    <svg className="capsule-ring" width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden="true">
      <circle className="ring-track" cx={size / 2} cy={size / 2} r={r} />
      <motion.circle className="ring-value" cx={size / 2} cy={size / 2} r={r} strokeDasharray={c} initial={false} animate={{ strokeDashoffset: c * (1 - Math.max(0, Math.min(1, value))) }} transition={SPRING.size} />
    </svg>
  );
}
