import { save as saveFile } from "@tauri-apps/plugin-dialog";
import { t } from "../../i18n";
import { toast, toastError } from "../../ui/Toast";

/** Saves a CSV the backend writes, where the user says. */
export async function exportCsv(name: string, write: (path: string) => Promise<void>) {
  try {
    const path = await saveFile({ defaultPath: `${name}-${new Date().toISOString().slice(0, 10)}.csv`, filters: [{ name: "CSV", extensions: ["csv"] }] });
    if (!path) return;
    await write(path);
    toast(t("common.exported"));
  } catch (e) {
    toastError(e);
  }
}