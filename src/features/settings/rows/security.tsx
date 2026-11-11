import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api } from "../../../api";
import { qk, useNetwork } from "../../../data";
import { t } from "../../../i18n";
import { Skeleton } from "../../../ui/Bits";
import { Button } from "../../../ui/Button";
import { Segmented, Switch } from "../../../ui/controls";
import { ConfirmDialog } from "../../../ui/Dialog";
import { Swap } from "../../../ui/motion";
import { PasswordDialog } from "../../vault/PasswordDialog";
import { VaultFlowDialog, type VaultFlow } from "../../vault/VaultFlowDialog";
import { useRow } from "../kit/rowError";
import { SettingRow } from "../kit/SettingRow";

/** Windows protection or a master password. Setting one is a flow of its own (password, then the
 * recovery code offer); removing it asks for it. The segment moves only once the vault has changed. */
function EncryptionControl() {
  const row = useRow();
  const qc = useQueryClient();
  const [flow, setFlow] = useState<VaultFlow | null>(null);
  const [removing, setRemoving] = useState(false);
  return (
    <>
      <Segmented
        label={t("settings.encryption")}
        value={row.ctx.vault.mode}
        options={[{ value: "dpapi", label: t("settings.encryptionDpapi") }, { value: "master", label: t("settings.masterPassword") }]}
        onChange={(m) => {
          // What the last flow did is old news once the next one starts.
          row.setResult(null);
          if (m === "master") setFlow("setMaster");
          else setRemoving(true);
        }}
      />
      <VaultFlowDialog flow={flow} onClose={() => setFlow(null)} />
      <PasswordDialog
        open={removing}
        onClose={() => setRemoving(false)}
        title={t("settings.removeMasterTitle")}
        text={t("settings.removeMasterText")}
        submitLabel={t("settings.removeMasterSubmit")}
        current
        onSubmit={async ({ current }) => {
          await api.vaultSetMode("dpapi", current);
          await qc.invalidateQueries({ queryKey: qk.vault });
          row.setResult(t("settings.removeMasterDone"));
        }}
      />
    </>
  );
}

export function EncryptionRow() {
  return (
    <SettingRow id="encryption">
      <EncryptionControl />
    </SettingRow>
  );
}

function ChangePassword() {
  const row = useRow();
  const [open, setOpen] = useState(false);
  const text = t("settings.changeMasterText");
  return (
    <>
      <Button size="md" aria-describedby={row.describedBy} onClick={() => setOpen(true)}>{t("settings.change")}</Button>
      <PasswordDialog
        open={open}
        onClose={() => setOpen(false)}
        title={t("settings.changeMasterTitle")}
        text={row.ctx.vault.hasRecovery ? `${text} ${t("vault.recovery.changeReminder")}` : text}
        submitLabel={t("settings.changeMasterSubmit")}
        current
        currentLabel={t("settings.currentPassword")}
        next
        onSubmit={async ({ current, next }) => {
          await api.vaultSetMode("master", current, next);
          row.setResult(t("settings.changeMasterDone"));
        }}
      />
    </>
  );
}

export function MasterPasswordRow() {
  return (
    <SettingRow id="masterPassword">
      <ChangePassword />
    </SettingRow>
  );
}

/** Create or replace the code: the current password, then the code, in one dialog. */
function RecoveryButton() {
  const row = useRow();
  const [flow, setFlow] = useState<VaultFlow | null>(null);
  return (
    <>
      <Button size="md" aria-describedby={row.describedBy} onClick={() => setFlow("recovery")}>
        {t(row.ctx.vault.hasRecovery ? "vault.recovery.replace" : "vault.recovery.create")}
      </Button>
      <VaultFlowDialog flow={flow} onClose={() => setFlow(null)} />
    </>
  );
}

export function RecoveryCodeRow() {
  return (
    <SettingRow id="recoveryCode">
      <RecoveryButton />
    </SettingRow>
  );
}

/** Off refuses launching on a Public network (and a master vault locks there). Turning it back on is
 * the risky way, so that one asks first. */
function PublicSwitch() {
  const { ctx, write, describedBy } = useRow();
  const [asking, setAsking] = useState(false);
  return (
    <>
      <Switch
        label={t("settings.public")}
        checked={ctx.s.allowPublicNetworks}
        describedBy={describedBy}
        onChange={(on) => (on ? setAsking(true) : void write({ allowPublicNetworks: false }))}
      />
      <ConfirmDialog
        open={asking}
        onClose={() => setAsking(false)}
        onConfirm={() => {
          setAsking(false);
          void write({ allowPublicNetworks: true });
        }}
        title={t("settings.publicConfirmTitle")}
        text={t("settings.publicConfirmText")}
        confirmLabel={t("settings.publicConfirm")}
      />
    </>
  );
}

/** Which network this PC is on now. */
function NetworkLine() {
  const network = useNetwork();
  if (network.isError) return null;
  if (!network.data) return <Skeleton width={180} height={12} className="mt-1" />;
  const on = network.data.public;
  return (
    <Swap k={on ? "public" : "private"}>
      <span className="set-net">
        <span aria-hidden="true" className="status-mark" data-shape={on ? "diamond" : "ring"} style={{ color: on ? "var(--attention)" : "var(--fg-3)" }} />
        {t(on ? "settings.publicNow" : "settings.publicNotNow")}
      </span>
    </Swap>
  );
}

export function PublicNetworkRow() {
  return (
    <SettingRow id="publicNetworks" detail={<NetworkLine />}>
      <PublicSwitch />
    </SettingRow>
  );
}
