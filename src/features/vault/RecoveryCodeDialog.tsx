import { useEffect, useState } from "react";
import { save as saveFile } from "@tauri-apps/plugin-dialog";
import { api } from "../../api";
import { Dialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { toast, toastError } from "../../ui/Toast";
import { t } from "../../i18n";

/** Shows a new recovery code, once. It won't close until the user says it is saved. */
export function RecoveryCodeDialog({ code, onClose }: { code: string | null; onClose: () => void }) {
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    if (code) setSaved(false);
  }, [code]);

  const close = () => {
    if (saved) onClose();
  };

  async function saveToFile() {
    if (!code) return;
    try {
      const path = await saveFile({ defaultPath: "RoALT-recovery-code.txt", filters: [{ name: t("vault.recovery.fileFilter"), extensions: ["txt"] }] });
      if (!path) return;
      await api.vaultRecoverySave(path, code);
      toast(t("vault.recovery.savedTo"));
    } catch (e) {
      toastError(e);
    }
  }

  return (
    <Dialog open={!!code} onClose={close} title={t("vault.recovery.showTitle")} width={460}>
      <div className="flex flex-col gap-4 px-5 pb-5">
        <p className="text-dim">{t("vault.recovery.showText")}</p>
        <output className="selectable mono block text-center text-[22px] tracking-[0.12em] py-3 rounded-md border border-line" aria-label={t("vault.recovery.code")}>
          {code}
        </output>
        <div className="flex flex-wrap gap-2">
          <button type="button" className="btn flex-1" onClick={() => code && api.vaultRecoveryCopy(code).then(() => toast(t("vault.recovery.copied")), toastError)}>
            <Icon name="copy" size={14} />
            {t("common.copy")}
          </button>
          <button type="button" className="btn flex-1" onClick={saveToFile}>
            <Icon name="download" size={14} />
            {t("vault.recovery.saveFile")}
          </button>
        </div>
        <ul className="list-disc ps-4 flex flex-col gap-1.5 text-[13px] text-dim">
          <li>{t("vault.recovery.limitFile")}</li>
          <li>{t("vault.recovery.limitPc")}</li>
        </ul>
        <label className="flex items-center gap-2.5 cursor-pointer">
          <input type="checkbox" className="cb" checked={saved} onChange={(e) => setSaved(e.target.checked)} />
          <span>{t("vault.recovery.savedTick")}</span>
        </label>
        <div className="flex justify-end">
          <button type="button" className="btn btn-primary" disabled={!saved} onClick={close}>{t("common.done")}</button>
        </div>
      </div>
    </Dialog>
  );
}
