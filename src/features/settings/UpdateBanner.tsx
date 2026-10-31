import { useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "../../api";
import { qk, useInstances } from "../../data";
import { ConfirmDialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { toastError } from "../../ui/Toast";
import { isRunning } from "../launch/Running";
import { t, tx } from "../../i18n";

/** The version Later hid: not offered again until RoALT restarts, or Check now finds it. */
let hidden: string | null = null;
const subs = new Set<() => void>();
const hide = (version: string | null) => {
  hidden = version;
  subs.forEach((f) => f());
};
const subscribe = (f: () => void) => {
  subs.add(f);
  return () => void subs.delete(f);
};
export const unhideUpdate = () => hide(null);

/** "RoALT x.y.z is ready" under the title bar while a newer release is known (self_update). */
export function UpdateBanner() {
  // The start-up check can finish before the app://update listener exists: ask the cached
  // result again (no network) until an update is known.
  const version = useQuery({ queryKey: qk.appUpdate, queryFn: api.selfUpdateStatus, refetchInterval: (q) => (q.state.data?.available ? false : 5_000) }).data?.available;
  const running = useInstances().data?.filter(isRunning).length ?? 0;
  const later = useSyncExternalStore(subscribe, () => hidden);
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  if (!version || version === later) return null;

  // Resolves just before RoALT quits for the installer, which starts it again.
  async function update() {
    setConfirm(false);
    setBusy(true);
    try {
      await api.selfUpdateInstall();
    } catch (e) {
      toastError(e);
      setBusy(false);
    }
  }

  return (
    <div role="status" className="enter panel flex-none flex flex-wrap items-center gap-x-4 gap-y-2 mx-3 mt-1 mb-2 ps-4 pe-3 py-2">
      <Icon name="download" />
      <span className="flex-1 min-w-0">
        {tx("settings.updateReady", { version: <span className="mono">{version}</span> })}
        {busy && <span className="text-dim"> {t("settings.updateDownloading")}</span>}
      </span>
      <button className="btn btn-sm btn-primary" disabled={busy} onClick={() => (running ? setConfirm(true) : void update())}>
        {t("settings.updateNow")}
      </button>
      <button className="btn btn-sm" disabled={busy} onClick={() => hide(version)}>
        {t("settings.updateLater")}
      </button>
      <ConfirmDialog
        open={confirm}
        onClose={() => setConfirm(false)}
        onConfirm={() => void update()}
        title={t("settings.updateConfirmTitle", { version })}
        text={t("settings.updateConfirmText", { count: running })}
        confirmLabel={t("settings.updateNow")}
        safe
      />
    </div>
  );
}
