import type { CSSProperties } from "react";
import { motion } from "motion/react";
import { Num } from "./motion/Num";
import { SPRING } from "./motion/tokens";
import { TONE_COLOR, type Tone } from "./Status";

/** A bar at the true ratio (both ends known). `value` 0..max; null shows an empty track. */
export function Meter({ value, max = 100, label, tone, className = "" }: { value: number | null; max?: number; label: string; tone?: Tone | "accent"; className?: string }) {
  const pct = value === null ? 0 : Math.max(0, Math.min(100, (value / max) * 100));
  return (
    <span
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={value ?? undefined}
      className={`meter ${className}`}
    >
      <span className="meter-fill" style={{ "--pct": `${pct}%`, background: tone && tone !== "accent" ? TONE_COLOR[tone] : undefined } as CSSProperties} />
    </span>
  );
}

/** A thin ring with the number inside: eases between samples (no React render per frame). */
export function Dial({ value, label, text, tone = "neutral", size = 64, stroke = 4 }: {
  /** 0..100; null when Windows doesn't say. */
  value: number | null;
  label: string;
  /** What the centre reads; default the rounded percentage. */
  text?: (n: number) => string;
  tone?: Tone | "accent";
  size?: number;
  stroke?: number;
}) {
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const pct = value === null ? 0 : Math.max(0, Math.min(100, value));
  const color = tone === "accent" ? "var(--accent-text)" : TONE_COLOR[tone];
  return (
    <span className="dial" style={{ width: size, height: size }} role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={value ?? undefined}>
      <svg viewBox={`0 0 ${size} ${size}`} aria-hidden="true">
        <circle className="dial-track" cx={size / 2} cy={size / 2} r={r} strokeWidth={stroke} />
        <motion.circle
          className="dial-value"
          cx={size / 2}
          cy={size / 2}
          r={r}
          strokeWidth={stroke}
          style={{ color }}
          strokeDasharray={c}
          initial={false}
          animate={{ strokeDashoffset: c * (1 - pct / 100), opacity: value === null ? 0 : 1 }}
          transition={SPRING.size}
        />
      </svg>
      <Num value={value} format={text ?? ((n) => `${Math.round(n)}%`)} className="relative text-[13px] font-semibold" />
    </span>
  );
}
