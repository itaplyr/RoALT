/** Draws the eye to `el` (a setting jumped to, a tour's stop): an accent wash 0 → 14 % → 0 with a
 * ring (it must show on see-through panels too); reduced motion: the same tint, still, for 1.5 s.
 * Shadows, not a background: a filled card (a preset) keeps its own fill under the wash. */
export function flash(el: HTMLElement, reduced: boolean) {
  // Forced colours drop shadows: a system-coloured outline instead.
  if (matchMedia("(forced-colors: active)").matches) {
    const on = { outline: "2px solid Highlight", outlineOffset: "-2px" };
    el.animate(reduced ? [on, on] : [{ ...on, outlineColor: "transparent" }, { ...on, offset: 0.3 }, { ...on, outlineColor: "transparent" }], { duration: reduced ? 1500 : 900, easing: "ease-out" });
    return;
  }
  const lit = { boxShadow: "inset 0 0 0 1px color-mix(in srgb, var(--accent-text) 40%, transparent), inset 0 0 0 100vmax color-mix(in srgb, var(--accent-text) 14%, transparent)" };
  const clear = { boxShadow: "inset 0 0 0 1px transparent, inset 0 0 0 100vmax transparent" };
  el.animate(reduced ? [lit, lit] : [clear, { ...lit, offset: 0.3 }, clear], { duration: reduced ? 1500 : 900, easing: "ease-out" });
}
