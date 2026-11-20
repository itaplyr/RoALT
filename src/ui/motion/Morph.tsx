import { useLayoutEffect, useRef, type CSSProperties, type ReactNode } from "react";
import { AnimatePresence, animate, motion, useMotionValue, type Transition } from "motion/react";
import { useReducedMotionFlag } from "./reduced";
import { SPRING, TWEEN } from "./tokens";

type Axis = "width" | "height" | "both";

/** A box that follows its content's natural size on a spring: the island growing a label, a group
 * opening, a toolbar swapping its buttons, a dialog changing step. The first size is set without
 * animating; reduced motion jumps. What doesn't fit yet is clipped (focus rings survive:
 * overflow-clip-margin). Width needs content that sets its own width (it is laid out max-content). */
export function Morph({ axis = "width", transition = SPRING.size, className, style, innerClassName, children }: {
  axis?: Axis;
  transition?: Transition;
  className?: string;
  style?: CSSProperties;
  innerClassName?: string;
  children: ReactNode;
}) {
  const inner = useRef<HTMLDivElement>(null);
  const width = useMotionValue<number | "auto">("auto");
  const height = useMotionValue<number | "auto">("auto");
  const reduced = useReducedMotionFlag();
  const first = useRef(true);
  const settings = useRef({ reduced, transition });
  settings.current = { reduced, transition };

  useLayoutEffect(() => {
    const el = inner.current;
    if (!el) return;
    const apply = () => {
      const { reduced, transition } = settings.current;
      const to = (mv: typeof width, v: number) => {
        if (first.current || reduced || mv.get() === "auto") {
          mv.stop();
          mv.set(v);
        } else if (mv.get() !== v) animate(mv, v, transition);
      };
      // Rounded up: offsetWidth/Height round down, and a box 0.2px short of its content shows a
      // scrollbar (a dialog's step). Not the bounding box: a dialog's entry scale skews that.
      const cs = getComputedStyle(el);
      if (axis !== "height") to(width, Math.ceil(parseFloat(cs.width)));
      if (axis !== "width") to(height, Math.ceil(parseFloat(cs.height)));
      first.current = false;
    };
    apply();
    const ro = new ResizeObserver(apply);
    ro.observe(el);
    return () => ro.disconnect();
  }, [axis, width, height]);

  const inline = axis !== "height";
  return (
    <motion.div
      className={className}
      style={{
        display: inline ? "inline-flex" : "block",
        width: axis === "height" ? undefined : width,
        height: axis === "width" ? undefined : height,
        overflow: "clip",
        overflowClipMargin: 4,
        flex: "none",
        maxWidth: "100%",
        ...style,
      }}
    >
      <div ref={inner} className={innerClassName} style={inline ? { display: "inline-flex", alignItems: "center", width: "max-content", flex: "none" } : undefined}>
        {children}
      </div>
    </motion.div>
  );
}

const VARIANTS = {
  fade: {
    initial: { opacity: 0 },
    animate: { opacity: 1, transition: TWEEN.fade },
    exit: { opacity: 0, transition: TWEEN.exit },
  },
  /** The reference's icon change: scale and a short blur, like motion blur. */
  blur: {
    initial: { opacity: 0, scale: 0.6, filter: "blur(4px)" },
    animate: { opacity: 1, scale: 1, filter: "blur(0px)", transition: TWEEN.icon },
    exit: { opacity: 0, scale: 0.6, filter: "blur(4px)", transition: TWEEN.exit },
  },
  rise: {
    initial: { opacity: 0, y: 6 },
    animate: { opacity: 1, y: 0, transition: SPRING.snappy },
    exit: { opacity: 0, y: -6, transition: TWEEN.exit },
  },
} as const;
const REDUCED = {
  initial: { opacity: 0 },
  animate: { opacity: 1, transition: { duration: 0.12 } },
  exit: { opacity: 0, transition: { duration: 0.08 } },
};

/** Cross-fades between keyed contents in place (the old one leaves while the new one arrives).
 * Usually inside a Morph, which then springs to the new size. */
export function Swap({ k, variant = "fade", className, children }: { k: string | number; variant?: keyof typeof VARIANTS; className?: string; children: ReactNode }) {
  const reduced = useReducedMotionFlag();
  const v = reduced ? REDUCED : VARIANTS[variant];
  return (
    <AnimatePresence mode="popLayout" initial={false}>
      <motion.span key={k} className={className} style={{ display: "inline-flex", alignItems: "center", gap: "inherit" }} initial={v.initial} animate={v.animate} exit={v.exit}>
        {children}
      </motion.span>
    </AnimatePresence>
  );
}
