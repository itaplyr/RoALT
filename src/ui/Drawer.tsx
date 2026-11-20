import { useEffect, useState, type ReactNode } from "react";
import { useModal } from "./Dialog";
import { DUR } from "./motion";

/** A modal sheet docked to the end side (`.drawer`). Its children are there while open and while it
 * slides out, then let go. Esc and a click on the backdrop close it; a dialog opened inside keeps its
 * own Esc. As a <dialog>, it also hides Browse's page while open. `focus`: what gets the focus when
 * it opens (a selector inside it); `width`: px, the default 760. */
export function Drawer({ open, onClose, labelledBy, focus, width, children }: {
  open: boolean;
  onClose: () => void;
  labelledBy: string;
  focus?: string;
  width?: number;
  children: ReactNode;
}) {
  const [mounted, setMounted] = useState(open);
  const [was, setWas] = useState(open);
  if (open !== was) {
    setWas(open);
    if (open) setMounted(true);
  }

  const modal = useModal(open, onClose, (d) => {
    if (focus) d.querySelector<HTMLElement>(focus)?.focus();
  });
  useEffect(() => {
    if (open || !mounted) return;
    const timer = setTimeout(() => setMounted(false), DUR.exit + 90);
    return () => clearTimeout(timer);
  }, [open, mounted]);

  return (
    <dialog
      {...modal}
      className="drawer"
      style={width ? { width: `min(${width}px, calc(100vw - 48px))` } : undefined}
      aria-labelledby={labelledBy}
      data-tauri-drag-region="false"
    >
      {mounted && <div className="drawer-body">{children}</div>}
    </dialog>
  );
}
