import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api, type VaultStatus } from "../../api";
import { qk } from "../../data";
import { announce } from "../../ui/a11y";
import { Dialog, DialogSteps } from "../../ui/Dialog";
import { StatusPill } from "../../ui/Status";
import { t } from "../../i18n";
import { PasswordForm } from "./PasswordDialog";
import { RecoveryCodeStep } from "./RecoveryCodeDialog";
import { publishRecoveryCode, recoveryCodeSaved, useRecoveryCodeClaim } from "./recoveryCode";
import { useOpenCount } from "./useOpenCount";

/** "setMaster": from Windows protection to a master password. "recovery": create or replace the code. */
export type VaultFlow = "setMaster" | "recovery";
type Step = "password" | "offer" | "code";

/**
 * A vault flow in one dialog whose body morphs from step to step.
 * - setMaster: a new password and its confirmation, then "Create a recovery code?" (made with the
 *   password just set, no retype), then the code.
 * - recovery: the current password, then the new code (replacing the old one, if any).
 * Keep it mounted and drive `flow`; each opening starts over. A new code is published at once, so a
 * lock mid-flow (which unmounts this) can't lose it: RecoveryCodeHost shows it after the unlock.
 */
export function VaultFlowDialog({ flow, onClose }: { flow: VaultFlow | null; onClose: () => void }) {
  const opened = useOpenCount(!!flow);
  return <Flow key={opened} flow={flow} onClose={onClose} />;
}

function Flow({ flow, onClose }: { flow: VaultFlow | null; onClose: () => void }) {
  const qc = useQueryClient();
  // As it was when the flow opened: making the code changes it.
  const [replacing] = useState(() => !!qc.getQueryData<VaultStatus>(qk.vault)?.hasRecovery);
  const [step, setStep] = useState<Step>("password");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [refused, setRefused] = useState(0);
  // The password just set, held for the offer step only.
  const held = useRef("");
  useRecoveryCodeClaim(!!flow);
  useEffect(() => () => void (held.current = ""), []);

  const go = (to: Step, said: string) => {
    setStep(to);
    announce(said);
  };
  const close = () => {
    if (busy) return;
    if (step === "code" && !saved) return setRefused((n) => n + 1);
    held.current = "";
    if (step === "code") recoveryCodeSaved();
    // Closed, it keeps no code (one can reset the password); the Dialog's own copy covers the fade.
    setCode("");
    onClose();
  };
  const created = (fresh: string) => {
    publishRecoveryCode(fresh);
    qc.invalidateQueries({ queryKey: qk.vault });
    setCode(fresh);
  };

  const title =
    step === "code" ? t("vault.recovery.showTitle")
    : step === "offer" ? t("vault.recovery.offerTitle")
    : flow === "setMaster" ? t("settings.setMasterTitle")
    : t(replacing ? "vault.recovery.replaceTitle" : "vault.recovery.createTitle");
  const shared = { onCancel: close, onBusy: setBusy };

  return (
    <Dialog open={!!flow} onClose={close} title={title} width={460}>
      {flow && (
        <DialogSteps step={step}>
          {step === "code" ? (
            <RecoveryCodeStep code={code} saved={saved} onSaved={setSaved} refused={refused} onDone={close} />
          ) : step === "offer" ? (
            <PasswordForm
              {...shared}
              text={
                <>
                  <StatusPill tone="ok" label={t("settings.setMasterDone")} className="flex w-fit mb-3" />
                  {t("vault.recovery.offerText")}
                </>
              }
              submitLabel={t("vault.recovery.create")}
              cancelLabel={t("vault.recovery.skip")}
              onSubmit={async () => {
                created(await api.vaultRecoveryCreate(held.current));
                held.current = "";
              }}
              onDone={() => go("code", t("vault.recovery.showTitle"))}
            />
          ) : flow === "setMaster" ? (
            <PasswordForm
              {...shared}
              text={t("settings.setMasterText")}
              submitLabel={t("settings.setMasterSubmit")}
              next
              nextLabel={t("settings.masterPassword")}
              onSubmit={async ({ next }) => {
                await api.vaultSetMode("master", undefined, next);
                held.current = next;
                qc.invalidateQueries({ queryKey: qk.vault });
              }}
              onDone={() => go("offer", `${t("settings.setMasterDone")} ${t("vault.recovery.offerTitle")}`)}
            />
          ) : (
            <PasswordForm
              {...shared}
              text={t(replacing ? "vault.recovery.replaceText" : "vault.recovery.createText")}
              submitLabel={t(replacing ? "vault.recovery.replace" : "vault.recovery.create")}
              current
              onSubmit={async ({ current }) => created(await api.vaultRecoveryCreate(current))}
              onDone={() => go("code", t("vault.recovery.showTitle"))}
            />
          )}
        </DialogSteps>
      )}
    </Dialog>
  );
}
