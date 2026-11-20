import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type AccountPatch, type AccountSummary, type Instance } from "../../api";
import { browseAs } from "../../app/navigation";
import { useUpdateAccount } from "../../data";
import { t } from "../../i18n";
import { ContextMenu, MenuCheckItem, MenuItem, MenuLabel, MenuSeparator } from "../../ui/Popover";
import { toastError } from "../../ui/Toast";
import { clip } from "./DetailOverview";
import { accountName } from "./shared";

/** Where an account's menu opens: at the pointer, or under its "…" button; focus goes back to `from`.
 * `solo`: about this account only, whatever is ticked (the detail panel's menu). */
export type MenuAt = { id: number; x: number; y: number; from: HTMLElement; solo?: boolean };

/** An account's menu in two labelled parts: what applies to the ticked rows (when this one is among
 * several), then what applies to this account alone. */
export function AccountMenu({ at, account: a, selection, byId, live, canLaunch, onClose, onLaunch, onValidate, onRemove }: {
  at: MenuAt | null;
  /** The account `at` points at, as it is now (the checkboxes follow its edits). */
  account: AccountSummary | undefined;
  /** Ticked account ids. */
  selection: number[];
  /** Every account by id: what the ticked ones are now. */
  byId: Map<number, AccountSummary>;
  live: Map<number, Instance>;
  canLaunch: (id: number) => boolean;
  onClose: () => void;
  onLaunch: (ids: number[]) => void;
  onValidate: (ids: number[]) => void;
  onRemove: (ids: number[]) => void;
}) {
  const update = useUpdateAccount();
  const ids = a && !at?.solo && selection.length > 1 && selection.includes(a.id) ? selection : a ? [a.id] : [];
  const many = ids.length > 1;
  const save = (patch: AccountPatch) => a && update.mutate({ id: a.id, patch });
  // With several ticked, Favourite and Bring back are theirs: ticked when all of them are so.
  const ticked = ids.flatMap((id) => byId.get(id) ?? []);
  const saveAll = (patch: AccountPatch) => ids.forEach((id) => update.mutate({ id, patch }));
  const inst = a && live.get(a.id);

  return (
    <ContextMenu at={a ? at : null} label={a ? t("accounts.menu.actionsFor", { name: accountName(a) }) : t("accounts.menu.actions")} onClose={onClose}>
      {a && (
        <>
          {many && (
            <>
              <MenuLabel>{t("accounts.selected", { count: ids.length })}</MenuLabel>
              {ids.some(canLaunch) && <MenuItem icon="play" onSelect={() => onLaunch(ids)}>{t("common.playEllipsis")}</MenuItem>}
              <MenuItem icon="refresh" onSelect={() => onValidate(ids)}>{t("accounts.menu.validateMany", { count: ids.length })}</MenuItem>
              <MenuCheckItem checked={ticked.every((x) => x.favourite)} onChange={(favourite) => saveAll({ favourite })}>{t("accounts.detail.favourite")}</MenuCheckItem>
              <MenuCheckItem checked={ticked.every((x) => !x.allowDisconnect)} onChange={(back) => saveAll({ allowDisconnect: !back })}>{t("accounts.detail.bringBack")}</MenuCheckItem>
              <MenuItem icon="trash" danger onSelect={() => onRemove(ids)}>{t("accounts.menu.removeMany", { count: ids.length })}</MenuItem>
              <MenuSeparator />
            </>
          )}
          <MenuLabel>@{a.username}</MenuLabel>
          {inst ? (
            <MenuItem icon="monitor" onSelect={() => api.instanceShow(inst.id, "focus").catch(toastError)}>{t("home.show")}</MenuItem>
          ) : (
            !many && canLaunch(a.id) && <MenuItem icon="play" onSelect={() => onLaunch([a.id])}>{t("common.playEllipsis")}</MenuItem>
          )}
          {!many && <MenuItem icon="refresh" onSelect={() => onValidate([a.id])}>{t("accounts.menu.validate")}</MenuItem>}
          <MenuItem icon="globe" onSelect={() => browseAs(a.id)}>{t("accounts.menu.openSession")}</MenuItem>
          <MenuItem icon="external" onSelect={() => openUrl(`https://www.roblox.com/users/${a.userId}/profile`).catch(toastError)}>{t("accounts.menu.openProfile")}</MenuItem>
          <MenuItem icon="copy" onSelect={() => clip(a.username, t("accounts.menu.usernameCopied"))}>{t("accounts.menu.copyUsername")}</MenuItem>
          <MenuItem icon="copy" onSelect={() => clip(String(a.userId), t("accounts.menu.userIdCopied"))}>{t("accounts.menu.copyUserId")}</MenuItem>
          <MenuSeparator />
          {/* With several ticked these two are above, for all of them; MAIN is one account's. */}
          {!many && <MenuCheckItem checked={a.favourite} onChange={(favourite) => save({ favourite })}>{t("accounts.detail.favourite")}</MenuCheckItem>}
          <MenuCheckItem checked={a.main} onChange={(main) => save({ main })}>{t("accounts.detail.main")}</MenuCheckItem>
          {!many && <MenuCheckItem checked={!a.allowDisconnect} onChange={(back) => save({ allowDisconnect: !back })}>{t("accounts.detail.bringBack")}</MenuCheckItem>}
          {!many && (
            <>
              <MenuSeparator />
              <MenuItem icon="trash" danger onSelect={() => onRemove([a.id])}>{t("common.removeEllipsis")}</MenuItem>
            </>
          )}
        </>
      )}
    </ContextMenu>
  );
}
