import type { ReactNode } from "react";

export type Tone = "ok" | "info" | "attention" | "warn" | "err" | "neutral";
export type Shape = "dot" | "ring" | "diamond" | "triangle" | "square" | "dash";

/** Status hues (never the accent: a user's accent must not repaint "in game"). */
export const TONE_COLOR: Record<Tone, string> = {
  ok: "var(--ok)",
  info: "var(--info)",
  attention: "var(--attention)",
  warn: "var(--warn)",
  err: "var(--err)",
  neutral: "var(--fg-3)",
};
/** Each tone has its own shape too: status is told apart without colour (joining's ring is not in
 * game's dot, nor starting's dash). */
export const TONE_SHAPE: Record<Tone, Shape> = { ok: "dot", info: "ring", attention: "diamond", warn: "triangle", err: "square", neutral: "dash" };
const SHAPE = TONE_SHAPE;

/** A tinted pill: shape, word, optional detail. `pulse` while something is under way. */
export function StatusPill({ tone, shape, label, detail, pulse, className = "" }: {
  tone: Tone;
  shape?: Shape;
  label: ReactNode;
  detail?: ReactNode;
  pulse?: boolean;
  className?: string;
}) {
  return (
    <span className={`status-pill ${className}`} style={{ color: TONE_COLOR[tone] }}>
      <span aria-hidden="true" className={`status-mark ${pulse ? "pulse" : ""}`} data-shape={shape ?? SHAPE[tone]} />
      <span className="text-fg">{label}</span>
      {detail && <span className="text-fg-3 font-normal truncate">{detail}</span>}
    </span>
  );
}

/** Just the shape, with its word for screen readers. */
export function StatusDot({ tone, shape, label, pulse, className = "" }: { tone: Tone; shape?: Shape; label: string; pulse?: boolean; className?: string }) {
  return (
    <span className={`inline-grid place-items-center ${className}`} style={{ color: TONE_COLOR[tone] }} title={label}>
      <span aria-hidden="true" className={`status-mark ${pulse ? "pulse" : ""}`} data-shape={shape ?? SHAPE[tone]} />
      <span className="sr-only">{label}</span>
    </span>
  );
}
