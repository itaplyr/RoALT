// Motion tokens, mirrored from styles/tokens.css (assertMotionTokens checks they agree in dev).
// Springs for what moves and resizes, short tweens for what leaves. Exits are faster than entrances.
import type { Transition } from "motion/react";

export const DUR = { instant: 80, fast: 140, base: 200, exit: 120, dialogExit: 160, move: 220 } as const;
export const EASE = { out: [0.22, 1, 0.36, 1], in: [0.4, 0, 1, 1] } as const;
export const EASE_CSS = { out: "cubic-bezier(0.22, 1, 0.36, 1)" } as const;

/** Liquid: everything that moves or resizes overshoots a little and settles, like a drop. */
export const SPRING = {
  /** Tiles, highlights, the hover ghost, thumbs, toasts. */
  snappy: { type: "spring", visualDuration: 0.3, bounce: 0.32 },
  /** The island's width, groups, Morph, Collapse. */
  smooth: { type: "spring", visualDuration: 0.42, bounce: 0.26 },
  /** Sheets, dialog steps, the rail. */
  gentle: { type: "spring", visualDuration: 0.55, bounce: 0.18 },
  /** Focal moments only: the island inflating, the launch flying into it. */
  inflate: { type: "spring", visualDuration: 0.6, bounce: 0.4 },
  /** Sizes (a label's width, a group, a collapsing row): no bounce. A size that overshoots shoves
   * its neighbours back and forth, and a drop following it wobbles twice over. */
  size: { type: "spring", visualDuration: 0.38, bounce: 0 },
} as const satisfies Record<string, Transition>;

export const TWEEN = {
  exit: { duration: DUR.exit / 1000, ease: EASE.in },
  fade: { duration: DUR.fast / 1000, ease: EASE.out },
  icon: { duration: 0.18, ease: EASE.out },
  number: { duration: 0.6, ease: EASE.out },
} as const satisfies Record<string, Transition>;

/** List entrances: 24 ms apart, at most 8 steps; lists past 40 items don't animate at all. */
export const STAGGER = { step: 0.024, max: 8, listLimit: 40 } as const;
export const RISE = { sm: 4, md: 8 } as const;

/** Shared elements that travel between parts of the tree (the only global layoutIds). */
export const LAYOUT_IDS = { launchLive: "live:launch" } as const;
/** A layoutId scoped to one component instance: lid("seg", useId(), "thumb"). */
export const lid = (...parts: (string | number)[]) => parts.join(":");

/** Dev only: the CSS tokens and these must not drift apart. */
export function assertMotionTokens() {
  const css = getComputedStyle(document.documentElement);
  const pairs: [string, number][] = [
    ["--dur-fast", DUR.fast],
    ["--dur-base", DUR.base],
    ["--dur-exit", DUR.exit],
    ["--dur-dialog-exit", DUR.dialogExit],
    ["--dur-move", DUR.move],
  ];
  for (const [name, ms] of pairs) {
    const v = parseFloat(css.getPropertyValue(name));
    // Reduced motion zeroes some of them on purpose.
    if (!document.documentElement.hasAttribute("data-rm") && v !== ms) console.warn(`motion token ${name} is ${v}ms in CSS, ${ms}ms in tokens.ts`);
  }
}
