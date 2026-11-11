import { useEffect, useRef } from "react";
import { animate, useMotionValue, useMotionValueEvent } from "motion/react";
import { useReducedMotionFlag } from "./reduced";
import { TWEEN } from "./tokens";

/** A number that eases between samples (CPU %, memory, a batch count). It writes its text straight
 * to the DOM on each frame: no React render per frame, so 30 of them on Home cost nothing. From
 * nothing (null) and under reduced motion it jumps. Tabular figures, so digits don't jiggle. */
export function Num({ value, format = (n) => String(Math.round(n)), empty = "—", className }: {
  value: number | null | undefined;
  format?: (n: number) => string;
  empty?: string;
  className?: string;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const mv = useMotionValue(value ?? 0);
  const prev = useRef<number | null>(value ?? null);
  const fmt = useRef(format);
  fmt.current = format;
  const reduced = useReducedMotionFlag();
  // React renders the first text only; from then on the motion value owns it (a re-render with the
  // final value would otherwise flash it before the tween).
  const first = useRef(value == null ? empty : format(value));

  useMotionValueEvent(mv, "change", (v) => {
    if (ref.current && prev.current !== null) ref.current.textContent = fmt.current(v);
  });

  useEffect(() => {
    const el = ref.current;
    if (value == null) {
      prev.current = null;
      mv.stop();
      if (el) el.textContent = empty;
      return;
    }
    if (prev.current === null || reduced) {
      mv.stop();
      mv.set(value);
      prev.current = value;
      if (el) el.textContent = fmt.current(value);
      return;
    }
    prev.current = value;
    const controls = animate(mv, value, TWEEN.number);
    return () => controls.stop();
  }, [value, reduced, mv, empty]);

  return (
    <span ref={ref} className={className} style={{ fontVariantNumeric: "tabular-nums" }}>
      {first.current}
    </span>
  );
}
