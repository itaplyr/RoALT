import { useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "../../api";
import { qk, useInstances } from "../../data";
import { ConfirmDialog } from "../../ui/Dialog";
import { Collapse } from "../../ui/motion";
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

/** The newer release on offer (self_update), or null: none known, or Later hid it. */
export function useUpdateOffer() {
  // The start-up check can finish before the app://update listener exists: ask the cached
  // result again (no network) until an update is known.
  const version = useQuery({ queryKey: qk.appUpdate, queryFn: api.selfUpdateStatus, refetchInterval: (q) => (q.state.data?.available ? false : 5_000) }).data?.available;
  const later = useSyncExternalStore(subscribe, () => hidden);
  return version && version !== later ? version : null;
}

/** Installing an update: asks first while clients run (they keep running and are followed again).
 * The confirm lives outside the island sheet: a modal closes open popovers. */
export function useUpdateInstall(version: string | null) {
  const running = useInstances().data?.filter(isRunning).length ?? 0;
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  // Resolves just before RoALT quits for the installer, which starts it again.
  async function install() {
    setConfirm(false);
    setBusy(true);
    try {
      await api.selfUpdateInstall();
    } catch (e) {
      toastError(e);
      setBusy(false);
    }
  }
  return {
    busy,
    start: () => (running ? setConfirm(true) : void install()),
    later: () => version && hide(version),
    dialog: (
      <ConfirmDialog
        open={confirm && !!version}
        onClose={() => setConfirm(false)}
        onConfirm={() => void install()}
        title={t("settings.updateConfirmTitle", { version: version ?? "" })}
        text={t("settings.updateConfirmText", { count: running })}
        confirmLabel={t("settings.updateNow")}
        safe
      />
    ),
  };
}

/** What the island's update sheet says; while it downloads, that it does, over a running bar. */
export function UpdateText({ version, busy }: { version: string; busy: boolean }) {
  return (
    <div role="status">
      <p>{tx("settings.updateReady", { version: <span className="mono">{version}</span> })}</p>
      <Collapse open={busy}>
        <p className="pt-1 text-[13px] text-fg-3">{t("settings.updateDownloading")}</p>
        <span aria-hidden="true" className="live-bar block mt-2" />
      </Collapse>
    </div>
  );
}
