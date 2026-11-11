import { useId, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api, asError, type AccountSummary, type VaultMode } from "../../api";
import type { CookieExport } from "../../bindings/CookieExport";
import { t } from "../../i18n";
import { Button } from "../../ui/Button";
import { Dialog, DialogSteps } from "../../ui/Dialog";
import { friendly, toast, toastError } from "../../ui/Toast";
import { PasswordForm } from "../vault/PasswordDialog";
import { Step } from "./AddAccountDialog";

type Stage = "confirm" | "password" | "done";

/**
 * "Export cookies" in one dialog that changes step: the warning, then the master password (or Windows
 * Hello, asked by the backend), then the file's path with Copy path, Show in folder and Delete now.
 * `ids`: the accounts to export, null for all; the dialog shows while it is set. Closing drops the
 * password and the path.
 */
export function ExportCookies({ ids, accounts, vaultMode, onClose }: {
  ids: number[] | null | undefined;
  accounts: AccountSummary[];
  vaultMode: VaultMode;
  onClose: () => void;
}) {
  const open = ids !== undefined;
  const [stage, setStage] = useState<Stage>("confirm");
  const [direction, setDirection] = useState<1 | -1>(1);
  const [result, setResult] = useState<CookieExport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [wasOpen, setWasOpen] = useState(open);
  if (open !== wasOpen) {
    setWasOpen(open);
    setStage("confirm");
    setResult(null);
    setError(null);
    setCopied(false);
  }
  const descId = useId();
  const title = stage === "done" && result ? t("accounts.exportDone", { count: result.count }) : t("accounts.exportTitle");
  // Said as the result step arrives (the focus jumps to Done), and on Copy path. A live region in the
  // dialog: the app's announcer sits behind the modal, where nothing is read.
  const said =
    stage === "done" && result
      ? [title, result.skipped > 0 && t("accounts.exportSkipped", { count: result.skipped }), copied && t("accounts.exportPathCopied")].filter(Boolean).join(". ")
      : "";
  const chosen = ids ? accounts.filter((a) => ids.includes(a.id)) : accounts;
  const main = chosen.find((a) => a.main);

  const go = (next: Stage, dir: 1 | -1 = 1) => {
    setDirection(dir);
    setStage(next);
    setError(null);
  };
  const exportNow = async (password?: string) => setResult(await api.accountsExportCookies(ids ?? undefined, password));
  const confirm = () => {
    if (vaultMode === "master") return go("password");
    // Windows Hello: the backend asks. A dismissed prompt stays quiet.
    setBusy(true);
    setError(null);
    exportNow()
      .then(() => go("done"))
      .catch((e) => {
        const err = asError(e);
        if (err.kind !== "cancelled") setError(friendly(err));
      })
      .finally(() => setBusy(false));
  };
  const deleteNow = () =>
    api.cookieExportDelete().then(
      () => {
        toast(t("accounts.exportDeleted"));
        onClose();
      },
      (e) => setError(friendly(asError(e))),
    );
  const alert = error && (
    <p role="alert" className="alert">
      {error}
    </p>
  );

  return (
    // Closing mid-request would not cancel it, so it waits.
    <Dialog open={open} onClose={() => !busy && onClose()} title={title} width={480} describedBy={stage === "confirm" ? `${descId}-text ${descId}-warn ${descId}-main` : undefined}>
      <DialogSteps step={stage} direction={direction}>
        {stage === "confirm" ? (
          <Step>
            <p id={`${descId}-text`} className="text-fg-2">{t("accounts.exportText", { count: chosen.length })}</p>
            <p id={`${descId}-warn`} className="alert">{t("accounts.exportWarning")}</p>
            {main && <p id={`${descId}-main`} className="alert">{t("accounts.removeMainWarning", { name: `@${main.username}` })}</p>}
            {alert}
            <div className="flex justify-end gap-2 pt-1">
              <Button data-first disabled={busy} onClick={onClose}>
                {t("common.cancel")}
              </Button>
              <Button variant="primary" icon="download" busy={busy} onClick={() => !busy && confirm()}>
                {t("accounts.export")}
              </Button>
            </div>
          </Step>
        ) : stage === "password" ? (
          <Step className="px-0 pb-0">
            <PasswordForm
              text={t("accounts.exportPassword")}
              submitLabel={t("accounts.export")}
              current
              cancelLabel={t("dashboard.back")}
              onSubmit={({ current }) => exportNow(current)}
              onDone={() => go("done")}
              onCancel={() => go("confirm", -1)}
              onBusy={setBusy}
            />
          </Step>
        ) : (
          result && (
            <Step>
              {result.skipped > 0 && <p className="alert">{t("accounts.exportSkipped", { count: result.skipped })}</p>}
              <div className="flex items-center gap-2">
                <input className="field mono flex-1 min-w-0" readOnly value={result.path} onFocus={(e) => e.currentTarget.select()} aria-label={t("accounts.exportPath")} />
                {/* Its outcome shows here: a toast would sit under the dialog's backdrop. */}
                <Button
                  icon={copied ? "check" : "copy"}
                  onClick={() =>
                    writeText(result.path).then(
                      () => setCopied(true),
                      (e) => setError(friendly(asError(e))),
                    )
                  }
                >
                  {t("accounts.exportCopyPath")}
                </Button>
              </div>
              <p className="text-[13px] text-fg-3">
                {t("accounts.exportDeletesAt", { time: new Date(result.deletesAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) })}
              </p>
              {alert}
              <div className="flex flex-wrap justify-end gap-2 pt-1">
                <Button variant="danger" icon="trash" onClick={deleteNow}>
                  {t("accounts.exportDeleteNow")}
                </Button>
                <Button icon="external" onClick={() => api.cookieExportReveal().catch(toastError)}>
                  {t("accounts.exportShow")}
                </Button>
                <Button variant="primary" data-first onClick={onClose}>
                  {t("common.done")}
                </Button>
              </div>
            </Step>
          )
        )}
      </DialogSteps>
      <p role="status" className="sr-only">
        {said}
      </p>
    </Dialog>
  );
}
