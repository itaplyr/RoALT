import { useEffect, type KeyboardEvent, type PointerEvent } from "react";
import { isRtl, t } from "../i18n";
import { useStored } from "./stored";

const KEY = "roalt.width.";

/** A panel width the user can drag, remembered per panel (a per-viewer convenience). */
export function usePanelWidth(id: string, initial: number, min: number, max: number) {
  const clamp = (w: number) => Math.round(Math.min(max, Math.max(min, w)));
  const [width, set] = useStored(KEY + id, (s) => (Number(s) ? clamp(Number(s)) : initial), String);
  const setWidth = (w: number) => set(clamp(w));
  return { width, setWidth, reset: () => setWidth(initial), min, max };
}

/** A draggable divider on a panel's edge. `side` is where the panel sits: dragging away from
 * it widens the panel. Arrow keys move it by 16 px; a double click restores the default. */
export function Divider({ panel, side, label }: {
  panel: ReturnType<typeof usePanelWidth>;
  side: "left" | "right";
  label: string;
}) {
  // `side` as in a left-to-right language; right to left, the layout has mirrored.
  const sign = (side === "left") !== isRtl() ? 1 : -1;
  // A view switch mid-drag removes this before the pointer comes up.
  useEffect(() => () => void (document.body.style.cursor = ""), []);

  const drag = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const [x0, w0] = [e.clientX, panel.width];
    const move = (m: globalThis.PointerEvent) => panel.setWidth(w0 + sign * (m.clientX - x0));
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      document.body.style.cursor = "";
    };
    document.body.style.cursor = "col-resize";
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  };

  const key = (e: KeyboardEvent) => {
    const step = { ArrowLeft: -16, ArrowRight: 16 }[e.key];
    if (step === undefined) return;
    e.preventDefault();
    panel.setWidth(panel.width + sign * step);
  };

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={panel.width}
      aria-valuemin={panel.min}
      aria-valuemax={panel.max}
      tabIndex={0}
      title={t("ui.resizeHint")}
      onPointerDown={drag}
      onDoubleClick={panel.reset}
      onKeyDown={key}
      className="relative z-10 w-0 flex-none outline-none cursor-col-resize before:absolute before:inset-y-0 before:-start-[3px] before:w-[6px] before:content-[''] hover:before:bg-edge focus-visible:before:bg-accent-text"
    />
  );
}
