import { useState } from "react";
import type { QueryClient } from "@tanstack/react-query";
import { api, type AccountSummary, type BulkProgress, type Crash, type Instance, type MultiInstanceStatus } from "../../api";
import { qk, useBulk, useCrashes, useInstances, useMultiStatus } from "../../data";
import { t } from "../../i18n";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import { Collapse, ListPresence } from "../../ui/motion";
import { toast, toastError } from "../../ui/Toast";
import { accountName, Avatar } from "../accounts/shared";
import { CRASH_KINDS, verdictOf } from "../crashes/kinds";
import { launchBlocked, launchEach, launchOne, launchStep, useLaunchActivity, useLaunchStep } from "../launch/launchQueue";
import { MultiNotice } from "../launch/MultiNotice";
import { isLive, isRunning, uptime, useNow } from "../launch/Running";
import { tabLaunch } from "./homeLayout";


/** What RoALT recorded about an ended client: how it ended, what its log said, its log. */
function CrashDetails({ crash: c }: { crash: Crash }) {
  const verdict = verdictOf(c);
  return (
    <div className="well flex flex-col gap-2 px-4 py-3 mt-1 mb-2.5">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <span>{t(CRASH_KINDS[c.kind])}</span>
        <span className="text-xs text-fg-3 tabular-nums">{c.placeId !== null ? t("launch.crash.afterPlace", { time: uptime(0, c.uptimeMs), place: c.placeId }) : t("launch.crash.after", { time: uptime(0, c.uptimeMs) })}</span>
      </div>
      {c.reason && <p className="mono text-xs text-fg-2 break-words selectable">{c.reason}</p>}
      {verdict && <p className="text-[13px] text-fg-2">{verdict}</p>}
      {c.hasLog && (
        <Button size="sm" iconEnd="external" className="self-start" onClick={() => api.crashOpenLog(c.id).catch(toastError)}>
          {t("launch.crash.openLog")}
        </Button>
      )}
    </div>
  );
}

/** What Play all again starts: each account once, at its latest ended client, when its sign-in is
 * valid and none of its clients runs. */
function againOf(list: Instance[], accounts: AccountSummary[]) {
  const valid = new Set(accounts.filter((a) => a.status === "valid").map((a) => a.id));
  const live = new Set(list.filter((i) => isRunning(i) && isLive(i)).map((i) => i.accountId));
  const ended = list.filter((i) => !isRunning(i)).sort((x, y) => (y.endedAt ?? 0) - (x.endedAt ?? 0));
  return ended.filter((i, n) => valid.has(i.accountId) && !live.has(i.accountId) && ended.findIndex((x) => x.accountId === i.accountId) === n);
}

/** The tray's Play all again: as the panel's button, from what the page last read; why it can't is
 * said in a toast (the tray has no place for it). */
export function playAllAgain(qc: QueryClient) {
  const list = qc.getQueryData<Instance[]>(qk.instances) ?? [];
  const accounts = qc.getQueryData<AccountSummary[]>(qk.accounts) ?? [];
  const again = againOf(list, accounts);
  if (!again.length) return;
  const several = again.length > 1 || list.some(isRunning);
  const why = launchStep() ?? launchBlocked(several, qc.getQueryData<MultiInstanceStatus>(qk.multi), qc.getQueryData<BulkProgress | null>(qk.bulk));
  if (why) return toast(why, "error");
  const byId = new Map(accounts.map((a) => [a.id, a]));
  void launchEach(again.map((i) => ({ account: byId.get(i.accountId)!, where: i.target, fpsCap: i.fpsCap })));
}

/** Clients that ended, newest first: how they ended, and a way to start them again, one or all. Why
 * starting is held up is said once, in the header. */
export function EndedPanel({ accounts }: { accounts: AccountSummary[] }) {
  const list = useInstances().data ?? [];
  const now = useNow(30_000);
  const [details, setDetails] = useState<string | null>(null);
  const crashes = new Map(useCrashes().data?.map((c) => [c.instanceId, c]));
  const multi = useMultiStatus().data;
  const batch = useBulk().data;
  const step = useLaunchStep();
  const activity = useLaunchActivity();
  const byId = new Map(accounts.map((a) => [a.id, a]));
  const running = list.filter(isRunning);
  const live = new Set(running.filter(isLive).map((i) => i.accountId));
  const ended = list.filter((i) => !isRunning(i)).sort((x, y) => (y.endedAt ?? 0) - (x.endedAt ?? 0));
  const can = (i: Instance) => byId.get(i.accountId)?.status === "valid" && !live.has(i.accountId);
  const again = againOf(list, accounts);
  // Starting next to running clients needs multi-instance, as a bulk launch does.
  const one = step ?? launchBlocked(running.length > 0, multi, batch);
  const all = one ?? launchBlocked(again.length > 1, multi, batch);
  const why = again.length > 1 ? all : again.length ? one : null;

  return (
    <Collapse open={ended.length > 0}>
      <section aria-labelledby="ended-title" className="panel flex flex-col px-4 pt-3 pb-1.5">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2 min-h-10">
          <h2 id="ended-title" className="section-title">{t("home.ended")}</h2>
          {why && <p id="ended-why" className="text-[13px] text-fg-2">{why}</p>}
          <div className="ms-auto flex flex-wrap gap-1.5">
            {again.length > 1 && (
              <Button
                size="md"
                icon="play"
                aria-disabled={!!all || undefined}
                aria-describedby={all ? "ended-why" : undefined}
                onClick={() => !all && void launchEach(again.map((i) => ({ account: byId.get(i.accountId)!, where: i.target, fpsCap: i.fpsCap })))}
              >
                {t("home.playAllAgain")}
              </Button>
            )}
            <Button
              size="md"
              variant="ghost"
              onClick={(e) => {
                // The whole panel folds away with this button: the focus waits on the page instead.
                const main = e.currentTarget.closest("main");
                if (main && e.currentTarget.contains(document.activeElement)) {
                  main.setAttribute("tabindex", "-1");
                  main.focus({ preventScroll: true });
                }
                api.instancesClearEnded().catch(toastError);
              }}
            >
              {t("launch.running.clearEnded")}
            </Button>
          </div>
        </div>
        {running.length > 0 && !multi?.active && <MultiNotice wanted className="my-2" />}
        <ListPresence
          as="ul"
          items={ended}
          getKey={(i) => i.id}
          className="flex flex-col"
          itemClassName="home-ended"
          render={(i) => {
            const a = byId.get(i.accountId);
            const name = a ? accountName(a) : t("launch.accountN", { id: i.accountId });
            const crash = crashes.get(i.id);
            const open = details === i.id;
            const starting = a ? tabLaunch(activity, a.id, live.has(a.id)) : undefined;
            const busy = starting === "queued" || starting === "launching";
            return (
              <>
                <div className="home-ended-row" data-tip={t("launch.running.pid", { pid: i.pid })}>
                  {a && <Avatar account={a} size={24} />}
                  <span className="font-medium min-w-0 truncate">{name}</span>
                  <span className="flex-1 min-w-[120px] text-[13px] text-fg-2 truncate">{i.note ?? t("launch.state.ended")}</span>
                  <span className="text-xs text-fg-3 tabular-nums">{uptime(i.startedAt, i.endedAt ?? now)}</span>
                  {crash && (
                    <Button size="sm" variant="ghost" aria-expanded={open} aria-label={t("launch.running.howEndedName", { name })} onClick={() => setDetails(open ? null : i.id)}>
                      {t("launch.running.howEnded")}
                      <Icon name="chevron" size={12} className={`home-chevron ${open ? "rotate-180" : ""}`} />
                    </Button>
                  )}
                  {a && can(i) && (
                    <Button
                      size="sm"
                      icon="play"
                      busy={busy}
                      aria-label={t("launch.running.relaunch", { name })}
                      data-tip={t("launch.running.relaunchHint")}
                      aria-disabled={(!!one && !busy) || undefined}
                      aria-describedby={one ? "ended-why" : undefined}
                      onClick={() => !one && void launchOne(a, i.target, i.fpsCap)}
                    >
                      {busy ? t("launch.launching") : t("home.playAgain")}
                    </Button>
                  )}
                </div>
                {crash && <Collapse open={open}><CrashDetails crash={crash} /></Collapse>}
              </>
            );
          }}
        />
      </section>
    </Collapse>
  );
}
