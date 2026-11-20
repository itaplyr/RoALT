import { useEffect, useState } from "react";
import { t } from "../../i18n";
import { readStored, writeStored } from "../../ui/stored";

/** Live preview frame rates, 0 for Off (a still picture now and then); the choice is remembered on
 * this PC and shared by the Dashboard and the Performance tab. */
export const PREVIEW_FPS = [0, 1, 5, 15, 30, 60] as const;
export type PreviewFps = (typeof PREVIEW_FPS)[number];
const KEY = "roalt.previewFps";
const CHANGED = "roalt:previewFps";

export function storedFps(): PreviewFps {
  const s = readStored(KEY);
  // Nothing stored isn't Off.
  const n = s ? Number(s) : NaN;
  return PREVIEW_FPS.find((f) => f === n) ?? 15;
}

export function setPreviewFps(f: PreviewFps) {
  writeStored(KEY, String(f));
  window.dispatchEvent(new Event(CHANGED));
}

/** A preview rate in words: "Off" for 0. */
export const fpsLabel = (fps: number) => (fps ? t("dashboard.fps", { fps }) : t("common.off"));

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
