import type { ButtonHTMLAttributes, CSSProperties, ReactNode } from "react";
import { Icon } from "./Icon";

/** A loading placeholder at the content's real size (no shimmer). */
export function Skeleton({ width, height = 12, radius, className = "" }: { width?: number | string; height?: number; radius?: number; className?: string }) {
  return <span aria-hidden="true" className={`skel block ${className}`} style={{ width, height, borderRadius: radius }} />;
}

/** "Ctrl+L" as keycaps. */
export function Kbd({ keys }: { keys: string }) {
  return (
    <span className="inline-flex items-center gap-1" dir="ltr">
      {keys.split("+").map((k) => (
        <kbd key={k} className="kbd">{k}</kbd>
      ))}
    </span>
  );
}

/** A filter or pick chip. `dot`: an identity colour (a group's). */
export function Chip({ pressed, dot, count, small, className = "", children, ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & {
  pressed?: boolean;
  dot?: string;
  count?: number;
  small?: boolean;
}) {
  return (
    <button type="button" className={`chip ${small ? "chip-sm" : ""} ${className}`} aria-pressed={pressed} {...rest}>
      {dot && <span aria-hidden="true" className="w-2 h-2 rounded-full flex-none" style={{ background: dot } as CSSProperties} />}
      {children}
      {count !== undefined && <span className="text-fg-3 font-normal tabular-nums">{count}</span>}
    </button>
  );
}

/** A native disclosure whose body opens to its height (styles: details::details-content). */
export function Disclosure({ summary, defaultOpen, onToggle, className = "", children }: {
  summary: ReactNode;
  defaultOpen?: boolean;
  onToggle?: (open: boolean) => void;
  className?: string;
  children: ReactNode;
}) {
  return (
    <details className={`disclosure ${className}`} open={defaultOpen} onToggle={(e) => onToggle?.(e.currentTarget.open)}>
      <summary className="disclosure-summary">
        <span className="min-w-0">{summary}</span>
        <Icon name="chevron" size={14} className="disclosure-chevron" />
      </summary>
      <div className="disclosure-body">{children}</div>
    </details>
  );
}
