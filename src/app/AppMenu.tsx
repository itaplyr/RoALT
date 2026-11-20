import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { VaultMode } from "../api";
import { useOnline, useSettings } from "../data";
import { fmtNum, t } from "../i18n";
import { DISCORD_INVITE } from "../features/donate/links";
import { Icon, Logo } from "../ui/Icon";
import { MenuItem, MenuSeparator, Popover, useAnchor } from "../ui/Popover";
import { toastError } from "../ui/Toast";
import { KillAllButton } from "./KillAll";
import { requestNavigation } from "./navigation";

/** The logo and name, as the app's own menu: the vault's state, the community, Donate, and the keys
 * that used to sit in the sidebar's foot. While locked, only what needs no vault. Beside it, how
 * many use RoALT now while that's on, then a Discord button: the community one click away. */
export function AppMenu({ unlocked, vaultMode, onRefresh, onDonate }: {
  unlocked: boolean;
  vaultMode?: VaultMode;
  onRefresh: () => void;
  onDonate: () => void;
}) {
  const anchor = useAnchor();
  const [open, setOpen] = useState(false);
  const counting = useSettings().data?.onlineCount ?? false;
  const online = useOnline(counting).data;
  // The cache outlives a switch turned off; the setting decides.
  const n = counting ? online?.online ?? null : null;
  const playing = n === null ? null : online?.playing ?? null;
  const compact = (v: number) => fmtNum(v, { notation: "compact", maximumFractionDigits: 1 });
  const counts = [n !== null && t("app.onlineNow", { count: n }), playing !== null && t("app.playingNow", { count: playing })].filter(Boolean).join(" · ");
  const id = "app-menu";
  return (
    <>
      <button type="button" className="app-menu-btn" popoverTarget={id} style={anchor.style} aria-label="RoALT" aria-haspopup="menu" aria-expanded={open}>
        <Logo />
        <span className="wordmark">RoALT</span>
        <Icon name="chevron" size={12} className="app-menu-chevron" />
      </button>
      {/* RoALT's own counts, beside its logo and apart from Discord, so they don't read as Discord's. */}
      {n !== null && (
        <>
          <span className="online-count" role="img" aria-label={counts} data-tip={counts}>
            <span className="online-dot" />
            {compact(n)}
            {playing !== null && (
              <>
                <Icon name="play" size={10} className="online-playing" />
                {compact(playing)}
              </>
            )}
          </span>
          <span className="title-sep online-count-sep" aria-hidden="true" />
        </>
      )}
      <button type="button" className="wc" aria-label={t("app.joinDiscord")} data-tip={t("app.joinDiscord")} onClick={() => openUrl(DISCORD_INVITE).catch(toastError)}>
        <Icon name="discord" size={16} />
      </button>
      <Popover id={id} anchor={anchor.name} placement="below-start" role="menu" label="RoALT" onToggle={setOpen} className="w-[260px]">
        <div className="flex items-center gap-2 px-2.5 pt-1.5 pb-2 text-[13px] text-fg-3" title={!unlocked ? t("vault.title") : vaultMode === "master" ? t("app.vaultUnlockedMaster") : t("app.vaultUnlockedDpapi")}>
          <Icon name="lock" size={14} />
          {t("app.encrypted")}
        </div>
        <MenuSeparator />
        {unlocked && (
          <MenuItem icon="refresh" kbd="F5" onSelect={onRefresh}>
            {t("ui.refresh")}
          </MenuItem>
        )}
        {unlocked && (
          <MenuItem icon="keyboard" onSelect={() => requestNavigation("settings", "shortcuts")}>
            {t("settings.section.shortcuts")}
          </MenuItem>
        )}
        <MenuItem icon="chat" onSelect={() => openUrl(DISCORD_INVITE).catch(toastError)}>
          {t("app.joinDiscord")}
        </MenuItem>
        <MenuItem icon="heart" onSelect={onDonate}>
          {t("donate.button")}
        </MenuItem>
        {unlocked && (
          <>
            <MenuSeparator />
            <div className="p-1" role="none">
              <KillAllButton role="menuitem" />
            </div>
          </>
        )}
      </Popover>
    </>
  );
}
