import { useEffect, useId, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, asError, type AccountSummary, type PrivateServer, type PrivateServerPatch, type PrivateServerSettings } from "../../api";
import { qk } from "../../data";
import { Skeleton } from "../../ui/Bits";
import { Button, IconButton } from "../../ui/Button";
import { Dialog, DialogSteps } from "../../ui/Dialog";
import { Switch, TextField } from "../../ui/controls";
import { Collapse, ListPresence } from "../../ui/motion";
import { friendly } from "../../ui/Toast";
import { fmtDate, fmtNum, t } from "../../i18n";

type Ask = "off" | "newLink" | "renew";

/** A dialog's question before something that can't be taken back, as one of its steps. Cancel goes
 * back; while `busy` both buttons hold still (and keep the focus). */
export function ConfirmStep({ text, confirmLabel, busy, onCancel, onConfirm }: {
  text: string;
  confirmLabel: string;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const textId = useId();
  return (
    <div className="dlg-body flex flex-col gap-5">
      <p id={textId} className="text-fg-2 text-pretty">{text}</p>
      <div className="flex flex-wrap justify-end gap-2">
        <Button data-autofocus aria-describedby={textId} aria-disabled={busy || undefined} onClick={() => !busy && onCancel()}>
          {t("common.cancel")}
        </Button>
        <Button variant="danger" aria-describedby={textId} busy={busy} onClick={() => !busy && onConfirm()}>
          {confirmLabel}
        </Button>
      </div>
    </div>
  );
}

/** Back from a question step, the focus returns to the control that asked: a button marked
 * data-asks="<question>", or the button inside an element marked so. The ref goes around the
 * DialogSteps, which outlives each step. */
export function useAskFocus<T extends HTMLElement>(ask: string | null) {
  const root = useRef<T>(null);
  const last = useRef<string | null>(null);
  useEffect(() => {
    if (ask) {
      last.current = ask;
      return;
    }
    const from = last.current;
    last.current = null;
    if (!from) return;
    const frame = requestAnimationFrame(() => root.current?.querySelector<HTMLElement>(`button[data-asks="${from}"], [data-asks="${from}"] button`)?.focus());
    return () => cancelAnimationFrame(frame);
  }, [ask]);
  return root;
}

/** Settings of a private server a vault account owns; every call is made as that owner. Turning it
 * off, a new invite link and auto-renew ask first, as steps of this dialog. */
export function PrivateServerDialog({ server, owner, accounts, onClose, onChanged }: {
  server: PrivateServer | null;
  owner: AccountSummary | null;
  /** Valid vault accounts, for "Allow all my accounts". */
  accounts: AccountSummary[];
  onClose: () => void;
  /** Something changed: the server list is stale. */
  onChanged: () => void;
}) {
  const qc = useQueryClient();
  const id = server?.vipServerId ?? 0;
  const key = [...qk.privateServers, "settings", id, owner?.id];
  const q = useQuery({ queryKey: key, queryFn: () => api.privateServerGet(owner!.id, id), enabled: !!server && !!owner });
  const s = q.data;
  const [busy, setBusy] = useState(false);
  const [ask, setAsk] = useState<Ask | null>(null);
  const [error, setError] = useState<string | null>(null);
  const steps = useAskFocus<HTMLDivElement>(ask);
  const allowedId = useId();
  const noPriceId = useId();
  const name = s?.name || server?.name || t("servers.unnamed");
  const at = owner?.username ?? "";
  const cost = s?.price ?? null;

  const close = () => {
    if (busy) return;
    setAsk(null);
    setError(null);
    onClose();
  };

  /** True when the call worked; why it didn't shows in the dialog. */
  async function run(call: () => Promise<PrivateServerSettings>) {
    setBusy(true);
    try {
      qc.setQueryData(key, await call());
      setError(null);
      onChanged();
      return true;
    } catch (e) {
      const err = asError(e);
      if (err.kind !== "cancelled") setError(friendly(err));
      // The write may have worked before the re-read failed: show the real state before a retry.
      void qc.invalidateQueries({ queryKey: key });
      onChanged();
      return false;
    } finally {
      setBusy(false);
      setAsk(null);
    }
  }
  const update = (patch: PrivateServerPatch) => run(() => api.privateServerUpdate(owner!.id, id, patch));
  // The price is the one the dialog shows (and the question states).
  const renew = (on: boolean, p: number) => run(() => api.privateServerRenew(owner!.id, id, on, p));
  // TextField puts the old name back when this throws.
  async function rename(next: string) {
    if (!next) setError(t("servers.needName"));
    else if (await update({ name: next })) return;
    throw new Error("not renamed");
  }

  const allowed = new Set(s?.allowedUsers.map((u) => u.userId));
  const toAdd = accounts.filter((a) => a.userId !== owner?.userId && !allowed.has(a.userId)).map((a) => a.userId);
  const date = s?.expires ? fmtDate(Date.parse(s.expires), { dateStyle: "medium" }) : null;
  const price = cost !== null ? fmtNum(cost) : "";
  const question =
    ask === "off" ? { title: t("servers.offTitle"), text: t("servers.offText", { name }), label: t("servers.turnOff"), go: () => void update({ active: false }) }
    : ask === "newLink" ? { title: t("servers.newLinkTitle"), text: t("servers.newLinkText", { name }), label: t("servers.newLink"), go: () => void update({ newJoinCode: true }) }
    : ask === "renew" && cost !== null ? { title: t("servers.renewTitle"), text: t("servers.renewText", { name, price, owner: at }), label: t("servers.renewConfirm", { price }), go: () => void renew(true, cost) }
    : null;

  const settings = q.isPending ? (
    <>
      <span className="sr-only">{t("common.loading")}</span>
      <div aria-hidden="true" className="flex flex-col gap-5">
        <div className="flex flex-col gap-1.5">
          <Skeleton width={48} />
          <Skeleton height={40} radius={12} />
        </div>
        {[0, 1, 2].map((i) => (
          <div key={i} className="flex items-center justify-between">
            <Skeleton width={140} />
            <Skeleton width={40} height={24} radius={999} />
          </div>
        ))}
        <Skeleton height={96} radius={12} />
        <Skeleton height={72} radius={12} />
      </div>
    </>
  ) : !s ? (
    <div className="flex flex-col items-start gap-3">
      <p role="alert" className="alert">{friendly(asError(q.error))}</p>
      <Button size="md" icon="refresh" busy={q.isFetching} disabled={q.isFetching} onClick={() => void q.refetch()}>{t("common.tryAgain")}</Button>
    </div>
  ) : (
    <>
      <label className="flex flex-col gap-1.5">
        <span className="label">{t("servers.name")}</span>
        <TextField label={t("servers.name")} value={s.name} mono={false} onCommit={rename} />
      </label>
      <div>
        <div className="setting" data-asks="off">
          <span className="setting-text">{t("servers.serverOn")}</span>
          <Switch label={t("servers.serverOn")} checked={s.active} onChange={(on) => (on ? void update({ active: true }) : setAsk("off"))} />
        </div>
        <div className="setting">
          <span className="setting-text">{t("servers.friendsCanJoin")}</span>
          <Switch label={t("servers.friendsCanJoin")} checked={s.friendsAllowed} onChange={(on) => void update({ friendsAllowed: on })} />
        </div>
        <div className="setting" data-asks="newLink">
          <span className="setting-text">{t("servers.inviteLink")}</span>
          <Button size="sm" disabled={busy} onClick={() => setAsk("newLink")}>{t("servers.newLink")}</Button>
        </div>
      </div>
      <section aria-labelledby={allowedId} className="flex flex-col gap-2">
        <h3 id={allowedId} className="label">{t("servers.allowedPlayers")}</h3>
        <div className="well scroll max-h-[208px] p-1">
          {s.allowedUsers.length === 0 && <p className="px-3 py-2.5 text-[13px] text-fg-3">{t("servers.noAllowed")}</p>}
          <ListPresence
            items={s.allowedUsers}
            getKey={(u) => u.userId}
            render={(u) => (
              <div className="flex items-center gap-2.5 min-h-11 ps-3 pe-1">
                <span className="flex-1 min-w-0 truncate">{u.displayName || u.username}</span>
                <span className="text-fg-3 truncate">@{u.username}</span>
                <IconButton
                  icon="x"
                  size="sm"
                  label={t("servers.removeName", { name: u.username })}
                  disabled={busy}
                  onClick={() => void update({ usersToRemove: [u.userId] })}
                />
              </div>
            )}
          />
        </div>
        <Button size="sm" className="self-start" disabled={busy || !toAdd.length} onClick={() => void update({ usersToAdd: toAdd })}>
          {t("servers.allowMine", { n: toAdd.length })}
        </Button>
      </section>
      <div className="well flex flex-col gap-1.5 px-4 py-3" data-asks="renew">
        <div className="flex items-center justify-between gap-3">
          <span>{t("servers.autoRenew")}</span>
          <Switch
            label={t("servers.autoRenew")}
            checked={s.renews}
            disabled={cost === null}
            describedBy={cost === null ? noPriceId : undefined}
            onChange={(on) => cost !== null && (on ? setAsk("renew") : void renew(false, cost))}
          />
        </div>
        <p className="meta">
          {s.expired ? t("servers.subExpired") : s.renews ? t("servers.subRenews") : t("servers.subEnds")}
          {date && <span className="mono"> {date}</span>}
          {cost !== null && <span className="mono"> · {t("servers.pricePerMonth", { price })}</span>}
        </p>
        {cost === null && <p id={noPriceId} className="meta">{t("servers.noPrice")}</p>}
      </div>
    </>
  );

  return (
    // Esc, the backdrop and X leave a question for the settings, as its Cancel does.
    <Dialog open={!!server && !!owner} onClose={question ? () => !busy && setAsk(null) : close} title={question?.title ?? t("servers.manageTitle", { name, owner: at })} width={520}>
      <div ref={steps}>
        <DialogSteps step={question ? ask! : "main"} direction={question ? 1 : -1}>
          {question ? (
            <ConfirmStep text={question.text} confirmLabel={question.label} busy={busy} onCancel={() => setAsk(null)} onConfirm={question.go} />
          ) : (
            <div className="dlg-body">
              <div className="flex flex-col gap-5">{settings}</div>
              <Collapse open={!!error}>
                <p role="alert" className="alert pt-4">{error}</p>
              </Collapse>
            </div>
          )}
        </DialogSteps>
      </div>
    </Dialog>
  );
}
