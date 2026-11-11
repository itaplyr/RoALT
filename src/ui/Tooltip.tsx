import { useEffect, useRef, type CSSProperties } from "react";
import { prefersReducedMotion } from "./motion";

/** The app's one tooltip, for anything with `data-tip` (icon buttons above all): after a short
 * delay on hover, at once on keyboard focus and while moving between triggers. `data-tip-kbd` adds a
 * shortcut, `data-tip-side="top"` puts it above. It is passive: names come from aria-label, and it
 * never counts as an open overlay (shortcuts keep working). */
export function TooltipHost() {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const tip = ref.current;
    if (!tip) return;
    const text = tip.querySelector<HTMLElement>("[data-text]")!;
    const kbd = tip.querySelector<HTMLElement>("[data-kbd]")!;
    let timer = 0;
    let current: HTMLElement | null = null;
    // The trigger wearing our anchor name (a popover's trigger keeps its own). Kept past hide(): the
    // fade-out still needs the anchor, else the tip jumps to the window's corner. The next show() takes it.
    let lentTo: HTMLElement | null = null;
    let warmUntil = 0;
    const open = () => tip.matches(":popover-open");
    const release = () => {
      lentTo?.style.removeProperty("anchor-name");
      lentTo = null;
    };

    const show = (el: HTMLElement) => {
      if (!el.isConnected || !el.dataset.tip) return;
      remember();
      release();
      current = el;
      const own = getComputedStyle(el).getPropertyValue("anchor-name").split(",")[0].trim();
      const lent = !own || own === "none";
      if (lent) {
        el.style.setProperty("anchor-name", "--tip");
        lentTo = el;
      }
      tip.style.setProperty("position-anchor", lent ? "--tip" : own);
      tip.dataset.side = el.dataset.tipSide ?? "bottom";
      text.textContent = el.dataset.tip;
      kbd.textContent = el.dataset.tipKbd ?? "";
      kbd.hidden = !el.dataset.tipKbd;
      if (!open()) tip.showPopover();
      flow();
    };
    // Moving from trigger to trigger, the tip flows from where it was instead of blinking there.
    let lastRect: DOMRect | null = null;
    let lastAt = 0;
    const flow = () => {
      const from = lastRect;
      if (!from || performance.now() - lastAt > 300 || prefersReducedMotion()) return;
      const to = tip.getBoundingClientRect();
      if (!to.width) return;
      const dx = from.left + from.width / 2 - (to.left + to.width / 2);
      const dy = from.top + from.height / 2 - (to.top + to.height / 2);
      if (Math.abs(dx) < 1 && Math.abs(dy) < 1) return;
      const spring = getComputedStyle(document.documentElement).getPropertyValue("--spring").trim() || "ease-out";
      tip.animate([{ transform: `translate(${dx}px, ${dy}px) scaleX(${(from.width / to.width).toFixed(3)})` }, { transform: "none" }], { duration: 420, easing: spring });
    };
    const remember = () => {
      if (!open()) return;
      lastRect = tip.getBoundingClientRect();
      lastAt = performance.now();
    };
    const hide = () => {
      clearTimeout(timer);
      current = null;
      if (open()) {
        remember();
        tip.hidePopover();
        warmUntil = performance.now() + 600;
      }
    };
    const later = (el: HTMLElement) => {
      clearTimeout(timer);
      const warm = open() || performance.now() < warmUntil;
      timer = window.setTimeout(() => show(el), warm ? 0 : 450);
    };

    const target = (e: Event) => (e.target instanceof Element ? e.target.closest<HTMLElement>("[data-tip]") : null);
    const over = (e: PointerEvent) => {
      const el = target(e);
      if (el && el !== current) later(el);
    };
    const out = (e: PointerEvent) => {
      const el = target(e);
      if (!el) return;
      const to = e.relatedTarget instanceof Element ? e.relatedTarget.closest("[data-tip]") : null;
      if (to !== el) hide();
    };
    const focus = (e: FocusEvent) => {
      const el = target(e);
      if (el?.matches(":focus-visible")) show(el);
    };
    const blur = (e: FocusEvent) => target(e) === current && hide();
    const key = (e: KeyboardEvent) => e.key === "Escape" && hide();

    document.addEventListener("pointerover", over);
    document.addEventListener("pointerout", out);
    document.addEventListener("pointerdown", hide, true);
    document.addEventListener("focusin", focus);
    document.addEventListener("focusout", blur);
    document.addEventListener("keydown", key, true);
    document.addEventListener("scroll", hide, true);
    return () => {
      hide();
      release();
      document.removeEventListener("pointerover", over);
      document.removeEventListener("pointerout", out);
      document.removeEventListener("pointerdown", hide, true);
      document.removeEventListener("focusin", focus);
      document.removeEventListener("focusout", blur);
      document.removeEventListener("keydown", key, true);
      document.removeEventListener("scroll", hide, true);
    };
  }, []);
  return (
    <div ref={ref} popover="manual" className="tooltip" data-passive aria-hidden="true" style={{ positionAnchor: "--tip" } as CSSProperties}>
      <span data-text />
      <span data-kbd className="kbd" hidden />
    </div>
  );
}
