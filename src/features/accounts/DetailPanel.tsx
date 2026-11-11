import { useId, useRef } from "react";
import { motion } from "motion/react";
import type { AccountSummary, Group, Instance, VaultMode } from "../../api";
import { useAccountInfo } from "../../data";
import { t } from "../../i18n";
import { IconButton } from "../../ui/Button";
import { EASE } from "../../ui/motion";
import { Tabs } from "../../ui/Tabs";
import { DetailCookie } from "./DetailCookie";
import { DetailOverrides } from "./DetailOverrides";
import { clip, DetailOverview } from "./DetailOverview";
import { accountName, Avatar } from "./shared";

export type DetailTab = "overview" | "overrides" | "cookie";

/** One account: who it is (and the copies of its names) on top, then Overview, Overrides and Cookie.
 * Only the open tab is mounted. The parent keys this by account, so nothing typed or revealed for
 * one account carries over to the next. */
export function DetailPanel({ account: a, groups, vaultMode, clipboardSeconds, checking, instance, tab, onTab, onValidate, onRemove, onLaunch, onMenu, menuOpen = false, onClose }: {
  account: AccountSummary;
  groups: Group[];
  vaultMode: VaultMode;
  clipboardSeconds: number;
  checking: boolean;
  instance?: Instance;
  tab: DetailTab;
  onTab: (tab: DetailTab) => void;
  onValidate: () => void;
  onRemove: () => void;
  onLaunch: () => void;
  /** Opens the account's menu under its "…" button. */
  onMenu: (from: HTMLElement) => void;
  /** That menu is open. */
  menuOpen?: boolean;
  /** Shown as a sheet over the list: its close button. */
  onClose?: () => void;
}) {
  const info = useAccountInfo(a.id);
  // Only info that belongs to this Roblox user (local ids can be reused after a removal).
  const d = info.data?.userId === a.userId ? info.data : undefined;
  const username = d?.username || a.username;
  const panelId = useId();
  const body = useRef<HTMLDivElement>(null);
  const tabs: { value: DetailTab; label: string }[] = [
    { value: "overview", label: t("dashboard.tabs.overview") },
    { value: "overrides", label: t("accounts.detail.overrides") },
    { value: "cookie", label: t("accounts.detail.cookie") },
  ];

  return (
    <div className="flex-1 min-h-0 flex flex-col">
      <header className="flex items-start gap-3.5 ps-5 pe-3 pt-5 pb-4">
        <Avatar account={a} size={56} url={d?.headshotUrl} />
        <div className="min-w-0 flex-1 flex flex-col pt-0.5">
          <h2 className="acc-detail-name truncate">{accountName(a)}</h2>
          <span className="flex items-center gap-0.5 min-w-0 text-fg-2">
            <span className="mono text-[13px] truncate selectable">@{username}</span>
            <IconButton icon="copy" size="sm" iconSize={13} label={t("accounts.menu.copyUsername")} onClick={() => clip(username, t("accounts.menu.usernameCopied"))} />
          </span>
          <span className="flex items-center gap-0.5 min-w-0 text-fg-3 -mt-1">
            <span className="mono text-[12px] truncate selectable">{t("accounts.detail.userId", { id: a.userId })}</span>
            <IconButton icon="copy" size="sm" iconSize={13} label={t("accounts.menu.copyUserId")} onClick={() => clip(String(a.userId), t("accounts.menu.userIdCopied"))} />
          </span>
        </div>
        <span className="flex-none flex -mt-1">
          <IconButton icon="dots" label={t("accounts.menu.actionsFor", { name: accountName(a) })} aria-haspopup="menu" aria-expanded={menuOpen} onClick={(e) => onMenu(e.currentTarget)} />
          {onClose && <IconButton icon="x" label={t("common.close")} shortcut="Esc" onClick={onClose} />}
        </span>
      </header>
      <div className="px-5 pb-4">
        <Tabs
          label={t("accounts.details")}
          value={tab}
          items={tabs}
          panelId={panelId}
          className="acc-tabs"
          onChange={(next) => {
            body.current?.scrollTo({ top: 0, behavior: "instant" });
            onTab(next);
          }}
        />
      </div>
      <div ref={body} id={panelId} role="tabpanel" aria-label={tabs.find((x) => x.value === tab)?.label} className="scroll flex-1 min-h-0 border-t border-line">
        {/* The new tab's content fades in; the old one is gone at once (a revealed cookie with it). */}
        <motion.div key={tab} initial={{ opacity: 0 }} animate={{ opacity: 1, transition: { duration: 0.12, ease: EASE.out } }}>
          {tab === "overview" ? (
            <DetailOverview account={a} groups={groups} checking={checking} instance={instance} onValidate={onValidate} onLaunch={onLaunch} onRemove={onRemove} />
          ) : tab === "overrides" ? (
            <DetailOverrides account={a} />
          ) : (
            <DetailCookie account={a} vaultMode={vaultMode} clipboardSeconds={clipboardSeconds} />
          )}
        </motion.div>
      </div>
    </div>
  );
}
