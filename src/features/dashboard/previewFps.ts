import { useEffect, useState } from "react";

/** Live preview frame rates; the choice is remembered on this PC and shared by the Dashboard and
 * the Performance tab. */
export const PREVIEW_FPS = [5, 15, 30, 60] as const;
export type PreviewFps = (typeof PREVIEW_FPS)[number];
const KEY = "roalt.previewFps";
const CHANGED = "roalt:previewFps";
/** When this PC keeps no storage, the choice lasts until RoALT closes. */
let unsaved: PreviewFps | null = null;

export function storedFps(): PreviewFps {
  try {
    const n = Number(localStorage.getItem(KEY));
    return PREVIEW_FPS.find((f) => f === n) ?? unsaved ?? 15;
  } catch {
    return unsaved ?? 15;
  }
}

export function setPreviewFps(f: PreviewFps) {
  unsaved = f;
  try {
    localStorage.setItem(KEY, String(f));
  } catch {
    // No storage: kept in `unsaved`.
  }
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
