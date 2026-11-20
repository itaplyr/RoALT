import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api, type MultiInstanceStatus } from "../../api";
import { qk, useMultiStatus } from "../../data";
import { t } from "../../i18n";
import { ConfirmDialog } from "../../ui/Dialog";
import { readStored, writeStored } from "../../ui/stored";
import { toastError } from "../../ui/Toast";

const NOTICE_KEY = "roalt.multiInstanceNotice";

/** Turns multi-instance on or off. The first time it is turned on, Roblox's stance on several
 * clients is shown first; render `dialog` next to the switch. */
/** `onError`: where a failed switch is told (a toast unless the caller shows it at its row). */
export function useMultiToggle(onStatus?: (s: MultiInstanceStatus) => void, onError: (e: unknown) => void = toastError) {
  const qc = useQueryClient();
  const [asking, setAsking] = useState(false);
  const set = (enabled: boolean) =>
    api.multiInstanceSet(enabled).then((s) => {
      qc.setQueryData(qk.multi, s);
      qc.invalidateQueries({ queryKey: qk.settings });
      onStatus?.(s);
    }, onError);
  const toggle = (enabled: boolean) => (enabled && readStored(NOTICE_KEY) !== "1" ? setAsking(true) : void set(enabled));
  const dialog = (
    <ConfirmDialog
      open={asking}
      onClose={() => setAsking(false)}
      onConfirm={() => {
        writeStored(NOTICE_KEY, "1");
        setAsking(false);
        void set(true);
      }}
      title={t("launch.multi.turnOnTitle")}
      text={t("launch.multi.turnOnText")}
      confirmLabel={t("launch.multi.turnOn")}
      safe
    />
  );
  return { toggle, dialog };
}

/** Says when multi-instance stands in the way, with the way out: turned off while several accounts
 * are `wanted` or a launch `replaces` the running client, or wanted but held up by a Roblox that
 * already runs. */
export function MultiNotice({ wanted, replaces, className = "" }: { wanted: boolean; replaces?: boolean; className?: string }) {
  const qc = useQueryClient();
  const status = useMultiStatus().data;
  const [confirm, setConfirm] = useState<"close" | "takeOver" | null>(null);
  const [busy, setBusy] = useState(false);
  const apply = (s: MultiInstanceStatus) => qc.setQueryData(qk.multi, s);
  const { toggle, dialog } = useMultiToggle();
  const run = () => {
    setBusy(true);
    (confirm === "takeOver" ? api.multiInstanceTakeOver() : api.robloxCloseAll(false))
      .then(apply, toastError)
      .finally(() => {
        setBusy(false);
        setConfirm(null);
      });
  };

  if (!status) return null;
  const blocked = status.enabled && !status.active;
  const off = (wanted || replaces) && !status.enabled;
  return (
    <>
      {(blocked || off) && (
        <div className={`enter notice ${className}`} data-tone={blocked ? "attention" : undefined}>
          {off ? (
            <>
              <div className="flex-[1_1_320px] min-w-0">
                <p className="font-medium">{wanted ? t("launch.multi.needTitle") : t("launch.multi.replaceTitle")}</p>
                <p className="text-fg-2 text-[13px]">{wanted ? t("launch.multi.needText") : t("launch.multi.replaceText")}</p>
              </div>
              <button className="btn btn-sm" onClick={() => toggle(true)}>{t("launch.multi.turnOn")}</button>
            </>
          ) : (
            <>
              <span role="alert" className="alert flex-[1_1_320px] min-w-0">{status.message}</span>
              {status.blockedBy.length > 0 ? (
                <div className="flex flex-wrap gap-2">
                  <button className="btn btn-sm" onClick={() => setConfirm("close")}>
                    {t("launch.multi.closeRobloxEllipsis")}
                  </button>
                  <button className="btn btn-sm" onClick={() => setConfirm("takeOver")}>
                    {t("launch.multi.takeOverEllipsis")}
                  </button>
                </div>
              ) : (
                <button className="btn btn-sm" onClick={() => api.multiInstanceSet(true).then(apply, toastError)}>{t("common.tryAgain")}</button>
              )}
            </>
          )}
        </div>
      )}
      <ConfirmDialog
        open={confirm === "close"}
        onClose={() => !busy && setConfirm(null)}
        onConfirm={run}
        busy={busy}
        title={t("launch.multi.closeTitle")}
        text={t("launch.multi.closeText", { count: status.blockedBy.length })}
        confirmLabel={t("launch.multi.closeRoblox")}
      />
      <ConfirmDialog
        open={confirm === "takeOver"}
        onClose={() => !busy && setConfirm(null)}
        onConfirm={run}
        busy={busy}
        title={t("launch.multi.takeOverTitle")}
        text={t("launch.multi.takeOverText", { count: status.blockedBy.length })}
        confirmLabel={t("launch.multi.takeOver")}
      />
      {dialog}
    </>
  );
}
