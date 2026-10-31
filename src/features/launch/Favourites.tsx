import { useId, useEffect, useRef, useState, type FormEvent } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, asError, type AccountSummary, type Favourite, type LaunchTarget } from "../../api";
import { qk, useFavourites } from "../../data";
import { t, tx } from "../../i18n";
import { Switch } from "../../ui/controls";
import { ConfirmDialog, Dialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { friendly, toast } from "../../ui/Toast";
import { accountName, Avatar, Status } from "../accounts/shared";

/** A row of keys on Play: click fills the target; right-click (or the pencil) renames or deletes. */
export function FavouritesRow({ accounts, active, onPick }: {
  accounts: AccountSummary[];
  active: number | null;
  onPick: (f: Favourite) => void;
}) {
  const qc = useQueryClient();
  const favs = useFavourites();
  const menu = useRef<HTMLDivElement>(null);
  const [menuFor, setMenuFor] = useState<Favourite | null>(null);
  const [editing, setEditing] = useState<Favourite | null>(null);
  const [editOpen, setEditOpen] = useState(false);
  const [deleting, setDeleting] = useState<Favourite | null>(null);

  const remove = useMutation({
    mutationFn: (f: Favourite) => api.favouriteRemove(f.id),
    onSuccess: (_, f) => {
      qc.setQueryData<Favourite[]>(qk.favourites, (list) => list?.filter((x) => x.id !== f.id));
      toast(t("launch.fav.deleted", { name: f.name }));
    },
    onSettled: () => setDeleting(null),
  });

  const openMenu = (f: Favourite) => {
    setMenuFor(f);
    menu.current?.showPopover();
    menu.current?.querySelector<HTMLElement>(".menu-item")?.focus();
  };

  return (
    <div className="flex flex-col gap-2.5">
      <span className="text-[13px] text-dim">{t("launch.fav.title")}</span>
      {favs.isError && <span className="text-[13px] text-dim">{t("launch.fav.loadFailed")}</span>}
      {favs.data?.length === 0 && <span className="text-[13px] text-dim">{t("launch.fav.empty")}</span>}
      <div className="flex flex-wrap gap-3">
      {favs.data?.map((f) => (
        <div
          key={f.id}
          className="group relative max-w-full"
          style={{ anchorName: `--fav-${f.id}` }}
          onContextMenu={(e) => {
            e.preventDefault();
            openMenu(f);
          }}
        >
          <button className="btn max-w-full pe-10" aria-pressed={active === f.id} onClick={() => onPick(f)}>
            <Icon name="star" size={14} fill />
            <span className="truncate max-w-[260px]" title={f.name}>{f.name}</span>
          </button>
          <button
            className="wc w-8 h-8 absolute end-1.5 top-1.5 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
            aria-label={t("launch.fav.renameOrDeleteName", { name: f.name })}
            title={t("launch.fav.renameOrDelete")}
            onClick={() => openMenu(f)}
          >
            <Icon name="pencil" size={12} />
          </button>
        </div>
      ))}
      </div>

      <div ref={menu} id="fav-menu" popover="auto" className="menu" style={{ positionAnchor: menuFor ? `--fav-${menuFor.id}` : undefined }}>
        <button
          className="menu-item"
          popoverTarget="fav-menu"
          popoverTargetAction="hide"
          onClick={() => {
            setEditing(menuFor);
            setEditOpen(true);
          }}
        >
          <Icon name="pencil" size={12} />
          {t("launch.fav.rename")}
        </button>
        <button
          className="menu-item text-orange"
          popoverTarget="fav-menu"
          popoverTargetAction="hide"
          disabled={remove.isPending}
          onClick={() => setDeleting(menuFor)}
        >
          <Icon name="trash" size={12} />
          {t("launch.fav.delete")}
        </button>
      </div>

      <ConfirmDialog
        open={!!deleting}
        onClose={() => setDeleting(null)}
        onConfirm={() => deleting && remove.mutate(deleting)}
        busy={remove.isPending}
        title={t("launch.fav.deleteTitle")}
        text={
          tx("launch.fav.deleteText", { name: <span className="text-text font-medium">{deleting?.name}</span> })
        }
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

/** Saves a new favourite (no `initial.id`) or edits one; the target comes from `resolve`. */
export function FavouriteDialog({ open, onClose, accounts, initial, resolve, onSaved }: {
  open: boolean;
  onClose: () => void;
  accounts: AccountSummary[];
  initial: { id?: number; name: string; accountIds: number[]; inTray: boolean };
  resolve: () => Promise<LaunchTarget>;
  onSaved?: (f: Favourite) => void;
}) {
  const qc = useQueryClient();
  const accountsLabel = useId();
  const [name, setName] = useState("");
  const [ids, setIds] = useState<number[]>([]);
  const [tray, setTray] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    setName(initial.name);
    setIds(initial.accountIds);
    setTray(initial.inTray);
    setError(null);
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  const toggle = (id: number) => setIds((s) => (s.includes(id) ? s.filter((x) => x !== id) : [...s, id]));

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (busy) return;
    const n = name.trim();
    if (!n) return setError(t("launch.fav.needName"));
    setBusy(true);
    try {
      const target = await resolve();
      const accountIds = accounts.filter((a) => ids.includes(a.id)).map((a) => a.id);
      const f = await api.favouriteSave({ id: initial.id, name: n, target, accountIds, inTray: tray });
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
    <Dialog open={open} onClose={() => !busy && onClose()} title={initial.id ? t("launch.editFavourite") : t("launch.saveAsFavourite")} width={440}>
      <form onSubmit={submit} className="flex flex-col gap-4 p-5">
        <label className="flex flex-col gap-1.5">
          <span className="label">{t("launch.fav.name")}</span>
          <input
            className="field"
            maxLength={60}
            value={name}
            aria-invalid={!!error && !name.trim()}
            onChange={(e) => {
              setName(e.target.value);
              setError(null);
            }}
          />
        </label>
        <div className="flex flex-col gap-1.5">
          <span className="label" id={accountsLabel}>{t("launch.fav.defaultAccounts")}</span>
          <div role="group" aria-labelledby={accountsLabel} className="well scroll max-h-[208px] p-1.5">
            {accounts.length === 0 && <p className="p-3 text-dim">{t("launch.fav.noAccounts")}</p>}
            {accounts.map((a) => (
              <label key={a.id} className="flex items-center gap-2.5 min-h-11 px-2.5 rounded-[10px] cursor-pointer hover:bg-hover has-[:checked]:bg-sel">
                <input type="checkbox" className="cb" checked={ids.includes(a.id)} onChange={() => toggle(a.id)} />
                <Avatar account={a} size={20} />
                <span className="flex-1 min-w-0 truncate">{accountName(a)}</span>
                <span className="flex justify-end min-w-0 max-w-[45%] text-xs">
                  <Status status={a.status} />
                </span>
              </label>
            ))}
          </div>
          <span className="text-xs text-dim">{t("launch.fav.picksFirst")}</span>
        </div>
        <div className="flex items-center justify-between gap-2">
          <span>{t("launch.fav.showInTray")}</span>
          <Switch label={t("launch.fav.showInTray")} checked={tray} onChange={setTray} />
        </div>
        {error && <div role="alert" className="alert">{error}</div>}
        <div className="flex justify-end gap-2">
          <button type="button" className="btn" onClick={onClose} disabled={busy}>{t("common.cancel")}</button>
          <button type="submit" className="btn btn-primary" disabled={busy}>
            <Icon name="star" size={12} fill />
            {initial.id ? t("common.save") : t("launch.fav.saveFavourite")}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
