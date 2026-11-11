import { playSound, type Sound } from "./sounds";
import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import { AnimatePresence, motion } from "motion/react";
import { asError, type ErrorPayload } from "../api";
import { IconButton } from "./Button";
import { isRtl, t, type Key } from "../i18n";
import { SPRING, TWEEN, useReducedMotionFlag } from "./motion";

type Tone = "ok" | "error";
type Item = { id: number; text: string; tone: Tone };

let items: Item[] = [];
let nextId = 0;
const subs = new Set<() => void>();
const publish = (next: Item[]) => {
  items = next;
  subs.forEach((f) => f());
};
const subscribe = (f: () => void) => {
  subs.add(f);
  return () => subs.delete(f);
};

/** Gone from the list at once; AnimatePresence plays its exit. */
function dismiss(id: number) {
  timers.delete(id);
  publish(items.filter((t) => t.id !== id));
}

/** Shows a short message at the bottom end. Callable from anywhere, including outside React. A
 *  sticky one stays until dismissed. */
export function toast(text: string, tone: Tone = "ok", sticky = false, sound?: Sound) {
  playSound(sound ?? (tone === "error" ? "error" : "success"));
  const id = ++nextId;
  publish([...items.slice(-3), { id, text, tone }]);
  if (!sticky) later(id, tone === "error" ? 7000 : 4000);
}

/** Pointer or focus on a toast holds it: it is being read. */
const timers = new Map<number, number>();
function later(id: number, ms: number) {
  timers.set(id, window.setTimeout(() => dismiss(id), ms));
}
function hold(id: number) {
  clearTimeout(timers.get(id));
}
function resume(id: number) {
  if (timers.has(id)) later(id, 3000);
}

const FRIENDLY: Record<string, Key> = {
  locked: "ui.error.locked",
  wrongPassword: "ui.error.wrongPassword",
  network: "ui.error.network",
  notFound: "ui.error.notFound",
  crypto: "ui.error.crypto",
  storage: "ui.error.storage",
};

/** User-facing text for an IPC error. Backend messages are already safe and user-facing. */
export function friendly(e: ErrorPayload): string {
  const k = FRIENDLY[e.kind];
  return k ? t(k) : e.message || t("ui.error.generic");
}

/** Error toast; a cancelled action (closed window, dismissed Windows Hello) stays quiet. */
export function toastError(e: unknown) {
  const err = asError(e);
  if (err.kind !== "cancelled") toast(friendly(err), "error");
}

/** The modal dialog on top, if any. A modal makes everything outside it inert, so the toasts move
 * into it while it is open (they would sit dimmed under it, out of reach, otherwise). */
function useTopModal() {
  const [top, setTop] = useState<HTMLDialogElement | null>(null);
  useEffect(() => {
    let stack = [...document.querySelectorAll<HTMLDialogElement>("dialog:modal")];
    setTop(stack.at(-1) ?? null);
    const mo = new MutationObserver((records) => {
      for (const { target } of records) {
        if (!(target instanceof HTMLDialogElement)) continue;
        stack = stack.filter((d) => d !== target);
        if (target.matches(":modal")) stack.push(target);
      }
      stack = stack.filter((d) => d.isConnected && d.matches(":modal"));
      setTop(stack.at(-1) ?? null);
    });
    mo.observe(document.body, { subtree: true, attributes: true, attributeFilter: ["open"] });
    return () => mo.disconnect();
  }, []);
  return top;
}

export function Toaster() {
  const list = useSyncExternalStore(subscribe, () => items);
  const reduced = useReducedMotionFlag();
  const away = (isRtl() ? -1 : 1) * 16;
  const top = useTopModal();
  const box = useRef<HTMLDivElement>(null);
  const newest = list.at(-1)?.id;
  // In the top layer, and raised again for each new toast: above whatever opened since (a menu).
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    if (el.matches(":popover-open")) el.hidePopover();
    el.showPopover();
  }, [top, newest]);
  // Each toast is its own status or alert region: no live region around them (read twice).
  return createPortal(
    <div ref={box} popover="manual" data-passive className="toaster flex flex-col items-end gap-2 pointer-events-none" data-tauri-drag-region="false">
      <AnimatePresence initial={false}>
        {list.map((item) => (
          <motion.div
            key={item.id}
            layout={reduced ? false : "position"}
            role={item.tone === "error" ? "alert" : "status"}
            className={`toast ${item.tone === "error" ? "toast-error" : ""}`}
            // It rises as a squashed drop and wobbles to rest; it leaves as a drop that shrinks away.
            initial={{ opacity: 0, y: reduced ? 0 : 28, scaleX: reduced ? 1 : 0.7, scaleY: reduced ? 1 : 0.85 }}
            animate={{ opacity: 1, y: 0, scaleX: 1, scaleY: 1, transition: { ...SPRING.snappy, bounce: 0.45, visualDuration: 0.42, opacity: TWEEN.fade } }}
            exit={{ opacity: 0, x: reduced ? 0 : away, scale: reduced ? 1 : 0.6, transition: TWEEN.exit }}
            onPointerEnter={() => hold(item.id)}
            onPointerLeave={() => resume(item.id)}
            onFocus={() => hold(item.id)}
            onBlur={() => resume(item.id)}
          >
            <span aria-hidden="true" className="toast-mark" />
            <span className="flex-1 min-w-0 break-words">{item.text}</span>
            <IconButton icon="x" size="sm" label={t("common.dismiss")} onClick={() => dismiss(item.id)} />
          </motion.div>
        ))}
      </AnimatePresence>
    </div>,
    top ?? document.body,
  );
}
