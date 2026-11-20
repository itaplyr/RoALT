import { api } from "../../api";
import type { ThumbPlace } from "../../bindings/ThumbPlace";
import { overlayOpen } from "../../ui/a11y";

/** Frame-free previews, where live ones would get Windows' yellow frame (`instances::thumbs`):
 * Windows draws each client over its card's picture box, on top of everything in RoALT's window.
 * One loop places them all, each frame while something may be moving (`BURST_MS` after a scroll, a
 * pointer, a key, a transition or a dialog) and twice a second anyway (the client's window may have
 * changed): an idle Overview costs no work each frame. None while a dialog or menu is open, for a box the view's edge cuts or
 * a toast covers; a dim one while the card's keys or pill sit over the picture. */
type Card = { box: HTMLElement; view: HTMLElement | null; dim: () => boolean };
const cards = new Map<string, Card>();
let frame = 0;
let timer = 0;
let sent = "";
let sentAt = 0;
let movedAt = 0;
/** Sent again at least this often. */
const REFRESH_MS = 500;
/** Placed each frame for this long after something that may move a card. */
const BURST_MS = 700;
const WAKES = ["scroll", "wheel", "resize", "pointerover", "pointerout", "pointerdown", "pointermove", "focusin", "focusout", "keydown", "transitionrun", "animationstart", "toggle"];
/** Dialogs opening, as their `open` attribute appears. */
const opened = new MutationObserver(() => wake());
/** A card or its scroller changing size moves the cards after it. */
const resized = new ResizeObserver(() => wake());

/** The nearest ancestor that scrolls: a box must be inside it to be drawn. */
function scroller(el: HTMLElement) {
  for (let p = el.parentElement; p; p = p.parentElement) {
    if (/(auto|scroll)/.test(getComputedStyle(p).overflowY)) return p;
  }
  return null;
}

const within = (r: DOMRect, v: { top: number; left: number; right: number; bottom: number }) =>
  r.top >= v.top - 0.5 && r.left >= v.left - 0.5 && r.bottom <= v.bottom + 0.5 && r.right <= v.right + 0.5;
const overlaps = (a: DOMRect, b: DOMRect) => a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;

function tick() {
  const places: ThumbPlace[] = [];
  if (document.visibilityState === "visible" && !overlayOpen()) {
    const toasts = document.querySelector<HTMLElement>(".toaster")?.getBoundingClientRect();
    const screen = { top: 0, left: 0, right: innerWidth, bottom: innerHeight };
    for (const [id, c] of cards) {
      const r = c.box.getBoundingClientRect();
      const inView = within(r, screen) && (!c.view || within(r, c.view.getBoundingClientRect()));
      if (r.width >= 1 && r.height >= 1 && inView && !(toasts?.height && overlaps(r, toasts))) {
        places.push({ id, x: r.left, y: r.top, width: r.width, height: r.height, dim: c.dim() });
      }
    }
  }
  const now = performance.now();
  const key = JSON.stringify(places);
  if (key !== sent || now - sentAt >= REFRESH_MS) {
    sent = key;
    sentAt = now;
    void api.instanceThumbs(places).catch(() => {});
  }
  frame = 0;
  // The last card gone: the empty list above took every picture away.
  if (!cards.size) return stop();
  if (now - movedAt < BURST_MS) frame = requestAnimationFrame(tick);
  else timer = window.setTimeout(tick, REFRESH_MS);
}

/** Something may move a card: placed each frame for a while. */
function wake() {
  movedAt = performance.now();
  if (frame) return;
  clearTimeout(timer);
  frame = requestAnimationFrame(tick);
}

function start() {
  for (const w of WAKES) window.addEventListener(w, wake, { capture: true, passive: true });
  opened.observe(document.body, { subtree: true, attributeFilter: ["open"] });
  wake();
}

function stop() {
  for (const w of WAKES) window.removeEventListener(w, wake, { capture: true });
  opened.disconnect();
  resized.disconnect();
  cancelAnimationFrame(frame);
  clearTimeout(timer);
  frame = timer = 0;
}

/** Draws client `id` over `box` until the returned function is called. `dim`: something of the
 * card's own is over the picture now. */
export function showWindowThumb(id: string, box: HTMLElement, dim: () => boolean) {
  const first = !cards.size;
  const view = scroller(box);
  cards.set(id, { box, view, dim });
  if (first) start();
  else wake();
  resized.observe(box.closest(".tile") ?? box);
  if (view) resized.observe(view);
  return () => {
    if (cards.get(id)?.box === box) cards.delete(id);
    resized.unobserve(box.closest(".tile") ?? box);
    // The next tick sends the list without it, then stops once none is left.
    wake();
  };
}
