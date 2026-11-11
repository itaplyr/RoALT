import { useState } from "react";
import { api } from "../../api";
import { useAccounts, useInstances, useSettings } from "../../data";
import { t } from "../../i18n";
import { toast, toastError } from "../../ui/Toast";
import { isRunning } from "../launch/Running";

/** The window tools for every running client (Home's toolbar, the island's running sheet). One runs at
 * a time per caller (`busy`). Tile says when it laid out fewer windows than it could. */
export function useWindowTools() {
  const settings = useSettings().data;
  const accounts = useAccounts().data;
  const running = (useInstances().data ?? []).filter(isRunning);
  const [busy, setBusy] = useState(false);
  const act = (f: () => Promise<unknown>) => {
    setBusy(true);
    return f().catch(toastError).finally(() => setBusy(false));
  };
  return {
    busy,
    tile: (micro: boolean) =>
      act(() =>
        api.instancesTile(micro).then((n) => {
          // With a window layout on, the accounts the user plays are left out on purpose: not "skipped".
          const layoutOn = (settings?.windowLayout ?? "off") !== "off";
          const could = running.filter((i) => !(layoutOn && accounts?.find((a) => a.id === i.accountId)?.fullQuality)).length;
          if (n < could) toast(t("dashboard.laidOut", { count: n }));
        }),
      ),
    minimizeAll: () => act(() => api.instancesShowAll("minimize")),
    restoreAll: () => act(() => api.instancesShowAll("restore")),
  };
}
