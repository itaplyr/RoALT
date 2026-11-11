import type { ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import { useReducedMotionFlag } from "./reduced";
import { SPRING, TWEEN } from "./tokens";

/** Content that opens to its own height on a spring and closes faster, fading as it goes: a row that
 * depends on a switch, a disclosure's body, a notice. Reduced motion: the content fades, then the
 * height jumps (jumping first would leave a clip-margin sliver fading). Unmounted while closed. */
export function Collapse({ open, className, children }: { open: boolean; className?: string; children: ReactNode }) {
  const reduced = useReducedMotionFlag();
  const height = reduced ? { duration: 0 } : SPRING.size;
  return (
    <AnimatePresence initial={false}>
      {open && (
        <motion.div
          key="collapse"
          className={className}
          style={{ overflow: "clip", overflowClipMargin: 4 }}
          initial={{ height: 0, opacity: 0 }}
          animate={{ height: "auto", opacity: 1, transition: { height, opacity: { ...TWEEN.fade, delay: reduced ? 0 : 0.04 } } }}
          exit={{ height: 0, opacity: 0, transition: { height: reduced ? { duration: 0, delay: TWEEN.exit.duration } : TWEEN.exit, opacity: TWEEN.exit } }}
        >
          {children}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
