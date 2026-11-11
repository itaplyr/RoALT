import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { toast } from "./Toast";
import { fmtNum, t } from "../i18n";
import { Morph, useLiquid } from "./motion";
import type { Range } from "./ranges";

export function Switch({ checked, onChange, label, disabled, describedBy }: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
  describedBy?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      className="switch"
      aria-checked={checked}
      aria-label={label}
      aria-describedby={describedBy}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    />
  );
}

/** Mutually exclusive choices on a track; the pressed one sits on a thumb that glides between them. */
export function Segmented<T extends string | number>({ label, value, options, onChange, mono, className = "" }: {
  label: string;
  value: T;
  options: { value: T; label: ReactNode; disabled?: boolean; title?: string }[];
  onChange: (v: T) => void;
  mono?: boolean;
  className?: string;
}) {
  const group = useRef<HTMLDivElement>(null);
  const thumb = useRef<HTMLSpanElement>(null);
  useLiquid(group, thumb, ".seg[aria-pressed=true]");
  return (
    <div ref={group} role="group" aria-label={label} className={`seg-group ${className}`} data-thumb>
      <span ref={thumb} aria-hidden="true" className="seg-thumb lq-drop" />
      {options.map((o) => {
        const on = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            className={mono ? "seg mono" : "seg"}
            data-choice
            disabled={o.disabled}
            title={o.title}
            aria-pressed={on}
            onClick={() => !on && onChange(o.value)}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

/** Text input that saves (trimmed) on blur or Enter; Escape puts the saved value back, and so
 * does a refused save (`onCommit` rejects; the caller shows why). */
export function TextField({ label, value, placeholder, className = "", mono = true, describedBy, onCommit }: {
  label: string;
  value: string;
  placeholder?: string;
  className?: string;
  mono?: boolean;
  describedBy?: string;
  onCommit: (text: string) => Promise<unknown>;
}) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  const commit = () => {
    const [next, before] = [text.trim(), value];
    if (next !== before) onCommit(next).catch(() => setText(before));
  };
  return (
    <input
      className={`field ${mono ? "mono" : ""} ${className}`}
      aria-label={label}
      aria-describedby={describedBy}
      value={text}
      placeholder={placeholder}
      spellCheck={false}
      autoComplete="off"
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
        if (e.key === "Escape" && text !== value) {
          e.stopPropagation();
          setText(value);
        }
      }}
    />
  );
}

/** Number input that saves on blur or Enter. Out of range or off-step: it says so. With `onInvalid`
 * the caller shows that inline, and what was typed stays, marked invalid, until a good value or
 * Esc; without it, a toast, and it snaps back. */
export function NumberField({ label, name, unit, short, min, max, step = 1, value, onCommit, onInvalid, disabled, describedBy, className = "w-20" }: {
  /** Shown before the field; else `name` (default `unit`) names it for screen readers only. */
  label?: string;
  name?: string;
  unit: string;
  short: string;
  min: number;
  max: number;
  step?: number;
  value: number;
  onCommit: (n: number) => void;
  onInvalid?: (message: string | null) => void;
  disabled?: boolean;
  describedBy?: string;
  className?: string;
}) {
  const [text, setText] = useState(String(value));
  const [bad, setBad] = useState(false);
  useEffect(() => {
    setText(String(value));
    setBad(false);
  }, [value]);
  const commit = () => {
    const n = Number(text);
    if (n === value) {
      setBad(false);
      return onInvalid?.(null);
    }
    if (text.trim() !== "" && Number.isInteger(n / step) && n >= min && n <= max) {
      setBad(false);
      onInvalid?.(null);
      onCommit(n);
    } else {
      const bounds = { unit, min: fmtNum(min), max: fmtNum(max) };
      const message = step === 1 ? t("ui.numberWhole", bounds) : t("ui.numberStep", { ...bounds, step: fmtNum(step) });
      if (onInvalid) {
        onInvalid(message);
        setBad(true);
      } else {
        toast(message, "error");
        setText(String(value));
      }
    }
  };
  return (
    <label className="flex items-center gap-2">
      {label ? <span>{label}</span> : <span className="sr-only">{name ?? unit}</span>}
      <input
        className={`field mono ${className}`}
        type="number"
        min={min}
        max={max}
        step={step}
        value={text}
        disabled={disabled}
        aria-invalid={bad || undefined}
        aria-describedby={describedBy}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        // The wheel steps a focused number field: scrolling past would change a setting.
        onWheel={(e) => e.currentTarget.blur()}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
          if (e.key === "Escape" && (bad || text !== String(value))) {
            e.stopPropagation();
            setText(String(value));
            setBad(false);
            onInvalid?.(null);
          }
        }}
      />
      {/* Named by its label when there is one; else the hidden name already says the unit. */}
      <span className="text-fg-3" aria-hidden={label ? undefined : true}>{short}</span>
    </label>
  );
}

/** "Off, or a value in range" (FPS cap, memory limits, summaries...): a switch and a number that
 * only takes what the backend accepts. Off writes 0; on writes the last value seen this session, or
 * `range.on`. A bad number is explained beside the field, not after the fact. */
export function OffOrNumberField({ label, value, range, unit, short, onCommit, describedBy }: {
  label: string;
  value: number;
  range: Range;
  unit: string;
  short: string;
  onCommit: (n: number) => void;
  describedBy?: string;
}) {
  const last = useRef(value || range.on);
  if (value) last.current = value;
  const [error, setError] = useState<string | null>(null);
  const errorId = useId();
  const on = value !== 0;
  return (
    <span className="inline-flex flex-col items-end gap-1">
      {/* The number slides out from its switch as it turns on. */}
      <Morph style={{ justifyContent: "flex-end" }} innerClassName="gap-3">
        {on && (
          <NumberField
            name={`${label}, ${unit}`}
            unit={unit}
            short={short}
            min={range.min}
            max={range.max}
            step={range.step}
            value={value}
            onCommit={onCommit}
            onInvalid={setError}
            describedBy={error ? errorId : describedBy}
            className="w-24"
          />
        )}
        <Switch
          checked={on}
          label={label}
          describedBy={describedBy}
          onChange={(next) => {
            setError(null);
            onCommit(next ? last.current : 0);
          }}
        />
      </Morph>
      {error && (
        <span id={errorId} role="alert" className="text-xs text-attention max-w-64 text-end">
          {error}
        </span>
      )}
    </span>
  );
}
