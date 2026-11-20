import { useRef, useState, type FormEvent } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { asError } from "../../api";
import { t } from "../../i18n";
import { Button, IconButton } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { Input } from "../../ui/Field";
import { friendly, toast, toastError } from "../../ui/Toast";

export type Flags = Record<string, string>;
type Row = { key: number; name: string; value: string };

/** JSON values as Roblox's file holds them: strings, `true` as "True" (as Bloxstrap writes it). */
function fromJson(text: string): Flags | string {
  let v: unknown;
  try {
    v = text.trim() ? JSON.parse(text) : {};
  } catch {
    return t("flags.badJson");
  }
  if (!v || typeof v !== "object" || Array.isArray(v)) return t("flags.badJson");
  const out: Flags = {};
  for (const [name, value] of Object.entries(v)) {
    if (typeof value === "string") out[name] = value;
    else if (typeof value === "boolean") out[name] = value ? "True" : "False";
    else if (typeof value === "number") out[name] = String(value);
    else return t("flags.badValue", { name });
  }
  return out;
}

/** The rows as flags; blank rows are dropped. A message when one can't be a flag. */
function fromRows(rows: Row[]): Flags | string {
  const out: Flags = {};
  for (const r of rows) {
    const name = r.name.trim();
    if (!name && !r.value.trim()) continue;
    if (!name) return t("flags.noName");
    if (name in out) return t("flags.duplicate", { name });
    out[name] = r.value;
  }
  return out;
}

const json = (f: Flags) => JSON.stringify(f, null, 2);
let nextKey = 0;
const rowsOf = (f: Flags): Row[] => Object.entries(f).map(([name, value]) => ({ key: nextKey++, name, value }));

/** Custom FFlags as name and value rows, or the same as JSON (Bloxstrap's format) to paste or
 * copy. Save hands them to `onSave`; a refusal stays in the dialog. */
export function FlagsDialog({ open, onClose, title, flags, onSave }: {
  open: boolean;
  onClose: () => void;
  title: string;
  flags: Flags;
  onSave: (flags: Flags) => Promise<unknown>;
}) {
  const [rows, setRows] = useState<Row[]>([]);
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const list = useRef<HTMLDivElement>(null);
  // Each opening starts from what is saved.
  const [wasOpen, setWasOpen] = useState(false);
  if (wasOpen !== open) {
    setWasOpen(open);
    if (open) {
      setRows(rowsOf(flags));
      setText(null);
      setError(null);
    }
  }

  const current = () => (text === null ? fromRows(rows) : fromJson(text));
  const edit = (key: number, patch: Partial<Row>) => {
    setRows((rs) => rs.map((r) => (r.key === key ? { ...r, ...patch } : r)));
    setError(null);
  };
  const add = () => {
    setRows((rs) => [...rs, { key: nextKey++, name: "", value: "" }]);
    requestAnimationFrame(() => list.current?.querySelector<HTMLInputElement>(".flag-row:last-child input")?.focus());
  };
  // Switching keeps what is typed: a mistake stops it, and is shown.
  const toggle = () => {
    const f = current();
    if (typeof f === "string") return setError(f);
    setError(null);
    if (text === null) setText(json(f));
    else {
      setRows(rowsOf(f));
      setText(null);
    }
  };
  const copy = () => {
    const f = current();
    if (typeof f === "string") return setError(f);
    writeText(json(f)).then(() => toast(t("flags.copied")), toastError);
  };

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (busy) return;
    const f = current();
    if (typeof f === "string") return setError(f);
    setBusy(true);
    try {
      await onSave(f);
      onClose();
    } catch (err) {
      setError(friendly(asError(err)));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onClose={onClose} title={title} width={560}>
      <form onSubmit={submit}>
        <div className="dlg-body flex flex-col gap-3">
          <p className="text-[13px] text-fg-3">{t("flags.note")}</p>
          {text === null ? (
            <div ref={list} className="flex flex-col gap-2">
              {rows.length === 0 && <p className="text-fg-2">{t("flags.empty")}</p>}
              {rows.map((r) => (
                <div key={r.key} className="flag-row flex items-center gap-2">
                  <Input mono className="flex-[3] min-w-0" aria-label={t("flags.name")} placeholder={t("flags.name")} value={r.name} maxLength={128} onChange={(e) => edit(r.key, { name: e.target.value })} />
                  <Input mono className="flex-[2] min-w-0" aria-label={t("flags.valueOf", { name: r.name || t("flags.name") })} placeholder={t("flags.value")} value={r.value} maxLength={1024} onChange={(e) => edit(r.key, { value: e.target.value })} />
                  <IconButton icon="trash" label={t("flags.remove", { name: r.name || t("flags.name") })} onClick={() => setRows((rs) => rs.filter((x) => x.key !== r.key))} />
                </div>
              ))}
              <div>
                <Button size="sm" variant="ghost" icon="plus" onClick={add}>{t("flags.add")}</Button>
              </div>
            </div>
          ) : (
            <textarea
              className="field mono text-xs"
              rows={12}
              aria-label={t("flags.jsonLabel")}
              value={text}
              onChange={(e) => {
                setText(e.target.value);
                setError(null);
              }}
              autoComplete="off"
              autoCorrect="off"
              autoCapitalize="off"
              spellCheck={false}
              placeholder={'{\n  "FIntDebugForceMSAASamples": "1"\n}'}
              data-autofocus
            />
          )}
          {error && <p role="alert" className="text-[13px] text-attention">{error}</p>}
        </div>
        <div className="dlg-foot">
          <Button size="md" variant="ghost" icon={text === null ? "terminal" : "sliders"} className="me-auto" onClick={toggle}>
            {text === null ? t("flags.editJson") : t("flags.editRows")}
          </Button>
          <Button size="md" variant="ghost" icon="copy" onClick={copy}>{t("flags.copy")}</Button>
          <Button size="md" onClick={onClose}>{t("common.cancel")}</Button>
          <Button size="md" type="submit" variant="primary" busy={busy} aria-disabled={busy || undefined}>{t("common.save")}</Button>
        </div>
      </form>
    </Dialog>
  );
}
