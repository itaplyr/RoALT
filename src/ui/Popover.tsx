import { useId, useLayoutEffect, useRef, type CSSProperties, type MouseEvent, type ReactNode, type RefObject } from "react";
import { isRtl } from "../i18n";
import { Icon, type IconName } from "./Icon";
import { Kbd } from "./Bits";
import { menuKeys } from "./menu";

/** A CSS anchor name for a trigger, and the style that names it. */
export function useAnchor() {
  const name = `--a${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
  return { name, style: { anchorName: name } as CSSProperties };
}

type Placement = "below-start" | "below-end" | "above-start";
const AREA: Record<Placement, string> = {
  "below-start": "block-end span-inline-end",
  "below-end": "block-end span-inline-start",
  "above-start": "block-start span-inline-end",
};

/** A popover (menu, picker, small form) anchored to its trigger; it opens from the trigger's side.
 * The trigger: `<button popoverTarget={id} style={anchor.style}>`. In the title bar it must not be a
 * drag region, so it opts out. */
export function Popover({ id, anchor, placement = "below-start", role, label, className = "", style, onToggle, children }: {
  id: string;
  anchor: string;
  placement?: Placement;
  role?: "menu" | "dialog" | "listbox";
  label?: string;
  className?: string;
  style?: CSSProperties;
  onToggle?: (open: boolean) => void;
  children: ReactNode;
}) {
  return (
    <div
      id={id}
      popover="auto"
      role={role}
      aria-label={label}
      className={`menu ${className}`}
      // The title bar is left to right; what opens from it reads in the app's language.
      dir={isRtl() ? "rtl" : "ltr"}
      data-up={placement.startsWith("above") || undefined}
      data-tauri-drag-region="false"
      style={{ positionAnchor: anchor, positionArea: AREA[placement], ...style } as CSSProperties}
      onKeyDown={role === "menu" ? menuKeys : undefined}
      onToggle={(e) => {
        const open = (e as unknown as ToggleEvent).newState === "open";
        // A menu opens on its checked item, else its first: the arrows start from there.
        const menu = e.currentTarget;
        if (open && role === "menu")
          requestAnimationFrame(() => {
            const item = menu.querySelector<HTMLElement>("[role^=menuitem][aria-checked=true]:not(:disabled)") ?? menu.querySelector<HTMLElement>(".menu-item:not(:disabled), [role^=menuitem]:not(:disabled)");
            item?.focus({ preventScroll: true });
            // In a long menu the checked item may sit below its fold; the page itself must not scroll.
            item?.scrollIntoView({ block: "nearest" });
          });
        onToggle?.(open);
      }}
    >
      {children}
    </div>
  );
}

const closeMenu = (e: MouseEvent<HTMLElement>) => e.currentTarget.closest<HTMLElement>("[popover]")?.hidePopover();

/** A menu item; it closes its menu unless `closes` is false. */
export function MenuItem({ icon, danger, kbd, disabled, closes = true, onSelect, children }: {
  icon?: IconName;
  danger?: boolean;
  kbd?: string;
  disabled?: boolean;
  closes?: boolean;
  onSelect: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      className="menu-item"
      data-danger={danger || undefined}
      disabled={disabled}
      onClick={(e) => {
        if (closes) closeMenu(e);
        onSelect();
      }}
    >
      {icon && <Icon name={icon} />}
      <span className="flex-1 min-w-0 truncate">{children}</span>
      {kbd && <Kbd keys={kbd} />}
    </button>
  );
}

/** An on/off item; the menu stays open so several can be set. */
export function MenuCheckItem({ checked, disabled, onChange, children }: { checked: boolean; disabled?: boolean; onChange: (v: boolean) => void; children: ReactNode }) {
  return (
    <button type="button" role="menuitemcheckbox" aria-checked={checked} className="menu-item" disabled={disabled} onClick={() => onChange(!checked)}>
      <span className="w-4 grid place-content-center">{checked && <Icon name="check" size={14} />}</span>
      <span className="flex-1 min-w-0 truncate">{children}</span>
    </button>
  );
}

/** One of several (a CPU cap); picking it keeps the menu open. */
export function MenuRadioItem({ checked, disabled, onSelect, children }: { checked: boolean; disabled?: boolean; onSelect: () => void; children: ReactNode }) {
  return (
    <button type="button" role="menuitemradio" aria-checked={checked} className="menu-item" disabled={disabled} onClick={() => !checked && onSelect()}>
      <span className="w-4 grid place-content-center">{checked && <span className="w-1.5 h-1.5 rounded-full bg-fg" />}</span>
      <span className="flex-1 min-w-0 truncate">{children}</span>
    </button>
  );
}

export const MenuSeparator = () => <div role="separator" className="menu-sep" />;
export const MenuLabel = ({ children }: { children: ReactNode }) => <div className="menu-label">{children}</div>;

/** A menu at the pointer (right-click) or at an element's corner (the Menu key). Stays inside the
 * window, focuses its first item, and gives the focus back to `at.from` when it closes, unless a click
 * took it somewhere else. With `at.beside` (the left edge of the key that opened it, `at.x` its right
 * edge) it opens beside that key, not over it. */
export function ContextMenu({ at, label, className = "", style, ref, onClose, children }: {
  at: { x: number; y: number; from: HTMLElement; beside?: number } | null;
  label: string;
  className?: string;
  style?: CSSProperties;
  ref?: RefObject<HTMLDivElement | null>;
  onClose: () => void;
  children: ReactNode;
}) {
  const own = useRef<HTMLDivElement>(null);
  const box = ref ?? own;
  useLayoutEffect(() => {
    const el = box.current;
    if (!el || !at) return;
    el.style.left = `${at.x}px`;
    el.style.top = `${at.y}px`;
    el.showPopover();
    const r = el.getBoundingClientRect();
    if (at.beside === undefined) {
      el.style.left = `${Math.max(8, Math.min(at.x, innerWidth - r.width - 8))}px`;
      el.style.top = `${Math.max(8, at.y + r.height > innerHeight - 8 ? at.y - r.height : at.y)}px`;
    } else {
      // On the side the text starts from: left of the key, or right of it right to left.
      el.style.left = `${isRtl() ? Math.min(at.x + 6, innerWidth - r.width - 4) : Math.max(4, at.beside - r.width - 6)}px`;
      el.style.top = `${Math.max(4, Math.min(at.y, innerHeight - r.height - 4))}px`;
    }
    el.querySelector<HTMLElement>("[role^=menuitem]:not(:disabled)")?.focus();
  }, [at]); // eslint-disable-line react-hooks/exhaustive-deps
  return (
    <div
      ref={box}
      popover="auto"
      role="menu"
      aria-label={label}
      className={`menu ${className}`}
      dir={isRtl() ? "rtl" : "ltr"}
      data-tauri-drag-region="false"
      style={{ position: "fixed", margin: 0, positionArea: "none", ...style } as CSSProperties}
      onKeyDown={menuKeys}
      onToggle={(e) => {
        if ((e as unknown as ToggleEvent).newState !== "closed") return;
        const f = document.activeElement;
        if (!f || f === document.body || e.currentTarget.contains(f)) at?.from.focus({ preventScroll: true });
        onClose();
      }}
    >
      {at && children}
    </div>
  );
}
