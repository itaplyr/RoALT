import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api, type AccountSummary, type BulkStage, type Instance } from "../../api";
import { dismissBulk, useBulk, useInstances } from "../../data";
import { fmtNum, t, type Key } from "../../i18n";
import { Icon } from "../../ui/Icon";
import { toastError } from "../../ui/Toast";
import { accountName, Avatar } from "../accounts/shared";
import { describeTarget } from "./target";

const STEPS: Key[] = ["launch.batch.stepTicket", "launch.batch.stepRun", "launch.batch.stepJoin", "launch.batch.stepIn"];
type Kind = Exclude<BulkStage["kind"], "failed">;
/** The step in progress; 4 is all done, -1 none. */
const AT: Record<Kind, number> = { queued: -1, ticket: 0, starting: 1, joining: 2, inGame: 4, done: 4, cancelled: -1 };
const NOTE: Record<Kind, Key> = {
  queued: "launch.batch.queued",
  ticket: "launch.batch.ticket",
  starting: "launch.batch.starting",
  joining: "launch.batch.joining",
  inGame: "launch.batch.inGameNote",
  done: "launch.batch.doneNote",
  cancelled: "launch.batch.cancelled",
};

function steps(stage: BulkStage, started: boolean, done: boolean) {
  // A started client that failed got past the ticket.
  const at = stage.kind === "failed" ? (started ? 1 : 0) : AT[stage.kind];
  const live = stage.kind === "failed" ? "step step-fail" : done ? "step step-live" : "step step-live pulse";
  return STEPS.map((_, i) => (i < at ? "step step-done" : i === at ? live : "step"));
}

/** A started client's stage from its tracked instance (as the backend mirrors it), so rows keep
 * moving after the batch itself stopped. `allowDisconnect`: its account's setting. */
function stageOf(i: Instance, allowDisconnect: boolean): BulkStage {
  // As the backend's mirror_item: these notes fail the item whatever the state.
  if (i.signInProblem) return { kind: "failed", message: i.note ?? "" };
  switch (i.state) {
    case "starting":
    case "joining":
    case "inGame":
      return { kind: i.state };
    case "disconnected":
      return allowDisconnect ? { kind: "done" } : { kind: "failed", message: i.note ?? t("launch.batch.disconnected") };
    case "exited":
      return { kind: "failed", message: i.note ?? t("launch.batch.closed") };
  }
}

/** The running or last bulk launch, above Running clients. `onPick` picks accounts in Launch. */
export function BatchSection({ accounts, onPick }: { accounts: AccountSummary[]; onPick: (ids: number[]) => void }) {
  const qc = useQueryClient();
  const p = useBulk().data;
  const instances = useInstances().data;
  const [cancelling, setCancelling] = useState<string | null>(null);
  if (!p) return null;

  const byId = new Map(accounts.map((a) => [a.id, a]));
  // The account an item was started as (ids are reused once an account is removed).
  const own = (i: { accountId: number; userId: number }) => {
    const a = byId.get(i.accountId);
    return a && a.userId === i.userId ? a : undefined;
  };
  const live = new Map(instances?.map((i) => [i.id, i]));
  const closed: BulkStage = { kind: "failed", message: t("launch.batch.closed") };
  const items = p.items.map((item) => {
    const inst = item.instanceId ? live.get(item.instanceId) : undefined;
    const running = item.stage.kind === "starting" || item.stage.kind === "joining" || item.stage.kind === "inGame";
    // Only ended clients leave the tracker; while the batch runs the backend already says so,
    // and a progress event may arrive before the instance list catches up.
    const stage = inst ? stageOf(inst, own(item)?.allowDisconnect ?? false) : running && p.done && instances && item.instanceId ? closed : item.stage;
    return { ...item, stage, job: inst?.jobId ?? null };
  });
  const n = items.length;
  // Allow-disconnect accounts that disconnected are done, not failed.
  const ok = items.filter((i) => i.stage.kind === "inGame" || i.stage.kind === "done").length;
  const done = items.some((i) => i.stage.kind === "done");
  const failed = items.filter((i) => i.stage.kind === "failed").length;
  const rest = items
    .filter((i) => i.stage.kind === "cancelled" || (i.stage.kind === "failed" && !i.instanceId))
    .filter((i) => own(i)?.status === "valid")
    .map((i) => i.accountId);
  const target = describeTarget(p.target);
  const summary = [target.kind, p.sameServer === "pickFirst" ? t("launch.batch.sameServer") : p.sameServer === "followFirst" ? t("launch.batch.followingFirst") : null, t("launch.batch.seconds", { s: fmtNum(p.delayMs / 1000) })]
    .filter(Boolean)
    .join(" · ");
  const cancel = () => {
    setCancelling(p.batchId);
    api.bulkCancel(p.batchId).catch((e) => {
      setCancelling(null);
      toastError(e);
    });
  };

  return (
    <section aria-labelledby="batch-label" className="enter well flex-none flex flex-col px-4 py-3.5">
      <div className="flex flex-col gap-2.5 pb-3.5 border-b border-line">
        <div className="flex items-center gap-2.5 min-w-0">
          <span id="batch-label" className="font-medium">{t("launch.batch.label")}</span>
          <span className="mono text-xs text-dim truncate" title={target.detail}>{summary}</span>
          {p.done ? (
            <button className="wc w-7 h-7 ms-auto" aria-label={t("launch.batch.dismiss")} title={t("common.dismiss")} onClick={() => dismissBulk(qc, p.batchId)}>
              <Icon name="x" size={10} />
            </button>
          ) : (
            <button className="btn btn-sm ms-auto" disabled={cancelling === p.batchId} onClick={cancel}>
              {cancelling === p.batchId ? t("launch.batch.cancelling") : t("common.cancel")}
            </button>
          )}
        </div>
        <div className="flex items-baseline gap-2" role="status">
          <span className="mono text-[26px] font-medium">{ok}/{n}</span>
          <span className="text-dim">
            {[done ? t("launch.batch.inGameDone") : t("launch.batch.inGame"), failed ? t("launch.batch.failed", { count: failed }) : null, p.done ? t("launch.batch.finished") : null]
              .filter(Boolean)
              .join(" · ")}
          </span>
        </div>
        <span className="meter">
          <span className="transition-[width] duration-500" style={{ width: `${(ok / (n || 1)) * 100}%` }} />
        </span>
        {p.stopped && (
          <div className="flex flex-col items-start gap-2">
            <p role="alert" className="alert">{p.stopped}</p>
            {rest.length > 0 && (
              <button className="btn btn-sm" onClick={() => onPick(rest)}>
                {t("launch.batch.pickRest", { count: rest.length })}
              </button>
            )}
          </div>
        )}
        {p.note && <p className="text-xs text-dim">{p.note}</p>}
      </div>
      <div aria-hidden="true" className="mono grid grid-cols-[minmax(0,1fr)_132px] gap-x-4 pt-2 text-[9px] text-dim">
        <span />
        <span className="grid grid-cols-4 gap-[3px]">
          {STEPS.map((s) => <span key={s} className="truncate">{t(s)}</span>)}
        </span>
      </div>
      <ol className="scroll max-h-[40vh]">
        {items.map((item) => {
          const a = own(item);
          const st = item.stage;
          const job = item.job;
          const note = st.kind === "failed"
            ? t("launch.batch.failedNote", { message: st.message })
            : st.kind === "inGame" && job ? t("launch.batch.inGameJob", { job: job.slice(0, 8) }) : t(NOTE[st.kind]);
          return (
            <li
              key={item.accountId}
              className={`grid grid-cols-[minmax(0,1fr)_132px] items-center gap-x-4 gap-y-1 py-3 border-b border-line last:border-b-0 ${st.kind === "cancelled" ? "opacity-55" : ""}`}
            >
              <span className="flex items-center gap-2.5 min-w-0">
                {a && <Avatar account={a} size={20} />}
                <span className="truncate font-medium">{a ? accountName(a) : t("launch.accountN", { id: item.accountId })}</span>
              </span>
              <span aria-hidden="true" className="grid grid-cols-4 gap-[3px]">
                {steps(st, !!item.instanceId, p.done).map((c, i) => <span key={i} className={c} />)}
              </span>
              <span className={`mono col-span-2 ps-[30px] text-xs ${st.kind === "failed" ? "text-orange" : st.kind === "inGame" ? "text-text" : "text-dim"}`}>
                {note}
              </span>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
