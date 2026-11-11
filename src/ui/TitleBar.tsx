import { useEffect, useMemo, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { CaptionGlyph } from "./Icon";
import { t } from "../i18n";

/** The custom title bar (window decorations are off): the app menu, the island in the middle
 * (`islandSlot` receives the element it is portalled into), the tools, and Windows' caption keys.
 * Empty space drags the window and a double click maximizes. Left to right in every language: the
 * window keys stay where Windows has them. */
export function TitleBar({ start, tools, islandSlot }: { start: ReactNode; tools: ReactNode; islandSlot: (el: HTMLDivElement | null) => void }) {
  const win = useMemo(() => getCurrentWindow(), []);
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const sync = () => win.isMaximized().then(setMaximized, () => {});
    sync();
    const un = win.onResized(sync);
    return () => void un.then((f) => f());
  }, [win]);

  return (
    <header className="titlebar" dir="ltr" data-tauri-drag-region="deep">
      <div className="titlebar-start">{start}</div>
      <div className="titlebar-center" ref={islandSlot} />
      <div className="titlebar-end">
        {tools}
        <button type="button" className="caption-btn" aria-label={t("ui.minimize")} onClick={() => win.minimize()}>
          <CaptionGlyph name="minimize" />
        </button>
        <button type="button" className="caption-btn" aria-label={maximized ? t("ui.restore") : t("ui.maximize")} onClick={() => win.toggleMaximize()}>
          <CaptionGlyph name={maximized ? "restore" : "maximize"} />
        </button>
        <button type="button" className="caption-btn caption-close" aria-label={t("ui.closeWindow")} onClick={() => win.close()}>
          <CaptionGlyph name="close" />
        </button>
      </div>
    </header>
  );
}
