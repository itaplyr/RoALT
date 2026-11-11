import { memo, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import { t } from "../../i18n";
import { Icon } from "../../ui/Icon";
import { RISE, SPRING, TWEEN, useReducedMotionFlag } from "../../ui/motion";
import type { Line } from "./useLogStream";

/** Scrolled down to within this many pixels of the bottom, the log follows new lines again. */
const NEAR = 24;
/** Lines go in runs of this many ids, so a new line re-renders its run and not all 5000. */
const RUN = 100;

/** One line, its filter matches marked. */
const LogLine = memo(function LogLine({ line, find }: { line: Line; find: RegExp | null }) {
  return (
    <div className="dbg-line" data-level={line.level}>
      {find ? line.text.split(find).map((part, i) => (i % 2 ? <mark key={i}>{part}</mark> : part)) : line.text}
    </div>
  );
});

// Lines never change and ids only grow: same ends and length means the same lines.
const LogRun = memo(
  function LogRun({ lines, find }: { lines: Line[]; find: RegExp | null }) {
    return lines.map((l) => <LogLine key={l.id} line={l} find={find} />);
  },
  (a, b) => a.find === b.find && a.lines.length === b.lines.length && a.lines[0] === b.lines[0] && a.lines.at(-1) === b.lines.at(-1),
);

/** The lines, terminal-style. At the bottom it follows what comes in; scrolled up it holds still and a
 * pill counts the lines below. Lines never animate (there can be 5000); off-screen ones skip
 * rendering (content-visibility) and keep their place, selection and find-in-page. */
export function LogView({ lines, find, label, children }: {
  lines: Line[];
  /** The filter, as one capture group: what to mark. */
  find: RegExp | null;
  label: string;
  /** Shown instead of the lines: loading, nothing to show. */
  children?: ReactNode;
}) {
  const box = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  const lastTop = useRef(0);
  // The newest line there was when the view left the bottom; null while following.
  const [seen, setSeen] = useState<number | null>(null);
  const reduced = useReducedMotionFlag();

  // Following (or with nothing to scroll), the view stays at the bottom: when lines come in, and when
  // lines drawn for the first time turn out taller than their estimate, or the window resizes.
  const stick = () => {
    const el = box.current;
    if (!el || (!follow.current && el.scrollHeight - el.clientHeight > NEAR)) return;
    follow.current = true;
    setSeen(null);
    el.scrollTop = el.scrollHeight;
    // Else the next scroll event measures from before this jump, and a scroll up reads as one down.
    lastTop.current = el.scrollTop;
  };
  useLayoutEffect(stick, [lines]);
  useLayoutEffect(() => {
    const ro = new ResizeObserver(() => stick());
    ro.observe(box.current!);
    ro.observe(box.current!.firstElementChild!);
    return () => ro.disconnect();
  }, []);

  const onScroll = () => {
    const el = box.current!;
    const top = el.scrollTop;
    const dist = el.scrollHeight - top - el.clientHeight;
    // Near the bottom it follows again only on the way down: a scroll up leaves at once, even while
    // lines stream in faster than it moves.
    if (dist <= 1 || (dist <= NEAR && top >= lastTop.current)) {
      follow.current = true;
      setSeen(null);
    } else if (top < lastTop.current && follow.current) {
      follow.current = false;
      setSeen(lines.at(-1)?.id ?? -1);
    }
    lastTop.current = top;
  };
  const jump = () => {
    follow.current = true;
    setSeen(null);
    // The pill leaves: the focus it had goes to the log, not back to the top of the page.
    box.current?.focus({ preventScroll: true });
    box.current?.scrollTo({ top: box.current.scrollHeight, behavior: reduced ? "auto" : "smooth" });
  };

  let unseen = 0;
  if (seen !== null) for (let i = lines.length - 1; i >= 0 && lines[i].id > seen; i--) unseen++;
  const runs: Line[][] = [];
  for (const l of lines) {
    const run = runs.at(-1);
    if (run && Math.floor(run[0].id / RUN) === Math.floor(l.id / RUN)) run.push(l);
    else runs.push([l]);
  }

  return (
    <div className="panel dbg-log">
      <div ref={box} role="log" aria-live="off" aria-label={label} tabIndex={0} dir="ltr" className="dbg-scroll selectable mono" onScroll={onScroll}>
        <div>{children ?? runs.map((run) => <LogRun key={Math.floor(run[0].id / RUN)} lines={run} find={find} />)}</div>
      </div>
      <AnimatePresence>
        {unseen > 0 && (
          <motion.button
            key="new"
            type="button"
            className="dbg-new"
            onClick={jump}
            initial={{ opacity: 0, y: RISE.md, scale: 0.96 }}
            animate={{ opacity: 1, y: 0, scale: 1, transition: SPRING.snappy }}
            exit={{ opacity: 0, y: RISE.sm, transition: TWEEN.exit }}
          >
            {t("debug.newLines", { count: unseen })}
            <Icon name="chevron" size={14} />
          </motion.button>
        )}
      </AnimatePresence>
    </div>
  );
}
