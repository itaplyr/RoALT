import { useState, type FormEvent } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, type AccountSummary, type Group } from "../../api";
import { groupColor, qk, useSaveGroup } from "../../data";
import { ConfirmDialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { toast } from "../../ui/Toast";
import { t, tx } from "../../i18n";
import { SWATCHES } from "./shared";

/** Group keys over the accounts list: click filters, right-click or the pencil edits, "+" creates. */
export function GroupChips({ groups, accounts, active, onPick }: {
  groups: Group[];
  accounts: AccountSummary[];
  active: number | null;
  onPick: (id: number | null) => void;
}) {
  const qc = useQueryClient();
  const [editing, setEditing] = useState<number | "new" | null>(null);
  const [deleting, setDeleting] = useState<Group | null>(null);
  const [deleteOpen, setDeleteOpen] = useState(false);

  const remove = useMutation({
    mutationFn: (g: Group) => api.groupRemove(g.id),
    onSuccess: (_, g) => {
      qc.setQueryData<Group[]>(qk.groups, (list) => list?.filter((x) => x.id !== g.id));
      qc.invalidateQueries({ queryKey: qk.accounts });
      if (active === g.id) onPick(null);
      setDeleteOpen(false);
      setEditing(null);
      toast(t("accounts.groups.deleted", { name: g.name }));
    },
  });

  return (
    <>
      {/* Capped, so many groups can't push the list off the window. */}
      <div className="scroll min-w-0 flex flex-wrap items-center gap-3 max-h-[132px] p-2 -m-2">
        <button className="chip" aria-pressed={active === null} onClick={() => onPick(null)}>{t("accounts.filter.all")}</button>
        {groups.map((g) => (
          <div
            key={g.id}
            className="group relative"
            onContextMenu={(e) => {
              e.preventDefault();
              setEditing(g.id);
            }}
          >
            <button className="chip pe-9" aria-pressed={active === g.id} onClick={() => onPick(active === g.id ? null : g.id)}>
              <span aria-hidden="true" className="w-2.5 h-2.5 flex-none rounded-full" style={{ background: groupColor(g) }} />
              <span className="max-w-[180px] truncate">{g.name}</span>
              <span className="mono absolute end-3 text-xs text-dim group-hover:opacity-0 group-has-[.wc:focus-visible]:opacity-0">
                {accounts.filter((a) => a.groupIds.includes(g.id)).length}
              </span>
            </button>
            <button
              className="wc w-8 h-8 absolute end-1 top-1.5 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
              aria-label={t("accounts.groups.edit", { name: g.name })}
              title={t("accounts.groups.editHint")}
              onClick={() => setEditing(g.id)}
            >
              <Icon name="pencil" size={12} />
            </button>
          </div>
        ))}
        <button className="chip" aria-label={t("accounts.groups.new")} title={groups.length ? t("accounts.groups.new") : t("accounts.groups.empty")} onClick={() => setEditing("new")}>
          <Icon name="plus" size={14} />
          {groups.length === 0 && t("accounts.groups.new")}
        </button>
      </div>
      {editing !== null && (
        <div className="basis-full">
          {editing === "new" ? (
            <GroupEditor key="new" groups={groups} onDone={() => setEditing(null)} />
          ) : (
            <GroupEditor
              key={editing}
              group={groups.find((g) => g.id === editing)}
              groups={groups}
              onDone={() => setEditing(null)}
              onDelete={() => {
                setDeleting(groups.find((g) => g.id === editing) ?? null);
                setDeleteOpen(true);
              }}
            />
          )}
        </div>
      )}

      <ConfirmDialog
        open={deleteOpen}
        onClose={() => setDeleteOpen(false)}
        onConfirm={() => deleting && remove.mutate(deleting)}
        busy={remove.isPending}
        title={t("accounts.groups.deleteTitle")}
        text={tx("accounts.groups.deleteText", { name: <span className="text-text font-medium">{deleting?.name}</span> })}
        confirmLabel={t("accounts.groups.delete")}
      />
    </>
  );
}

function GroupEditor({ group, groups, onDone, onDelete }: {
  group?: Group;
  groups: Group[];
  onDone: () => void;
  onDelete?: () => void;
}) {
  const save = useSaveGroup();
  const [name, setName] = useState(group?.name ?? "");
  const [color, setColor] = useState(group?.color ?? SWATCHES[groups.length % SWATCHES.length]);
  const [error, setError] = useState<string | null>(null);

  function submit(e: FormEvent) {
    e.preventDefault();
    const n = name.trim();
    if (!n) return onDone();
    if (groups.some((g) => g.id !== group?.id && g.name.toLowerCase() === n.toLowerCase())) {
      return setError(t("accounts.groups.nameTaken"));
    }
    save.mutate({ id: group?.id, name: n, color }, { onSuccess: onDone });
  }

  return (
    <form
      onSubmit={submit}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          onDone();
        }
      }}
      className="enter well flex flex-wrap items-center gap-3 p-3"
    >
      <input
        autoFocus
        className="field flex-[1_1_200px] w-auto"
        aria-label={t("accounts.groups.name")}
        placeholder={t("accounts.groups.name")}
        maxLength={32}
        value={name}
        aria-invalid={!!error}
        onChange={(e) => {
          setName(e.target.value);
          setError(null);
        }}
      />
      {error && <span role="alert" className="text-xs text-orange">{error}</span>}
      <div className="flex flex-wrap gap-1.5" role="group" aria-label={t("accounts.groups.colour")}>
        {SWATCHES.map((c) => (
          <button
            key={c}
            type="button"
            className="swatch"
            style={{ background: c }}
            aria-label={t("accounts.groups.colourValue", { color: c })}
            aria-pressed={c === color}
            onClick={() => setColor(c)}
          />
        ))}
      </div>
      <div className="flex flex-wrap gap-2">
        <button type="submit" className="btn btn-primary btn-sm" disabled={save.isPending}>
          {group ? t("common.save") : t("accounts.groups.create")}
        </button>
        <button type="button" className="btn btn-sm" onClick={onDone}>{t("common.cancel")}</button>
        {onDelete && (
          <button type="button" className="btn btn-sm btn-danger px-2" aria-label={t("accounts.groups.delete")} title={t("accounts.groups.delete")} onClick={onDelete}>
            <Icon name="trash" size={12} />
          </button>
        )}
      </div>
    </form>
  );
}
