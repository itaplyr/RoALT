import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, type AccountSummary, type Group } from "../../api";
import { groupColor, qk, useSaveGroup } from "../../data";
import { fmtNum, t, tx } from "../../i18n";
import { Chip } from "../../ui/Bits";
import { Button, IconButton } from "../../ui/Button";
import { ConfirmDialog } from "../../ui/Dialog";
import { Num } from "../../ui/motion";
import { Popover } from "../../ui/Popover";
import { toast } from "../../ui/Toast";
import { SWATCHES } from "./shared";

const EDITOR = "acc-group-editor";
const anchorOf = (id: number | "new") => `--acc-group-${id}`;
const editor = () => document.getElementById(EDITOR);

/** Group keys over the accounts list: a click filters, the pencil or a right-click edits, "New group"
 * creates. The editor opens from the chip it edits, over the list (nothing below moves). */
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
  const chips = useRef<HTMLDivElement>(null);
  const deleted = useRef(false);
  const counts = useMemo(() => {
    const m = new Map<number, number>();
    for (const a of accounts) for (const g of a.groupIds) m.set(g, (m.get(g) ?? 0) + 1);
    return m;
  }, [accounts]);
  const group = typeof editing === "number" ? groups.find((g) => g.id === editing) : undefined;

  // The delete confirm, a modal, hides the editor (and it stays mounted, edits and all): it comes
  // back once the confirm has closed, or the focus goes to All when the group is gone.
  useEffect(() => {
    const el = editor();
    if (deleteOpen) return;
    if (deleted.current) {
      deleted.current = false;
      chips.current?.querySelector<HTMLElement>(".chip")?.focus();
    } else if (editing !== null && el) {
      if (!el.matches(":popover-open")) el.showPopover();
      el.querySelector("input")?.focus();
    }
  }, [editing, deleteOpen]);

  const remove = useMutation({
    mutationFn: (g: Group) => api.groupRemove(g.id),
    onSuccess: (_, g) => {
      qc.setQueryData<Group[]>(qk.groups, (list) => list?.filter((x) => x.id !== g.id));
      qc.invalidateQueries({ queryKey: qk.accounts });
      if (active === g.id) onPick(null);
      deleted.current = true;
      setEditing(null);
      setDeleteOpen(false);
      toast(t("accounts.groups.deleted", { name: g.name }));
    },
  });

  return (
    <>
      {/* Capped, so many groups can't push the list off the window. */}
      <div ref={chips} className="scroll flex-[1_1_320px] min-w-0 flex flex-wrap items-center gap-2 max-h-[124px] p-1 -m-1">
        <Chip pressed={active === null} onClick={() => onPick(null)}>{t("accounts.filter.all")}</Chip>
        {groups.map((g) => (
          <span
            key={g.id}
            className="acc-chip relative inline-flex"
            style={{ anchorName: anchorOf(g.id) }}
            onContextMenu={(e) => {
              e.preventDefault();
              setEditing(g.id);
            }}
          >
            <Chip pressed={active === g.id} dot={groupColor(g)} className="pe-10" onClick={() => onPick(active === g.id ? null : g.id)}>
              <span className="max-w-[180px] truncate">{g.name}</span>
              <span className="acc-chip-count absolute end-3.5 text-fg-3 font-normal tabular-nums">
                <Num value={counts.get(g.id) ?? 0} format={(v) => fmtNum(Math.round(v))} />
              </span>
            </Chip>
            <IconButton
              icon="pencil"
              size="sm"
              iconSize={13}
              label={t("accounts.groups.edit", { name: g.name })}
              className="acc-chip-edit absolute end-1 top-1/2 -translate-y-1/2"
              aria-haspopup="dialog"
              aria-expanded={editing === g.id}
              onClick={() => setEditing(g.id)}
            />
          </span>
        ))}
        <span className="inline-flex" style={{ anchorName: anchorOf("new") }}>
          <Button size="md" variant="ghost" icon="plus" aria-haspopup="dialog" aria-expanded={editing === "new"} onClick={() => setEditing("new")}>
            {t("accounts.groups.newEllipsis")}
          </Button>
        </span>
      </div>

      <Popover
        id={EDITOR}
        anchor={anchorOf(editing ?? "new")}
        role="dialog"
        label={group ? t("accounts.groups.edit", { name: group.name }) : t("accounts.groups.new")}
        className="acc-editor"
        onToggle={(open) => !open && !deleteOpen && setEditing(null)}
      >
        {editing !== null && (
          <GroupEditor
            key={editing}
            group={group}
            groups={groups}
            onDone={() => editor()?.hidePopover()}
            onDelete={
              group &&
              (() => {
                setDeleting(group);
                setDeleteOpen(true);
              })
            }
          />
        )}
      </Popover>

      <ConfirmDialog
        open={deleteOpen}
        onClose={() => setDeleteOpen(false)}
        onConfirm={() => deleting && remove.mutate(deleting)}
        busy={remove.isPending}
        title={t("accounts.groups.deleteTitle")}
        text={tx("accounts.groups.deleteText", { name: <span className="text-fg font-medium">{deleting?.name}</span> })}
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
    <form onSubmit={submit} className="flex flex-col gap-3">
      <input
        className="field"
        aria-label={t("accounts.groups.name")}
        placeholder={t("accounts.groups.name")}
        maxLength={32}
        value={name}
        aria-invalid={!!error || undefined}
        aria-describedby={error ? `${EDITOR}-error` : undefined}
        onChange={(e) => {
          setName(e.target.value);
          setError(null);
        }}
      />
      {error && <p id={`${EDITOR}-error`} role="alert" className="text-[13px] text-attention -mt-1">{error}</p>}
      <div className="flex flex-wrap gap-2" role="group" aria-label={t("accounts.groups.colour")}>
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
      <div className="flex items-center gap-2 pt-1">
        {onDelete && <IconButton icon="trash" label={t("accounts.groups.delete")} className="acc-danger" onClick={onDelete} />}
        <Button size="md" variant="ghost" className="ms-auto" onClick={onDone}>{t("common.cancel")}</Button>
        <Button size="md" variant="primary" type="submit" busy={save.isPending}>
          {group ? t("common.save") : t("accounts.groups.create")}
        </Button>
      </div>
    </form>
  );
}
