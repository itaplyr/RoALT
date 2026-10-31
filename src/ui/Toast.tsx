import { useSyncExternalStore } from "react";
import { asError, type ErrorPayload } from "../api";
import { Icon } from "./Icon";
import { t, type Key } from "../i18n";

type Tone = "ok" | "error";
type Item = { id: number; text: string; tone: Tone; leaving?: boolean };

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

function dismiss(id: number) {
  publish(items.map((t) => (t.id === id ? { ...t, leaving: true } : t)));
  setTimeout(() => publish(items.filter((t) => t.id !== id)), 200);
}

/** Shows a short message bottom-right. Callable from anywhere, including outside React. A sticky
 *  one stays until dismissed. */
export function toast(text: string, tone: Tone = "ok", sticky = false) {
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

export function Toaster() {
  const list = useSyncExternalStore(subscribe, () => items);
  return (
    <div className="fixed end-4 bottom-4 z-50 flex flex-col items-end gap-2 pointer-events-none" aria-live="polite">
      {list.map((item) => (
        <div
          key={item.id}
          role={item.tone === "error" ? "alert" : "status"}
          className={`toast enter ${item.tone === "error" ? "toast-error" : ""} ${item.leaving ? "toast-leave" : ""}`}
          onPointerEnter={() => hold(item.id)}
          onPointerLeave={() => resume(item.id)}
          onFocus={() => hold(item.id)}
          onBlur={() => resume(item.id)}
        >
          <span className="flex-1 min-w-0 pt-0.5 break-words">{item.text}</span>
          <button className="wc w-6 h-6" aria-label={t("common.dismiss")} onClick={() => dismiss(item.id)}>
            <Icon name="x" size={12} />
          </button>
        </div>
      ))}
    </div>
  );
}
