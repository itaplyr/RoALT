import { useEffect, useRef, useState, type CSSProperties } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api } from "../api";
import { qk } from "../data";
import { t } from "../i18n";
import { prefersReducedMotion } from "../ui/motion";
import { toast, toastError } from "../ui/Toast";

const HOLD_MS = 1000;

/** The panic button: held for a second (pointer, Space or Enter), it cancels the batch, stops
 * rejoins and restarts, and terminates every Roblox client at once, RoALT's or not. In the app menu
 * and the island's running sheet. */
export function KillAllButton({ role, onFired }: { role?: "menuitem"; onFired?: () => void }) {
  const qc = useQueryClient();
  const [held, setHeld] = useState(false);
  const [busy, setBusy] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const button = useRef<HTMLButtonElement>(null);
  useEffect(() => () => clearTimeout(timer.current), []);
  const fire = () => {
    if (!prefersReducedMotion()) button.current?.animate([{ filter: "brightness(1.9)" }, { filter: "none" }], { duration: 350, easing: "ease-out" });
    setHeld(false);
    setBusy(true);
    api
      .panicKill()
      .then((st) => {
        qc.setQueryData(qk.multi, st);
        toast(t("launch.kill.done"));
      }, toastError)
      .finally(() => setBusy(false));
    onFired?.();
  };
  const start = () => {
    if (busy || held) return;
    setHeld(true);
    timer.current = window.setTimeout(fire, HOLD_MS);
  };
  const stop = () => {
    clearTimeout(timer.current);
    setHeld(false);
  };
  const key = (e: { key: string }) => e.key === " " || e.key === "Enter";
  return (
    <button
      ref={button}
      type="button"
      // In a menu it is one of its items, so the menu's arrow keys reach it.
      role={role}
      className="btn btn-danger hold-btn relative overflow-hidden w-full whitespace-normal leading-tight py-1.5"
      style={{ "--hold": `${HOLD_MS}ms` } as CSSProperties}
      data-held={held || undefined}
      data-busy={busy || undefined}
      // Not `disabled`: that would drop the focus of whoever pressed it (start and the click check busy).
      aria-disabled={busy || undefined}
      data-tip={t("launch.kill.hint")}
      data-tip-side="top"
      // Screen readers and voice control activate without pressing anything to hold.
      onClick={(e) => e.detail === 0 && !busy && !held && fire()}
      onPointerDown={(e) => e.button === 0 && start()}
      onPointerUp={stop}
      onPointerLeave={stop}
      onKeyDown={(e) => {
        if (!key(e)) return;
        e.preventDefault();
        if (!e.repeat) start();
      }}
      onKeyUp={(e) => key(e) && stop()}
      onBlur={stop}
    >
      <span aria-hidden="true" className="hold-fill" />
      <span className="hold-label relative min-w-0">{busy ? t("launch.kill.killing") : held ? t("launch.kill.holding") : t("launch.kill.hold")}</span>
    </button>
  );
}
