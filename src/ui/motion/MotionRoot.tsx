import type { ReactNode } from "react";
import { MotionConfig } from "motion/react";
import { useMotionOff, useReducedMotionFlag } from "./reduced";
import { SPRING } from "./tokens";

/** Motion's settings for the whole app. Reduced: "always", so transforms and layout jump while
 * opacity and colour still ease. Off: every animation skips to its end. Width, height and blur
 * animations check the flag themselves (Morph, Collapse, IconSwap). */
export function MotionRoot({ children }: { children: ReactNode }) {
  const reduced = useReducedMotionFlag();
  const off = useMotionOff();
  return (
    <MotionConfig reducedMotion={reduced ? "always" : "never"} skipAnimations={off} transition={SPRING.snappy}>
      {children}
    </MotionConfig>
  );
}
