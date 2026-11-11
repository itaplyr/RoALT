import { useId, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api, asError, type AccountSummary, type Instance, type Macro, type MacroInput } from "../../api";
import { requestNavigation } from "../../app/navigation";
import { qk, useMacroRuns } from "../../data";
import { t } from "../../i18n";
import { Button } from "../../ui/Button";
import { Switch } from "../../ui/controls";
import { Dialog, DialogBody, DialogFooter, DialogSteps } from "../../ui/Dialog";
import { Field, Input } from "../../ui/Field";
import { Icon } from "../../ui/Icon";
import { Collapse, ListPresence } from "../../ui/motion";
import { friendly } from "../../ui/Toast";
import { accountName, Avatar } from "../accounts/shared";

/** A client by its account, named as the cards name it. */
export function clientOf(i: Instance | undefined, accounts: AccountSummary[]) {
  const account = i && accounts.find((a) => a.id === i.accountId);
  return { account, name: account ? accountName(account) : i ? t("dashboard.account", { id: i.accountId }) : "—" };
}

/** What went wrong, opening under the form (in the editor, over its buttons). */
function ErrorLine({ error }: { error: string | null }) {
  return (
    <Collapse open={!!error}>
      <p role="alert" className="alert mac-error">{error}</p>
    </Collapse>
  );
}

/** The editor: a new macro saved here counts as trusted; an edited one keeps its trust. Leaving it
 * with changes asks first, as its second step: a second dialog over it could end up under it. */
export function MacroDialog({ input, onClose, onSaved }: { input: MacroInput | null; onClose: () => void; onSaved: () => void }) {
  const [m, setM] = useState<MacroInput>({ name: "", body: "", hotkey: "", runOnLaunch: false });
  const [from, setFrom] = useState<MacroInput | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [discarding, setDiscarding] = useState(false);
  const cancel = useRef<HTMLButtonElement>(null);
  const keep = useRef<HTMLButtonElement>(null);
  const onLaunchHint = useId();
  const askText = useId();
  // Each opening starts from what it was given; set while rendering, so no frame shows the last one.
  if (input !== from) {
    setFrom(input);
    if (input) {
      setM(input);
      setError(null);
      setDiscarding(false);
    }
  }
  const dirty = !!input && (["name", "body", "hotkey", "runOnLaunch"] as const).some((k) => m[k] !== input[k]);
  // Esc, the backdrop, X and Cancel ask first rather than lose a typed script. Asking twice is asking
  // once: the Dialog calls this twice when the browser closed it anyway.
  const close = () => {
    if (busy) return;
    if (!dirty) return onClose();
    setDiscarding(true);
    requestAnimationFrame(() => keep.current?.focus());
  };
  const resume = () => {
    setDiscarding(false);
    requestAnimationFrame(() => cancel.current?.focus());
  };
  // Taken before it becomes the browser's close request, which a page can hold only after fresh
  // input: on the question, Esc means keep editing.
  const escape = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    e.preventDefault();
    if (discarding) resume();
    else close();
  };
  const put = (patch: Partial<MacroInput>) => {
    setM((old) => ({ ...old, ...patch }));
    setError(null);
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    api
      .macroSave(m)
      .then(
        () => {
          onSaved();
          onClose();
        },
        (err) => setError(friendly(asError(err))),
      )
      .finally(() => setBusy(false));
  };

  return (
    <Dialog open={input !== null} onClose={close} title={discarding ? t("macros.discardTitle") : input?.id ? t("macros.editTitle", { name: input.name }) : t("macros.new")} width={720}>
      <div onKeyDown={escape}>
        <DialogSteps step={discarding ? "discard" : "edit"} direction={discarding ? 1 : -1}>
          {discarding ? (
            <>
              <DialogBody className="text-fg-2">
                <p id={askText}>{t("macros.discardText")}</p>
              </DialogBody>
              <DialogFooter>
                <Button ref={keep} aria-describedby={askText} onClick={resume}>
                  {t("common.cancel")}
                </Button>
                <Button variant="danger" aria-describedby={askText} onClick={onClose}>
                  {t("macros.discard")}
                </Button>
              </DialogFooter>
            </>
          ) : (
            <form onSubmit={submit}>
              <DialogBody className="mac-form">
                <Field label={t("macros.name")}>{({ id }) => <Input id={id} maxLength={60} value={m.name} onChange={(e) => put({ name: e.target.value })} />}</Field>
                <Field label={t("macros.hotkey")} hint={t("macros.hotkeyHint")}>
                  {({ id, describedBy }) => (
                    <Input id={id} mono className="mac-key" placeholder={t("settings.placeholderOff")} aria-describedby={describedBy} value={m.hotkey} onChange={(e) => put({ hotkey: e.target.value })} />
                  )}
                </Field>
                <div className="mac-toggle">
                  <div className="min-w-0">
                    <p className="label">{t("macros.onLaunch")}</p>
                    <p id={onLaunchHint} className="mac-hint">{t("macros.onLaunchHint")}</p>
                  </div>
                  <Switch label={t("macros.onLaunch")} checked={m.runOnLaunch} describedBy={onLaunchHint} onChange={(runOnLaunch) => put({ runOnLaunch })} />
                </div>
                <Field label={t("macros.script")} hint={t("macros.scriptHint")}>
                  {({ id, describedBy }) => (
                    <textarea
                      id={id}
                      className="field mono mac-script"
                      rows={16}
                      aria-describedby={describedBy}
                      autoComplete="off"
                      autoCorrect="off"
                      autoCapitalize="off"
                      spellCheck={false}
                      value={m.body}
                      onChange={(e) => put({ body: e.target.value })}
                      onKeyDown={(e) => {
                        if (e.key !== "Tab" || e.shiftKey || e.ctrlKey || e.altKey || e.metaKey) return;
                        e.preventDefault();
                        // Through the editing commands, so Ctrl+Z undoes it like typing.
                        document.execCommand("insertText", false, "\t");
                      }}
                    />
                  )}
                </Field>
              </DialogBody>
              <DialogFooter className="mac-foot">
                {/* In the sticky footer: scrolled anywhere, a refused Save says why where it was pressed. */}
                <ErrorLine error={error} />
                <div className="flex flex-wrap justify-end gap-2">
                  <Button ref={cancel} onClick={close} disabled={busy}>
                    {t("common.cancel")}
                  </Button>
                  {/* Busy, it stays enabled (the focus stays on it); submit ignores it then. A missing
                      name is the backend's to refuse, in words, here. */}
                  <Button type="submit" variant="primary" busy={busy}>
                    {t("common.save")}
                  </Button>
                </div>
              </DialogFooter>
            </form>
          )}
        </DialogSteps>
      </div>
    </Dialog>
  );
}

/** Picks the clients to run on: every in-game one that runs no macro, all ticked. */
export function RunDialog({ macro, instances, accounts, onClose }: { macro: Macro | null; instances: Instance[]; accounts: AccountSummary[]; onClose: () => void }) {
  const qc = useQueryClient();
  const runs = useMacroRuns().data ?? [];
  const [ids, setIds] = useState<string[]>([]);
  const [from, setFrom] = useState<Macro | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const clientsLabel = useId();
  const inGame = instances.filter((i) => i.state === "inGame");
  const busyOn = (id: string) => runs.find((r) => r.instanceId === id);
  // Ticked once on each opening (the same macro again too); later instance changes leave the picks alone.
  if (macro !== from) {
    setFrom(macro);
    if (macro) {
      setIds(inGame.filter((i) => !busyOn(i.id)).map((i) => i.id));
      setError(null);
    }
  }
  // A client that left the game or started another macro since is left out.
  const picked = ids.filter((id) => inGame.some((i) => i.id === id) && !busyOn(id));
  const toggle = (id: string) => setIds((old) => (old.includes(id) ? old.filter((x) => x !== id) : [...old, id]));

  const run = () => {
    if (!macro || busy) return;
    setBusy(true);
    api
      .macroRun(macro.id, picked)
      .then(
        (list) => {
          qc.setQueryData(qk.macroRuns, list);
          onClose();
        },
        (err) => setError(friendly(asError(err))),
      )
      .finally(() => setBusy(false));
  };

  return (
    <Dialog open={macro !== null} onClose={() => !busy && onClose()} title={t("macros.runTitle", { name: macro?.name ?? "" })} width={460}>
      <DialogBody>
        <p id={clientsLabel} className="label">
          {t("macros.clients")}
        </p>
        <div role="group" aria-labelledby={clientsLabel} className="well scroll mac-clients">
          {inGame.length === 0 ? (
            <div className="mac-none">
              <p>{t("macros.noClients")}</p>
              <Button
                size="sm"
                icon="play"
                onClick={() => {
                  onClose();
                  requestNavigation("launch");
                }}
              >
                {t("app.nav.launch")}
              </Button>
            </div>
          ) : (
            <ListPresence
              items={inGame}
              getKey={(i) => i.id}
              className="mac-picks"
              render={(i) => {
                const other = busyOn(i.id);
                const { account, name } = clientOf(i, accounts);
                return (
                  <label className="mac-pick">
                    <input type="checkbox" className="cb" disabled={!!other} checked={!other && ids.includes(i.id)} onChange={() => toggle(i.id)} />
                    {account && <Avatar account={account} size={24} />}
                    <span className="mac-name">{name}</span>
                    {other && <span className="mac-pick-note">{t("macros.busy", { name: other.name })}</span>}
                  </label>
                );
              }}
            />
          )}
        </div>
        <ErrorLine error={error} />
      </DialogBody>
      <DialogFooter>
        <Button onClick={onClose} disabled={busy}>
          {t("common.cancel")}
        </Button>
        <Button variant="primary" icon="play" busy={busy} disabled={!picked.length} onClick={run}>
          {t("macros.run")}
        </Button>
      </DialogFooter>
    </Dialog>
  );
}

/** An imported or restored macro's whole source, read once before it may run. Cancel comes first. */
export function TrustDialog({ macro, onClose, onTrusted }: { macro: Macro | null; onClose: () => void; onTrusted: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [from, setFrom] = useState<Macro | null>(null);
  const warn = useId();
  if (macro !== from) {
    setFrom(macro);
    setError(null);
  }
  const trust = () => {
    if (!macro || busy) return;
    setBusy(true);
    setError(null);
    api
      .macroTrust(macro.id)
      .then(
        () => {
          onTrusted();
          onClose();
        },
        (err) => setError(friendly(asError(err))),
      )
      .finally(() => setBusy(false));
  };
  return (
    <Dialog open={macro !== null} onClose={() => !busy && onClose()} title={t("macros.reviewTitle", { name: macro?.name ?? "" })} width={720} describedBy={warn}>
      <DialogBody>
        <div className="mac-warn mac-caution">
          <Icon name="alert" className="text-warn" />
          <p id={warn}>{t("macros.reviewWarn")}</p>
        </div>
        <pre tabIndex={0} aria-label={t("macros.script")} className="well scroll selectable mono mac-source">
          {macro?.body}
        </pre>
        <ErrorLine error={error} />
      </DialogBody>
      <DialogFooter>
        <Button onClick={onClose} disabled={busy} data-autofocus>
          {t("common.cancel")}
        </Button>
        <Button variant="danger" busy={busy} onClick={trust}>
          {t("macros.trust")}
        </Button>
      </DialogFooter>
    </Dialog>
  );
}
