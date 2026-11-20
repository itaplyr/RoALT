import { useEffect, useState } from "react";
import { readStored, writeStored } from "../../ui/stored";

/** Live preview frame rates; the choice is remembered on this PC and shared by the Dashboard and
 * the Performance tab. */
export const PREVIEW_FPS = [5, 15, 30, 60] as const;
export type PreviewFps = (typeof PREVIEW_FPS)[number];
const KEY = "roalt.previewFps";
const CHANGED = "roalt:previewFps";

export function storedFps(): PreviewFps {
  const n = Number(readStored(KEY));
  return PREVIEW_FPS.find((f) => f === n) ?? 15;
}

export function setPreviewFps(f: PreviewFps) {
  writeStored(KEY, String(f));
  window.dispatchEvent(new Event(CHANGED));
}

/** The preview rate, following changes made anywhere in RoALT. */
export function usePreviewFps(): [PreviewFps, (f: PreviewFps) => void] {
  const [fps, setFps] = useState(storedFps);
  useEffect(() => {
    const changed = () => setFps(storedFps());
    window.addEventListener(CHANGED, changed);
    return () => window.removeEventListener(CHANGED, changed);
  }, []);
  return [fps, setPreviewFps];
}
