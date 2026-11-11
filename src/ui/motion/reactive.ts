// The UI's reactive surfaces, wired once for the whole document (styles/motion.css draws them):
// - a press leaves ink where it landed on buttons, island tiles and chips;
// - a chip's fill blooms from the press point (--px/--py);
// - primary buttons lean toward the pointer (--lx/--ly);
// - a switch or checkbox that was pressed gets data-moved, so its stretch or jelly plays (not on load);
// - client cards light up under the pointer (--mx/--my);
// - the last press sets --vt-x/--vt-y on <html>: where the next page pours in from.
// All of it stands down under reduced motion (the CSS checks html[data-rm]; here, `calm()`).
import { prefersReducedMotion } from "./reduced";

const INK = ".btn, .chip, .island-item, .island-child, .seg, .menu-item";
const LEAN = ".btn-primary, .btn-play";
/** How far a primary button leans, in px, at its edges. */
const LEAN_X = 5;
const LEAN_Y = 3;

let installed = false;

/** Where `e` is inside `el`, in the element's own pixels (the window may be zoomed). */
function local(el: HTMLElement, e: PointerEvent) {
  const r = el.getBoundingClientRect();
  const s = el.offsetWidth ? r.width / el.offsetWidth : 1;
  return { x: (e.clientX - r.left) / s, y: (e.clientY - r.top) / s, w: el.offsetWidth, h: el.offsetHeight };
}

function ink(el: HTMLElement, e: PointerEvent) {
  const p = local(el, e);
  let box = el.querySelector<HTMLElement>(":scope > .lq-ink");
  if (!box) {
    box = document.createElement("span");
    box.className = "lq-ink";
    box.setAttribute("aria-hidden", "true");
    el.append(box);
  }
  const drop = document.createElement("span");
  drop.className = "lq-ripple";
  drop.style.left = `${p.x}px`;
  drop.style.top = `${p.y}px`;
  box.append(drop);
  drop.addEventListener("animationend", () => drop.remove(), { once: true });
  // Under Off nothing animates, so nothing would ever end it.
  setTimeout(() => drop.remove(), 900);
}

export function installReactive() {
  if (installed) return;
  installed = true;
  const root = document.documentElement;

  document.addEventListener(
    "pointerdown",
    (e) => {
      if (!(e.target instanceof Element) || e.button !== 0) return;
      root.style.setProperty("--vt-x", `${e.clientX}px`);
      root.style.setProperty("--vt-y", `${e.clientY}px`);
      if (prefersReducedMotion()) return;
      const chip = e.target.closest<HTMLElement>(".chip");
      if (chip) {
        const p = local(chip, e);
        chip.style.setProperty("--px", `${p.x}px`);
        chip.style.setProperty("--py", `${p.y}px`);
      }
      const hit = e.target.closest<HTMLElement>(INK);
      if (hit && !hit.matches(":disabled, [aria-disabled=true]")) ink(hit, e);
    },
    true,
  );

  // Marked on the press, not on load: their stretch and jelly play only for a real change.
  document.addEventListener(
    "click",
    (e) => {
      const el = e.target instanceof Element ? e.target.closest<HTMLElement>(".switch, .cb") : null;
      el?.setAttribute("data-moved", "");
    },
    true,
  );
  document.addEventListener(
    "keydown",
    (e) => {
      if (e.key !== " " && e.key !== "Enter") return;
      const el = e.target instanceof Element ? e.target.closest<HTMLElement>(".switch, .cb") : null;
      el?.setAttribute("data-moved", "");
    },
    true,
  );

  let leaning: HTMLElement | null = null;
  const unlean = () => {
    leaning?.style.removeProperty("--lx");
    leaning?.style.removeProperty("--ly");
    leaning = null;
  };
  document.addEventListener(
    "pointermove",
    (e) => {
      if (!(e.target instanceof Element) || e.pointerType !== "mouse") return;
      const btn = e.target.closest<HTMLElement>(LEAN);
      if (btn !== leaning) unlean();
      if (btn && !prefersReducedMotion() && !btn.matches(":disabled, [aria-disabled=true]")) {
        const p = local(btn, e);
        btn.style.setProperty("--lx", `${((p.x / p.w - 0.5) * 2 * LEAN_X).toFixed(1)}px`);
        btn.style.setProperty("--ly", `${((p.y / p.h - 0.5) * 2 * LEAN_Y).toFixed(1)}px`);
        leaning = btn;
      }
      const card = e.target.closest<HTMLElement>(".home-card");
      if (card) {
        const p = local(card, e);
        card.style.setProperty("--mx", `${p.x}px`);
        card.style.setProperty("--my", `${p.y}px`);
      }
    },
    { passive: true },
  );
  document.addEventListener("pointerleave", unlean);
  document.addEventListener("pointerout", (e) => {
    if (leaning && !(e.relatedTarget instanceof Node && leaning.contains(e.relatedTarget))) unlean();
  });
}
