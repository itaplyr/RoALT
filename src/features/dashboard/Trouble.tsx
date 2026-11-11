import { useRef } from "react";
import { api, type AccountSummary, type Instance } from "../../api";
import { t } from "../../i18n";
import { Button } from "../../ui/Button";
import { Collapse, ListPresence } from "../../ui/motion";
import { toastError } from "../../ui/Toast";
import { clientName } from "./ClientCard";

/** Clients whose relaunch has to wait, and the ways out. "Try now" retries all of them (the backend
 * has no retry for one), so two or more come as one notice under it; each can still be stopped. */
export function Trouble({ list, byId }: { list: Instance[]; byId: Map<number, AccountSummary> }) {
  const now = list.filter((i) => i.relaunching && i.relaunchProblem);
  // The last ones stay on screen while the notice folds away.
  const last = useRef(now);
  if (now.length) last.current = now;
  const waiting = now.length ? now : last.current;
  const one = waiting.length === 1 ? waiting[0] : null;
  const nameOf = (i: Instance) => clientName(i, byId.get(i.accountId));
  const stop = (i: Instance) => api.instanceStopRelaunch(i.id).catch(toastError);
  return (
    <Collapse open={now.length > 0}>
      <section role="status" className="notice home-trouble" data-tone="attention">
        <span aria-hidden="true" className="status-mark pulse text-attention" data-shape="diamond" />
        <div className="flex-[1_1_320px] min-w-0">
          <p className="font-medium">{one ? t("home.lost", { name: nameOf(one) }) : t("home.lostMany", { count: waiting.length })}</p>
          {one && <p className="text-[13px] text-fg-2">{t("home.retryLine", { why: one.relaunchProblem ?? "" })}</p>}
        </div>
        <div className="flex gap-1.5">
          <Button size="md" icon="refresh" onClick={() => api.instancesRetryRelaunch().catch(toastError)}>{t("home.tryNow")}</Button>
          {one && <Button size="md" variant="ghost" onClick={() => void stop(one)}>{t("home.stopTrying")}</Button>}
        </div>
        {!one && (
          <ListPresence
            as="ul"
            items={waiting}
            getKey={(i) => i.id}
            className="home-trouble-list"
            render={(i) => (
              <div className="home-trouble-row">
                <span className="font-medium truncate">{nameOf(i)}</span>
                <span className="flex-1 min-w-0 truncate text-[13px] text-fg-2" data-tip={i.relaunchProblem ?? undefined}>{i.relaunchProblem}</span>
                <Button size="sm" variant="ghost" onClick={() => void stop(i)}>{t("home.stopTrying")}</Button>
              </div>
            )}
          />
        )}
      </section>
    </Collapse>
  );
}
