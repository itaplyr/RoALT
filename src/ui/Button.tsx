import type { ButtonHTMLAttributes, Ref } from "react";
import { Icon, type IconName } from "./Icon";

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "lg" | "md" | "sm";
const SIZE: Record<Size, string> = { lg: "", md: "btn-sm", sm: "btn-xs" };
const VARIANT: Record<Variant, string> = { primary: "btn-primary", secondary: "", ghost: "btn-ghost", danger: "btn-danger" };

/** A pill button. `busy` keeps the label (and the width) and spins an icon beside it. */
export function Button({ variant = "secondary", size = "lg", icon, iconEnd, busy, block, className = "", children, ref, ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant;
  size?: Size;
  icon?: IconName;
  iconEnd?: IconName;
  busy?: boolean;
  block?: boolean;
  ref?: Ref<HTMLButtonElement>;
}) {
  const s = size === "lg" ? 16 : 14;
  return (
    <button
      ref={ref}
      type="button"
      className={`btn ${VARIANT[variant]} ${SIZE[size]} ${block ? "btn-block" : ""} ${className}`}
      aria-busy={busy || undefined}
      data-busy={busy || undefined}
      {...rest}
    >
      {busy ? <Icon name="refresh" size={s} className="spin" /> : icon && <Icon name={icon} size={s} />}
      {children}
      {iconEnd && <Icon name={iconEnd} size={s} />}
    </button>
  );
}

const ICON_SIZE: Record<Size, string> = { lg: "w-10 h-10", md: "", sm: "w-7 h-7" };

/** A round icon-only button. `label` is its accessible name and its tooltip; `shortcut` shows in it. */
export function IconButton({ icon, label, size = "md", shortcut, className = "", iconSize, ref, ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & {
  icon: IconName;
  label: string;
  size?: Size;
  shortcut?: string;
  iconSize?: number;
  ref?: Ref<HTMLButtonElement>;
}) {
  return (
    <button
      ref={ref}
      type="button"
      className={`wc ${ICON_SIZE[size]} ${className}`}
      aria-label={label}
      data-tip={label}
      data-tip-kbd={shortcut}
      {...rest}
    >
      <Icon name={icon} size={iconSize ?? (size === "sm" ? 14 : 16)} />
    </button>
  );
}
