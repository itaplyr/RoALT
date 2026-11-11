import { useId, type InputHTMLAttributes, type ReactNode, type Ref, type SelectHTMLAttributes } from "react";
import { Icon, type IconName } from "./Icon";

/** A label above its control, an optional one-line hint, an error tied to it (aria-describedby). */
export function Field({ label, hint, error, children, className = "" }: {
  label: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
  /** Gets the ids to wire: `<Input id={id} aria-describedby={describedBy} />`. */
  children: (ids: { id: string; describedBy?: string; invalid: boolean }) => ReactNode;
  className?: string;
}) {
  const id = useId();
  const hintId = `${id}-hint`;
  const errorId = `${id}-error`;
  const describedBy = [hint && hintId, error && errorId].filter(Boolean).join(" ") || undefined;
  return (
    <div className={`flex flex-col gap-1.5 ${className}`}>
      <label htmlFor={id} className="label">{label}</label>
      {children({ id, describedBy, invalid: !!error })}
      {hint && !error && <p id={hintId} className="text-[13px] text-fg-3">{hint}</p>}
      {error && <p id={errorId} role="alert" className="text-[13px] text-attention">{error}</p>}
    </div>
  );
}

/** A text input with an optional leading icon (search, link). */
export function Input({ leading, mono, invalid, className = "", ref, ...rest }: InputHTMLAttributes<HTMLInputElement> & {
  leading?: IconName;
  mono?: boolean;
  invalid?: boolean;
  ref?: Ref<HTMLInputElement>;
}) {
  const input = (
    <input
      ref={ref}
      className={`field ${mono ? "mono" : ""} ${leading ? "field-search" : ""} ${className}`}
      aria-invalid={invalid || undefined}
      spellCheck={false}
      {...rest}
    />
  );
  if (!leading) return input;
  return (
    <span className="relative flex-1 min-w-0 flex">
      <span aria-hidden="true" className="pointer-events-none absolute inset-y-0 start-3 grid place-content-center text-fg-3">
        <Icon name={leading} size={15} />
      </span>
      {input}
    </span>
  );
}

/** A native select with the app's chevron. */
export function Select<T extends string>({ value, onChange, options, className = "", ...rest }: Omit<SelectHTMLAttributes<HTMLSelectElement>, "onChange" | "value"> & {
  value: T;
  onChange: (v: T) => void;
  /** `lang`: the option's own language (a language's native name), so screen readers voice it so. */
  options: readonly { value: T; label: string; lang?: string }[];
}) {
  return (
    <span className={`relative inline-flex ${className}`}>
      <select className="field appearance-none pe-9 w-full" value={value} onChange={(e) => onChange(e.target.value as T)} {...rest}>
        {options.map((o) => (
          <option key={o.value} value={o.value} lang={o.lang}>{o.label}</option>
        ))}
      </select>
      <span aria-hidden="true" className="pointer-events-none absolute inset-y-0 end-3 grid place-content-center text-fg-3">
        <Icon name="chevron" size={14} />
      </span>
    </span>
  );
}

/** A checkbox with its label; `hideLabel` keeps the label for screen readers only. */
export function Checkbox({ checked, indeterminate, onChange, label, hideLabel, disabled, className = "" }: {
  checked: boolean;
  indeterminate?: boolean;
  onChange: (v: boolean) => void;
  label: ReactNode;
  hideLabel?: boolean;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <label className={`inline-flex items-center gap-2.5 ${disabled ? "opacity-60" : "cursor-pointer"} ${className}`}>
      <input
        type="checkbox"
        className="cb"
        checked={checked}
        disabled={disabled}
        ref={(el) => {
          if (el) el.indeterminate = !!indeterminate;
        }}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span className={hideLabel ? "sr-only" : "min-w-0"}>{label}</span>
    </label>
  );
}

export function Radio({ checked, onChange, name, value, label, disabled }: {
  checked: boolean;
  onChange: () => void;
  name: string;
  value: string;
  label: ReactNode;
  disabled?: boolean;
}) {
  return (
    <label className={`inline-flex items-center gap-2.5 ${disabled ? "opacity-60" : "cursor-pointer"}`}>
      <input type="radio" className="radio" name={name} value={value} checked={checked} disabled={disabled} onChange={onChange} />
      <span className="min-w-0">{label}</span>
    </label>
  );
}
