import { useEffect, useState, type RefObject } from "react";
import { api } from "../../api";
import type { PreviewFps } from "./previewFps";

/** How often a hidden RoALT window looks again whether it is shown. */
const HIDDEN_MS = 500;

/** All cards together ask for at most this many pictures a second.
 * ponytail: a guess, tune it from measurements with many clients. */
const PICTURES_PER_SECOND = 60;
/** A picture counts as old from this age, and only after asking this long without a break. */
const STALE_MS = 3000;
const STATUSES = ["live", "minimized", "noWindow", "pending", "unsupported"] as const;
export type PictureStatus = (typeof STATUSES)[number];
/** The cards scrolled into view, sharing `PICTURES_PER_SECOND`. */
let cardsInView = 0;

/** Draws the client's window on `canvas` up to `fps` times a second while the RoALT window is
 * visible, one request in flight at a time; a request that gets no new picture changes nothing on
 * the canvas. `status`: what the client's window is doing (`window::Status`), null before the first
 * answer; `painted`: the canvas holds a picture, maybe an old one; `ageS`: seconds since its picture, when it is live and that has gone on too long. */
export function useThumbnail(id: string, canvas: RefObject<HTMLCanvasElement | null>, fps: PreviewFps) {
  const [status, setStatus] = useState<PictureStatus | null>(null);
  const [painted, setPainted] = useState(false);
  const [ageS, setAgeS] = useState<number | null>(null);
  useEffect(() => {
    let stop = false;
    let timer: number | undefined;
    let known = 0;
    // Nothing was asked for while out of sight: a picture isn't old until asked for a while again.
    let wasVisible = false;
    let askingSince = 0;
    let wasLive = false;
    // A card scrolled out of sight costs no pictures.
    let inView = true;
    cardsInView++;
    const seen = new IntersectionObserver((entries) => {
      const now = entries[entries.length - 1].isIntersecting;
      if (now !== inView) cardsInView += now ? 1 : -1;
      inView = now;
    });
    if (canvas.current) seen.observe(canvas.current);
    const tick = async () => {
      const started = performance.now();
      const el = canvas.current;
      const visible = document.visibilityState === "visible" && inView;
      if (visible && !wasVisible) askingSince = started;
      wasVisible = visible;
      if (el && visible) {
        try {
          const buf = await api.instanceThumbnail(id, Math.min(960, Math.max(320, Math.round(el.clientWidth * devicePixelRatio))), known);
          if (stop) return;
          const head = new DataView(buf);
          const [w, h, code, age] = [head.getUint32(0, true), head.getUint32(4, true), head.getUint32(8, true), head.getUint32(20, true)];
          if (buf.byteLength > 24) {
            // Resizing clears the canvas and costs a reallocation: only when the size changes.
            if (el.width !== w) el.width = w;
            if (el.height !== h) el.height = h;
            el.getContext("2d")?.putImageData(new ImageData(new Uint8ClampedArray(buf, 24, w * h * 4), w, h), 0, 0);
            known = head.getUint32(16, true);
            setPainted(true);
          }
          setStatus(STATUSES[code]);
          // Back from minimized or no window: no picture came meanwhile, as when out of sight.
          const live = STATUSES[code] === "live";
          if (live && !wasLive) askingSince = performance.now();
          wasLive = live;
          setAgeS(live && age >= STALE_MS && performance.now() - askingSince >= STALE_MS ? Math.floor(age / 1000) : null);
        } catch {
          // Ended or not started: the card goes with the next instances list.
        }
      }
      const delay = Math.max(1000 / fps, (cardsInView * 1000) / PICTURES_PER_SECOND);
      if (!stop) timer = window.setTimeout(tick, visible ? Math.max(0, delay - (performance.now() - started)) : HIDDEN_MS);
    };
    void tick();
    return () => {
      stop = true;
      clearTimeout(timer);
      seen.disconnect();
      if (inView) cardsInView--;
    };
  }, [id, canvas, fps]);
  return { status, painted, ageS };
}
