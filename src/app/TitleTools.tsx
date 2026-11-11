import type { VaultMode } from "../api";
import { t } from "../i18n";
import { IconButton } from "../ui/Button";
import { Icon } from "../ui/Icon";
import { IconSwap } from "../ui/motion";

/** The title bar's own keys: the layout, the theme, and the vault (a lock key with a master password,
 * else the encryption's state). */
export function TitleTools({ dark, onToggleTheme, sideTabs, onToggleTabs, onLock, vaultMode }: {
  dark: boolean;
  onToggleTheme?: () => void;
  sideTabs?: boolean;
  onToggleTabs?: () => void;
  onLock?: () => void;
  vaultMode?: VaultMode;
}) {
  return (
    <div className="title-tools">
      {onToggleTabs && (
        <IconButton icon={sideTabs ? "layoutTop" : "layoutSide"} label={sideTabs ? t("ui.accountsTop") : t("ui.accountsSide")} onClick={onToggleTabs} />
      )}
      {onToggleTheme && (
        <button type="button" className="wc" aria-label={dark ? t("ui.lightTheme") : t("ui.darkTheme")} data-tip={dark ? t("ui.lightTheme") : t("ui.darkTheme")} onClick={onToggleTheme}>
          <IconSwap name={dark ? "sun" : "moon"} />
        </button>
      )}
      {onLock ? (
        <IconButton icon="lock" label={t("ui.lockVault")} shortcut="Ctrl+L" onClick={onLock} />
      ) : (
        vaultMode === "dpapi" && (
          <span className="vault-glyph" role="img" aria-label={t("app.vaultUnlockedDpapi")} data-tip={t("app.encrypted")}>
            <Icon name="shield" />
          </span>
        )
      )}
    </div>
  );
}
