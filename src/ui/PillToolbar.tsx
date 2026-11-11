import { useEffect, useRef, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import { rovingKeys } from "./a11y";
import { Morph, SPRING, TWEEN, useReducedMotionFlag } from "./motion";

const ITEMS = "button:not(:disabled), [role=switch], input";
/** What shares the one Tab stop: not a text field (it keeps its own stop and its caret), nor what
 * sits in a menu or dialog opened from the toolbar. */
const ROVES = "button, [role=switch], input:is([type=checkbox], [type=radio])";
const stops = (bar: HTMLElement): Element[] =>
  [...bar.querySelectorAll<HTMLElement>(ROVES)].filter((el) => {
    const layer = el.closest("[popover], dialog");
    return !layer || layer.contains(bar);
  });
function stopOn(bar: HTMLElement, stop: Element) {
  for (const el of stops(bar) as HTMLElement[]) {
    const want = el === stop ? 0 : -1;
    if (el.tabIndex !== want) el.tabIndex = want;
  }
}
function repairStop(bar: HTMLElement) {
  const all = stops(bar) as HTMLElement[];
  const usable = all.filter((el) => !el.matches(":disabled"));
  const open = all.filter((el) => el.tabIndex === 0);
  if (open.length === 1 && usable.includes(open[0])) return;
  const focused = usable.find((el) => el === document.activeElement);
  const kept = usable.find((el) => open.includes(el));
  const to = focused ?? kept ?? usable[0];
  if (to) stopOn(bar, to);
}

/** A contextual toolbar in the island's material (the Accounts selection, Home's window tools,
 * Debug's controls). Its width springs as its contents change; arrows move between its controls.
 * `open` false: it folds away (sticky-bottom ones drop out of sight). */
export function PillToolbar({ label, open = true, placement = "inline", className = "", children }: {
  label: string;
  open?: boolean;
  placement?: "inline" | "sticky-bottom";
  className?: string;
  children: ReactNode;
}) {
  const reduced = useReducedMotionFlag();
  const bar = useRef<HTMLDivElement>(null);
  // One Tab stop (the control last focused), as the island has: arrows move inside. Kept as the
  // controls change (a selection swapping its set, a control disabling).
  useEffect(() => {
    const el = bar.current;
    if (!open || !el) return;
    repairStop(el);
    const mo = new MutationObserver(() => repairStop(el));
    mo.observe(el, { childList: true, subtree: true, attributes: true, attributeFilter: ["disabled"] });
    return () => mo.disconnect();
  }, [open]);
  return (
    <AnimatePresence initial={false}>
      {open && (
        <motion.div
          ref={bar}
          role="toolbar"
          aria-label={label}
          className={`pill-toolbar ${className}`}
          data-placement={placement}
          data-open
          initial={{ opacity: 0, y: reduced ? 0 : 12, scale: reduced ? 1 : 0.96 }}
          animate={{ opacity: 1, y: 0, scale: 1, transition: SPRING.smooth }}
          exit={{ opacity: 0, y: reduced ? 0 : 8, scale: reduced ? 1 : 0.98, transition: TWEEN.exit }}
          onKeyDown={(e) => rovingKeys(e, ITEMS)}
          onFocus={(e) => stops(e.currentTarget).includes(e.target) && stopOn(e.currentTarget, e.target)}
        >
          <Morph axis="width" innerClassName="gap-0.5">
            {children}
          </Morph>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

export const PillSeparator = () => <span aria-hidden="true" className="pill-sep" />;
export const PillLabel = ({ children }: { children: ReactNode }) => <span className="pill-label">{children}</span>;
