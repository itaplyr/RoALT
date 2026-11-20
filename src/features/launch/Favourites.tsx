import { useEffect, useId, useLayoutEffect, useReducer, useRef, useState, type CSSProperties, type FormEvent, type RefObject } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, asError, type AccountSummary, type Favourite, type LaunchTarget, type SameServer } from "../../api";
import type { FavouriteOptions } from "../../bindings/FavouriteOptions";
import { groupColor, qk, useFavourites, useGame, useGroups } from "../../data";
import { t, tx, type Key } from "../../i18n";
import { Chip, Skeleton } from "../../ui/Bits";
import { Button, IconButton } from "../../ui/Button";
import { OffOrNumberField, Switch } from "../../ui/controls";
import { ConfirmDialog, Dialog } from "../../ui/Dialog";
import { Field, Input, Select } from "../../ui/Field";
import { Icon } from "../../ui/Icon";
import { Collapse, riseIn, useFlip, useFreshItems } from "../../ui/motion";
import { MenuItem, MenuSeparator, Popover } from "../../ui/Popover";
import { RANGES } from "../../ui/ranges";
import { friendly, toast, toastError } from "../../ui/Toast";
import { reordered } from "../dashboard/homeLayout";
import { useGridDrag } from "../dashboard/useGridDrag";
import { AccountPicker } from "./AccountPicker";
import { copyRoaltLink, roaltLink, targetPlace } from "./target";
import "./launch.css";

/** A favourite's game icon; the star while RoALT doesn't know its game (or it names none). */
export function FavIcon({ target, size }: { target: LaunchTarget; size: number }) {
  const url = useGame(targetPlace(target)).data?.iconUrl;
  return url ? (
    <img src={url} alt="" width={size} height={size} className="fav-icon" loading="lazy" />
  ) : (
    <Icon name="star" size={Math.min(size, 14)} fill className="flex-none" />
  );
}

/** The ways a favourite's accounts spread over servers, as Play names them. */
const SPREADS: readonly (readonly [SameServer, Key])[] = [
  ["ownPrivate", "launch.spreadOwn"],
  ["randomPublic", "launch.spreadRandom"],
  ["closest", "launch.spreadClosest"],
  ["mostFull", "servers.orderMostFull"],
  ["mostEmpty", "servers.orderMostEmpty"],
];

const MENU = "fav-menu";
const menu = () => document.getElementById(MENU);

/** Favourites as pills (Play, a Home tab): a click fills the target. Drag one, or Alt and the arrows on
 * it, to move it; its ⋯ (or a right-click) renames or deletes it. */
export function FavouritesRow({ accounts, active, onPick }: {
  accounts: AccountSummary[];
  active: number | null;
  onPick: (f: Favourite) => void;
}) {
  const qc = useQueryClient();
  const favs = useFavourites();
  const list = favs.data ?? [];
  const ids = list.map((f) => f.id);
  const labelId = useId();
  const [menuFor, setMenuFor] = useState<Favourite | null>(null);
  const [editing, setEditing] = useState<Favourite | null>(null);
  const [editOpen, setEditOpen] = useState(false);
  const [deleting, setDeleting] = useState<Favourite | null>(null);
  const removed = useRef(false);

  const remove = useMutation({
    mutationFn: (f: Favourite) => api.favouriteRemove(f.id),
    onSuccess: (_, f) => {
      removed.current = true;
      qc.setQueryData<Favourite[]>(qk.favourites, (l) => l?.filter((x) => x.id !== f.id));
      toast(t("launch.fav.deleted", { name: f.name }));
    },
    onSettled: () => setDeleting(null),
  });
  // Drawn in the new order now, not when the query next tells React: the glide needs this render, and
  // Alt and the arrows give the moved one its focus back on the next frame.
  const [, rerender] = useReducer((n: number) => n + 1, 0);
  const reorder = (next: number[]) => {
    qc.setQueryData<Favourite[]>(qk.favourites, (l) => l?.map((f) => ({ ...f, sortOrder: next.indexOf(f.id) })));
    rerender();
    api.favouritesReorder(next).catch((e) => {
      toastError(e);
      void qc.invalidateQueries({ queryKey: qk.favourites });
    });
  };

  const drag = useGridDrag((from, over) => reorder(reordered(ids, ids, from, over)));
  // A deleted one's neighbours glide into its room; a move is glided by useGridDrag itself.
  useFlip(drag.grid, {
    skip: () => {
      const was = removed.current;
      removed.current = false;
      return !was;
    },
  });
  // A new one (just saved, or the list arriving) rises in.
  const fresh = useFreshItems(ids);
  useLayoutEffect(() => {
    fresh.forEach((id, i) => {
      const el = drag.grid.current?.querySelector(`[data-fav="${id}"]`);
      if (el) riseIn(el, i);
    });
  }, [fresh.join()]); // eslint-disable-line react-hooks/exhaustive-deps

  const openMenu = (f: Favourite) => {
    setMenuFor(f);
    const m = menu();
    if (m && !m.matches(":popover-open")) m.showPopover();
  };

  return (
    <div className="flex flex-col gap-2">
      <span id={labelId} className="label text-fg-2">{t("launch.fav.title")}</span>
      {favs.isPending ? (
        <div aria-busy="true" aria-label={t("common.loading")} className="launch-favs">
          {[112, 148, 96].map((w) => (
            <Skeleton key={w} width={w} height={34} radius={999} />
          ))}
        </div>
      ) : favs.isError ? (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <p className="text-[13px] text-fg-2">{t("launch.fav.loadFailed")}</p>
          <Button size="sm" icon="refresh" busy={favs.isFetching} disabled={favs.isFetching} onClick={() => void favs.refetch()}>
            {t("common.tryAgain")}
          </Button>
        </div>
      ) : list.length === 0 ? (
        <p className="text-[13px] text-fg-3">{t("launch.fav.empty")}</p>
      ) : null}
      {/* Mounted through loading too: useGridDrag and useFlip hold on to this element. */}
      <ul ref={drag.grid as RefObject<HTMLUListElement>} aria-labelledby={labelId} className="sort-row launch-favs empty:hidden">
        {list.map((f) => (
          <li
            key={f.id}
            data-fav={f.id}
            data-flip
            className="launch-fav"
            style={{ anchorName: `--fav-${f.id}` } as CSSProperties}
            onContextMenu={(e) => {
              e.preventDefault();
              openMenu(f);
            }}
          >
            <button type="button" className="chip launch-fav-pick" aria-pressed={active === f.id} onClick={() => onPick(f)} {...drag.item(f.name)}>
              <FavIcon target={f.target} size={18} />
              <span className="truncate max-w-[240px]">{f.name}</span>
            </button>
            <IconButton
              icon="dots"
              size="sm"
              className="launch-fav-more"
              label={t("launch.fav.editOrDeleteName", { name: f.name })}
              aria-haspopup="menu"
              popoverTarget={MENU}
              onClick={() => setMenuFor(f)}
            />
          </li>
        ))}
      </ul>
      <span className="sr-only" aria-live="polite">{drag.moved}</span>

      <Popover
        id={MENU}
        anchor={`--fav-${menuFor?.id ?? 0}`}
        role="menu"
        label={menuFor?.name}
      >
        <MenuItem
          icon="pencil"
          onSelect={() => {
            setEditing(menuFor);
            setEditOpen(true);
          }}
        >
          {t("launch.fav.edit")}
        </MenuItem>
        {menuFor && roaltLink(menuFor.target) && (
          <MenuItem icon="link" onSelect={() => copyRoaltLink(menuFor.target)}>{t("share.copyLink")}</MenuItem>
        )}
        <MenuSeparator />
        <MenuItem icon="trash" danger disabled={remove.isPending} onSelect={() => setDeleting(menuFor)}>
          {t("launch.fav.deleteConfirm")}
        </MenuItem>
      </Popover>

      <ConfirmDialog
        open={!!deleting}
        onClose={() => !remove.isPending && setDeleting(null)}
        onConfirm={() => deleting && remove.mutate(deleting)}
        busy={remove.isPending}
        title={t("launch.fav.deleteTitle")}
        text={tx("launch.fav.deleteText", { name: <span className="text-fg font-medium">{deleting?.name}</span> })}
        confirmLabel={t("launch.fav.deleteConfirm")}
      />

      {editing && (
        <FavouriteDialog
          open={editOpen}
          onClose={() => setEditOpen(false)}
          accounts={accounts}
          initial={editing}
          resolve={() => Promise.resolve(editing.target)}
        />
      )}
    </div>
  );
}

/** The accounts a favourite launches: its own, then the members of its groups as they are now, each
 * once (as the backend's `favourite_accounts`). */
export const favouriteAccountIds = (f: Favourite, accounts: AccountSummary[]) => [
  ...new Set([...f.accountIds, ...accounts.filter((a) => a.groupIds.some((g) => f.options.groupIds.includes(g))).map((a) => a.id)]),
];

/** Saves a new favourite (no `initial.id`) or edits one; the target comes from `resolve`. */
export function FavouriteDialog({ open, onClose, accounts, initial, resolve, onSaved }: {
  open: boolean;
  onClose: () => void;
  accounts: AccountSummary[];
  initial: { id?: number; name: string; accountIds: number[]; inTray: boolean; options?: FavouriteOptions };
  resolve: () => Promise<LaunchTarget>;
  onSaved?: (f: Favourite) => void;
}) {
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [ids, setIds] = useState<number[]>([]);
  const groups = useGroups().data ?? [];
  const [groupIds, setGroupIds] = useState<number[]>([]);
  const [tray, setTray] = useState(false);
  // How it launches: "" is as a favourite always did (no particular way), 0 no cap of its own.
  const [servers, setServers] = useState<SameServer | "">("");
  const [fps, setFps] = useState(0);
  const [hotkey, setHotkey] = useState("");
  const [nameError, setNameError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    setName(initial.name);
    setIds(initial.accountIds);
    setGroupIds(initial.options?.groupIds ?? []);
    setTray(initial.inTray);
    setServers(initial.options?.sameServer ?? "");
    setFps(initial.options?.fpsCap ?? 0);
    setHotkey(initial.options?.hotkey ?? "");
    setNameError(null);
    setError(null);
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (busy) return;
    const n = name.trim();
    if (!n) return setNameError(t("launch.fav.needName"));
    setBusy(true);
    setError(null);
    try {
      const target = await resolve();
      const accountIds = accounts.filter((a) => ids.includes(a.id)).map((a) => a.id);
      // A group deleted since isn't kept.
      const options: FavouriteOptions = { groupIds: groupIds.filter((id) => groups.some((g) => g.id === id)), hotkey: hotkey.trim(), sameServer: servers || null, fpsCap: fps || null };
      const f = await api.favouriteSave({ id: initial.id, name: n, target, accountIds, inTray: tray, options });
      qc.invalidateQueries({ queryKey: qk.favourites });
      toast(initial.id ? t("launch.fav.saved", { name: f.name }) : t("launch.fav.added", { name: f.name }));
      onSaved?.(f);
      onClose();
    } catch (err) {
      setError(friendly(asError(err)));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onClose={() => !busy && onClose()} title={initial.id ? t("launch.editFavourite") : t("launch.saveAsFavourite")} width={460}>
      <form onSubmit={(e) => void submit(e)}>
        <div className="dlg-body flex flex-col gap-4">
          <Field label={t("launch.fav.name")} error={nameError}>
            {(f) => (
              <Input
                id={f.id}
                aria-describedby={f.describedBy}
                invalid={f.invalid}
                maxLength={60}
                autoComplete="off"
                value={name}
                onChange={(e) => {
                  setName(e.target.value);
                  setNameError(null);
                }}
              />
            )}
          </Field>
          <div className="flex flex-col gap-1.5">
            <span aria-hidden="true" className="label">{t("launch.fav.defaultAccounts")}</span>
            <AccountPicker variant="list" accounts={accounts} picked={ids} onPicked={setIds} label={t("launch.fav.defaultAccounts")} />
            <span className="text-xs text-fg-3">{t("launch.fav.picksFirst")}</span>
          </div>
          {groups.length > 0 && (
            <div className="flex flex-col gap-1.5">
              <span aria-hidden="true" className="label">{t("accounts.groups.title")}</span>
              <div role="group" aria-label={t("accounts.groups.title")} className="flex flex-wrap gap-1.5">
                {groups.map((g) => (
                  <Chip key={g.id} small pressed={groupIds.includes(g.id)} dot={groupColor(g)} onClick={() => setGroupIds((now) => (now.includes(g.id) ? now.filter((x) => x !== g.id) : [...now, g.id]))}>
                    <span className="max-w-[160px] truncate">{g.name}</span>
                  </Chip>
                ))}
              </div>
              <span className="text-xs text-fg-3">{t("launch.fav.groupsHint")}</span>
            </div>
          )}
          {/* What a click on Home, the tray or its key launches it with; picking it in Play sets them there. */}
          <label className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1.5">
            <span>{t("launch.findServer")}</span>
            <Select
              aria-label={t("launch.findServer")}
              value={servers}
              onChange={(v) => setServers(v as SameServer | "")}
              options={[
                { value: "", label: t("common.default") },
                ...(["pickFirst", "followFirst"] as const).map((v) => ({ value: v, label: `${t("launch.sameServer")}: ${t(v === "pickFirst" ? "launch.pickServer" : "launch.followFirst")}` })),
                ...SPREADS.map(([v, label]) => ({ value: v, label: `${t("launch.spread")}: ${t(label)}` })),
              ]}
            />
          </label>
          <div className="flex items-center justify-between gap-3">
            <span aria-hidden="true">{t("settings.fpsCap")}</span>
            <OffOrNumberField label={t("settings.fpsCap")} value={fps} range={RANGES.fpsCap} unit={t("accounts.detail.fpsUnit")} short="FPS" onCommit={setFps} />
          </div>
          <div className="flex items-center justify-between gap-3">
            <span aria-hidden="true">{t("launch.fav.showInTray")}</span>
            <Switch label={t("launch.fav.showInTray")} checked={tray} onChange={setTray} />
          </div>
          <Field label={t("launch.fav.hotkey")} hint={t("settings.globalKeyHint")}>
            {(f) => (
              <Input id={f.id} aria-describedby={f.describedBy} autoComplete="off" spellCheck={false} placeholder={t("settings.placeholderOff")} maxLength={40} value={hotkey} onChange={(e) => setHotkey(e.target.value)} />
            )}
          </Field>
          <Collapse open={!!error}>
            <p role="alert" className="alert">{error}</p>
          </Collapse>
        </div>
        <div className="dlg-foot">
          <Button onClick={onClose} disabled={busy}>{t("common.cancel")}</Button>
          <Button type="submit" variant="primary" busy={busy} icon="star">
            {initial.id ? t("common.save") : t("launch.fav.saveFavourite")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
