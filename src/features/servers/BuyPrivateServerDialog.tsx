import { useEffect, useRef, useState, type FormEvent } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, asError, type AccountSummary, type GameInfo } from "../../api";
import { Button } from "../../ui/Button";
import { Dialog, DialogSteps } from "../../ui/Dialog";
import { Field, Input, Select } from "../../ui/Field";
import { Collapse } from "../../ui/motion";
import { friendly, toast, toastError } from "../../ui/Toast";
import { fmtNum, t } from "../../i18n";
import { accountName } from "../accounts/shared";
import { ConfirmStep, useAskFocus } from "./PrivateServerDialog";

/** The idempotency key of a purchase that may have gone through, per `accountId:universeId`: the
 *  next try (even after reopening) with the same name and price is the same purchase to Roblox. */
const unsure = new Map<string, { key: string; name: string; robux: number }>();

/** Buys a private server of `game` (spends Robux). There's no price endpoint, so the user types the
 *  price from the game's page; Roblox refuses, charging nothing, when it isn't the real one. The
 *  purchase is asked about once more, as the dialog's second step. */
export function BuyPrivateServerDialog({ open, onClose, game, accounts, payer, onBought }: {
  open: boolean;
  onClose: () => void;
  game: GameInfo;
  /** Valid vault accounts, the ones that can pay. */
  accounts: AccountSummary[];
  /** Picked first when it opens. */
  payer: number | undefined;
  onBought: () => void;
}) {
  const [accountId, setAccountId] = useState(0);
  const [name, setName] = useState("");
  const [price, setPrice] = useState("");
  // `on`: the field it is about; none, the form's.
  const [error, setError] = useState<{ on?: "name" | "price"; text: string } | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const sending = useRef(false);
  const steps = useAskFocus<HTMLDivElement>(confirming ? "buy" : null);

  useEffect(() => {
    if (!open) return;
    setAccountId(payer ?? accounts[0]?.id ?? 0);
    setName("");
    setPrice("");
    setError(null);
    setConfirming(false);
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  const account = accounts.find((a) => a.id === accountId);
  const robux = Number(price);

  function review(e: FormEvent) {
    e.preventDefault();
    if (!account) return setError({ text: t("servers.buyNeedAccount") });
    if (!name.trim()) return setError({ on: "name", text: t("servers.needName") });
    if (!price.trim() || !Number.isInteger(robux) || robux < 0) return setError({ on: "price", text: t("servers.buyNeedPrice") });
    setError(null);
    setConfirming(true);
  }

  async function buy() {
    // A ref, not `busy`: two clicks before the next render would both see busy false.
    if (sending.current || !account) return;
    sending.current = true;
    setBusy(true);
    const pair = `${account.id}:${game.universeId}`;
    const prev = unsure.get(pair);
    const key = prev && prev.name === name.trim() && prev.robux === robux ? prev.key : crypto.randomUUID();
    try {
      await api.privateServerBuy({ accountId: account.id, universeId: game.universeId, name: name.trim(), expectedPrice: robux, idempotencyKey: key });
      unsure.delete(pair);
      toast(t("servers.bought", { name: name.trim() }));
      onBought();
      setConfirming(false);
      onClose();
    } catch (err) {
      setConfirming(false);
      const e = asError(err);
      // No answer, or a 5xx: Roblox may have charged. Anything else is a refusal that charged nothing.
      if (e.kind === "network" || e.kind === "roblox") {
        unsure.set(pair, { key, name: name.trim(), robux });
        toast(t("servers.buyMaybe"), "error");
        onBought();
        onClose();
      } else {
        unsure.delete(pair);
        setError({ text: friendly(e) });
      }
    } finally {
      sending.current = false;
      setBusy(false);
    }
  }

  const fields = (
    <form onSubmit={review} className="dlg-body flex flex-col gap-4">
      <p className="text-fg-2 text-pretty">{t("servers.buyIntro", { game: game.name })}</p>
      <Field label={t("servers.payingAccount")}>
        {({ id }) => (
          <Select
            id={id}
            value={String(accountId)}
            onChange={(v) => setAccountId(Number(v))}
            options={accounts.map((a) => ({ value: String(a.id), label: `${accountName(a)} (@${a.username})` }))}
          />
        )}
      </Field>
      <Field label={t("servers.name")} error={error?.on === "name" && error.text}>
        {({ id, describedBy, invalid }) => (
          <Input
            id={id}
            aria-describedby={describedBy}
            invalid={invalid}
            maxLength={50}
            autoComplete="off"
            value={name}
            onChange={(e) => {
              setName(e.target.value);
              setError(null);
            }}
          />
        )}
      </Field>
      <Field label={t("servers.priceRobux")} error={error?.on === "price" && error.text}>
        {({ id, describedBy, invalid }) => (
          <div className="flex flex-wrap items-center gap-3">
            <Input
              id={id}
              aria-describedby={describedBy}
              invalid={invalid}
              mono
              className="w-32"
              type="number"
              min={0}
              step={1}
              value={price}
              onWheel={(e) => e.currentTarget.blur()}
              onChange={(e) => {
                setPrice(e.target.value);
                setError(null);
              }}
            />
            <Button size="md" icon="external" onClick={() => openUrl(`https://www.roblox.com/games/${game.placeId}`).catch(toastError)}>
              {t("servers.openOnRoblox")}
            </Button>
          </div>
        )}
      </Field>
      <div>
        <Collapse open={!!error && !error.on}>
          <p role="alert" className="alert pb-4">{error?.text}</p>
        </Collapse>
        <div className="flex justify-end gap-2 pt-1">
          <Button onClick={onClose} disabled={busy}>{t("common.cancel")}</Button>
          <Button type="submit" variant="primary" data-asks="buy" disabled={busy || !accounts.length}>{t("servers.buyReview")}</Button>
        </div>
      </div>
    </form>
  );

  return (
    // Esc, the backdrop and X leave the question for the form, as its Cancel does.
    <Dialog open={open} onClose={() => !busy && (confirming ? setConfirming(false) : onClose())} title={confirming ? t("servers.buyConfirmTitle") : t("servers.buyTitle")} width={460}>
      <div ref={steps}>
        <DialogSteps step={confirming ? "confirm" : "form"} direction={confirming ? 1 : -1}>
          {confirming ? (
            <ConfirmStep
              text={t("servers.buyConfirmText", { name: name.trim(), price: fmtNum(robux), owner: account?.username ?? "" })}
              confirmLabel={busy ? t("servers.buying") : t("servers.buyFor", { price: fmtNum(robux) })}
              busy={busy}
              onCancel={() => setConfirming(false)}
              onConfirm={() => void buy()}
            />
          ) : (
            fields
          )}
        </DialogSteps>
      </div>
    </Dialog>
  );
}
