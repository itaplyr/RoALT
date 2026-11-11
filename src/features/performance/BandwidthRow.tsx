import { useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, asError, type BandwidthStatus } from "../../api";
import { qk } from "../../data";
import { fmtNum, t } from "../../i18n";
import { Skeleton } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { OffOrNumberField } from "../../ui/controls";
import { Morph, Swap } from "../../ui/motion";
import { RANGES } from "../../ui/ranges";
import { TONE_COLOR } from "../../ui/Status";
import { friendly } from "../../ui/Toast";
import { useRow } from "../settings/kit/rowError";
import { SettingRow } from "../settings/kit/SettingRow";

const KBPS = "kbit/s";

/** Off, in force, or set but lapsed (Windows restarted since): shape and words. */
function State({ status }: { status: BandwidthStatus }) {
  const [tone, shape, text] = !status.kbps
    ? (["neutral", "ring", t("settings.bandwidthStateOff")] as const)
    : status.active
      ? (["ok", "dot", t("settings.bandwidthInForce", { kbps: fmtNum(status.kbps) })] as const)
      : (["warn", "triangle", t("settings.bandwidthStale", { kbps: fmtNum(status.kbps) })] as const);
  return (
    <Swap k={text}>
      <span className="set-net" style={{ color: TONE_COLOR[tone] }}>
        <span aria-hidden="true" className="status-mark" data-shape={shape} />
        <span className="text-fg-2">{text}</span>
      </span>
    </Swap>
  );
}

/** Off, or a limit in range, as every such field; but a choice here is only a draft until its button,
 * which has Windows ask for admin rights. The button shows while there is something to apply. */
function Controls({ status }: { status: BandwidthStatus }) {
  const row = useRow();
  const qc = useQueryClient();
  const button = useRef<HTMLButtonElement>(null);
  const [draft, setDraft] = useState(status.kbps);
  const [was, setWas] = useState(status);
  if (was !== status) {
    setWas(status);
    setDraft(status.kbps);
  }
  const [busy, setBusy] = useState(false);
  // A new limit, none, or the same one again once Windows has dropped it.
  const pending = draft !== status.kbps || (draft > 0 && !status.active);

  const apply = () => {
    if (busy) return;
    setBusy(true);
    row.setError(null);
    api
      .bandwidthApply(draft)
      .then((st) => {
        // The button goes once it is done: the focus stays in the row, on the switch.
        if (document.activeElement === button.current) button.current?.closest(".set-control")?.querySelector<HTMLElement>("[role=switch]")?.focus();
        qc.setQueryData(qk.bandwidth, st);
        void qc.invalidateQueries({ queryKey: qk.settings });
      }, row.fail)
      .finally(() => setBusy(false));
  };

  // The button comes in before the field, so the switch stays where it was. Its own Morph beside the
  // field's, not around it: nested, the outer spring chases the inner.
  return (
    <span className="flex items-center justify-end">
      <Morph>
        {pending && (
          <Button ref={button} size="md" className="me-3" busy={busy} aria-disabled={busy || undefined} aria-description={t("settings.adminRights")} onClick={apply}>
            <Swap k={busy ? "busy" : draft ? "apply" : "remove"}>{t(busy ? "settings.waitingWindows" : draft ? "settings.bandwidthApply" : "settings.bandwidthRemove")}</Swap>
          </Button>
        )}
      </Morph>
      <OffOrNumberField
        label={t("settings.bandwidth")}
        value={draft}
        range={RANGES.bandwidthKbps}
        unit={KBPS}
        short={KBPS}
        onCommit={(n) => {
          // A new draft: the last attempt's error is about another.
          row.setError(null);
          setDraft(n);
        }}
        describedBy={row.describedBy}
      />
    </span>
  );
}

/** Roblox's upload limit, a Windows QoS policy: what is in force now, and the limit to apply. */
export function BandwidthRow() {
  const status = useQuery({ queryKey: qk.bandwidth, queryFn: api.bandwidthStatus, staleTime: 0 });
  const s = status.data;
  return (
    <SettingRow
      id="bandwidth"
      detail={
        status.isError ? (
          <p className="alert">{friendly(asError(status.error))}</p>
        ) : (
          <div role="status">{s ? <State status={s} /> : <Skeleton width={150} height={12} className="mt-1" />}</div>
        )
      }
    >
      {status.isError ? (
        <Button size="md" busy={status.isFetching} onClick={() => void status.refetch()}>
          {t("common.tryAgain")}
        </Button>
      ) : s ? (
        <Controls status={s} />
      ) : (
        <Skeleton width={40} height={24} radius={999} />
      )}
    </SettingRow>
  );
}
