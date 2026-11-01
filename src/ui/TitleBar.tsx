import { useEffect, useMemo, useState } from "react";
import { useIsFetching } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Icon, Logo } from "./Icon";
import { t } from "../i18n";

/** Custom title bar (window decorations are off). Empty space drags; double-click maximizes. */
export function TitleBar({ dark, onToggleTheme, onLock, onRefresh, sideTabs, onToggleTabs }: {
  dark: boolean;
  onToggleTheme?: () => void;
  onLock?: () => void;
  onRefresh?: () => void;
  /** Accounts are on the side; `onToggleTabs` moves them. */
  sideTabs?: boolean;
  onToggleTabs?: () => void;
}) {
  const win = useMemo(() => getCurrentWindow(), []);
  const fetching = useIsFetching() > 0;
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const sync = () => win.isMaximized().then(setMaximized, () => {});
    sync();
    const un = win.onResized(sync);
    return () => void un.then((f) => f());
  }, [win]);

  return (
    // Left to right in every language: the window's keys stay where Windows has them.
    <header dir="ltr" data-tauri-drag-region="deep" className="h-11 flex-none flex items-center gap-2.5 ps-[18px]">
      <Logo />
      <span className="font-bold tracking-[0.02em]">RoALT</span>
      <div className="ms-auto flex items-center">
        {onRefresh && (
          <button className="wc" aria-label={t("ui.refresh")} title={t("ui.refreshKey", { key: "F5" })} onClick={onRefresh}>
            <Icon name="refresh" className={fetching ? "spin" : ""} />
          </button>
        )}
        {onToggleTabs && (
          <button className="wc" aria-label={sideTabs ? t("ui.accountsTop") : t("ui.accountsSide")} title={sideTabs ? t("ui.accountsTop") : t("ui.accountsSide")} onClick={onToggleTabs}>
            {/* What a click gives. */}
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" aria-hidden="true">
              <rect x="1.5" y="2.5" width="13" height="11" rx="2" />
              <path d={sideTabs ? "M1.5 6h13" : "M6 2.5v11"} />
            </svg>
          </button>
        )}
        {onToggleTheme && (
          <button className="wc" aria-label={dark ? t("ui.lightTheme") : t("ui.darkTheme")} title={t("ui.theme")} onClick={onToggleTheme}>
            <span key={dark ? "d" : "l"} className="pop grid">
              <Icon name={dark ? "sun" : "moon"} />
            </span>
          </button>
        )}
        {onLock && (
          <button className="wc" aria-label={t("ui.lockVault")} title={t("ui.lockVaultKey", { key: "Ctrl+L" })} onClick={onLock}>
            <Icon name="lock" />
          </button>
        )}
        <span className="w-px h-5 bg-line mx-1" aria-hidden="true" />
        <button className="wc" aria-label={t("ui.minimize")} onClick={() => win.minimize()}>
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5h10" stroke="currentColor" /></svg>
        </button>
        <button className="wc" aria-label={maximized ? t("ui.restore") : t("ui.maximize")} onClick={() => win.toggleMaximize()}>
          {maximized ? (
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" aria-hidden="true">
              <rect x="0.5" y="2.5" width="7" height="7" /><path d="M2.5 2.5V0.5h7v7h-2" />
            </svg>
          ) : (
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" />
            </svg>
          )}
        </button>
        <button className="wc wc-close" aria-label={t("ui.closeWindow")} onClick={() => win.close()}>
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" /></svg>
        </button>
      </div>
    </header>
  );
}
