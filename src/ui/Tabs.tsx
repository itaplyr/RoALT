import { useRef, type KeyboardEvent, type ReactNode } from "react";
import { isRtl } from "../i18n";
import { Icon, type IconName } from "./Icon";
import { useLiquid } from "./motion";

/** A tablist with a thumb that glides to the selected tab. Arrows, Home and End move and select
 * (automatic activation); one Tab stop. `panelId`: the tabpanel the tabs control. */
export function Tabs<T extends string | number>({ label, value, onChange, items, panelId, className = "" }: {
  label: string;
  value: T;
  onChange: (v: T) => void;
  items: { value: T; label: ReactNode; icon?: IconName; count?: number; disabled?: boolean }[];
  panelId?: string;
  className?: string;
}) {
  const list = useRef<HTMLDivElement>(null);
  const thumb = useRef<HTMLSpanElement>(null);
  useLiquid(list, thumb, "[role=tab][aria-selected=true]");
  const enabled = items.filter((i) => !i.disabled);
  // The one Tab stop: the selected tab, or the first when none is (it stays reachable).
  const stop = enabled.some((i) => i.value === value) ? value : enabled[0]?.value;
  const key = (e: KeyboardEvent) => {
    // From the focused tab: the selected one may be none, or lag (Browse's follows its page loading).
    const from = (e.target as HTMLElement).closest<HTMLElement>("[role=tab]")?.dataset.value;
    const focused = enabled.findIndex((i) => String(i.value) === from);
    const at = focused >= 0 ? focused : enabled.findIndex((i) => i.value === value);
    const back = isRtl() ? "ArrowRight" : "ArrowLeft";
    const next = e.key === back ? at - 1 : e.key === (isRtl() ? "ArrowLeft" : "ArrowRight") ? at + 1 : e.key === "Home" ? 0 : e.key === "End" ? enabled.length - 1 : null;
    if (next === null) return;
    e.preventDefault();
    const to = enabled[(next + enabled.length) % enabled.length];
    onChange(to.value);
    requestAnimationFrame(() => list.current?.querySelector<HTMLElement>(`[data-value="${CSS.escape(String(to.value))}"]`)?.focus());
  };
  return (
    <div ref={list} role="tablist" aria-label={label} className={`seg-group ${className}`} data-thumb onKeyDown={key}>
      <span ref={thumb} aria-hidden="true" className="seg-thumb tab-thumb lq-drop" />
      {items.map((i) => {
        const on = i.value === value;
        return (
          <button
            key={i.value}
            type="button"
            role="tab"
            className="seg inline-flex items-center gap-2"
            data-value={i.value}
            aria-selected={on}
            aria-controls={panelId}
            tabIndex={i.value === stop ? 0 : -1}
            disabled={i.disabled}
            onClick={() => !on && onChange(i.value)}
          >
            {i.icon && <Icon name={i.icon} size={14} />}
            {i.label}
            {i.count !== undefined && <span className="text-fg-3 tabular-nums">{i.count}</span>}
          </button>
        );
      })}
    </div>
  );
}
