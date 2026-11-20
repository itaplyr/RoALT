import { useState } from "react";
import type { AccountSummary } from "../../api";
import { useInstances } from "../../data";
import { t } from "../../i18n";
import { ConfirmDialog } from "../../ui/Dialog";
import { accountName } from "../accounts/shared";
import { isLive } from "./Running";

/** A launch of an account that is already playing: asked first, then `go` gets its client's id
 * (`launchOne`'s `moveFrom`), which closes before it launches again. Render `dialog` beside the
 * button. `liveOf`: the account's playing client, if it has one. */
export function useMoveConfirm() {
  const instances = useInstances().data ?? [];
  const [asking, setAsking] = useState<{ account: AccountSummary; go: (moveFrom: string) => void } | null>(null);
  const liveOf = (a: AccountSummary | null | undefined) => (a ? instances.find((i) => i.accountId === a.id && isLive(i)) : undefined);
  const name = asking ? accountName(asking.account) : "";
  const dialog = (
    <ConfirmDialog
      open={!!asking}
      onClose={() => setAsking(null)}
      onConfirm={() => {
        const inst = asking && liveOf(asking.account);
        setAsking(null);
        // Its client may have ended while the question was open: then it is a plain launch.
        asking?.go(inst?.id ?? "");
      }}
      title={t("move.title", { name })}
      text={t("move.text", { name })}
      confirmLabel={t("move.confirm")}
      safe
    />
  );
  return { liveOf, ask: (account: AccountSummary, go: (moveFrom: string) => void) => setAsking({ account, go }), dialog };
}
