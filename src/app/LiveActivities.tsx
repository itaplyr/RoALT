import { useEffect, useRef, useState } from "react";
import type { AccountSummary, BulkStage, Settings } from "../api";
import { useInstances } from "../data";
import { TONE_COLOR, TONE_SHAPE } from "../ui/Status";
import { t, type Key } from "../i18n";
import { accountName, Avatar } from "../features/accounts/shared";
import { useWindowTools } from "../features/dashboard/windowTools";
import { useLaunchActivity } from "../features/launch/launchQueue";
import { LaunchProgress } from "../features/launch/LaunchProgress";
import { isRunning, STATES } from "../features/launch/Running";
import { jumpTo } from "../features/settings/jumpTo";
import { usePublicBlock } from "../features/settings/PublicNetworkBanner";
import { UpdateText, useUpdateInstall, useUpdateOffer } from "../features/settings/UpdateBanner";
import { announce } from "../ui/a11y";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";
import { Capsule, Ring } from "../ui/island/Capsule";
import { LAYOUT_IDS, Swap } from "../ui/motion";
import { KillAllButton } from "./KillAll";
import { requestNavigation } from "./navigation";

/** What a batch item is doing now, in a few words. */
const NOTE: Partial<Record<BulkStage["kind"], Key>> = {
  queued: "launch.batch.queued",
  ticket: "launch.batch.ticket",
  starting: "launch.batch.starting",
  joining: "launch.batch.joining",
};

/** The island's live activities: a launch under way, the running clients, a notice. */
export function LiveActivities({ accounts, settings, onPick }: { accounts: AccountSummary[]; settings: Settings; onPick: (ids: number[]) => void }) {
  return (
    <>
      <LaunchCapsule onPick={onPick} />
      <RunningCapsule accounts={accounts} />
      <NoticeCapsule settings={settings} />
    </>
  );
}

/** The launch on show, however it started (Play, Servers, Home's tabs, the tray): a ring and
 * "3/8 · Alt 02 joining"; it lingers 6 s once finished. As it appears, its button takes over
 * LaunchButton's pill in flight (the same layoutId), so the pill lands in the island. */
function LaunchCapsule({ onPick }: { onPick: (ids: number[]) => void }) {
  const a = useLaunchActivity();
  const [linger, setLinger] = useState(false);
  const told = useRef<string | null>(null);
  const n = a?.rows.length ?? 0;
  const busy = a?.current;

  useEffect(() => {
    if (!a) return;
    if (told.current !== a.id && !a.done) {
      told.current = a.id;
      announce(`${a.title}: ${a.ok}/${n}`);
    }
    if (!a.done) return;
    if (told.current === a.id) {
      told.current = `${a.id}:done`;
      announce([`${a.title}: ${a.ok}/${n}`, a.failed ? t("launch.batch.failed", { count: a.failed }) : null, t("launch.batch.finished")].filter(Boolean).join(" · "));
    }
    setLinger(true);
    const timer = setTimeout(() => setLinger(false), 6000);
    return () => clearTimeout(timer);
  }, [a?.id, a?.done]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <Capsule
      show={!!a && (!a.done || linger)}
      label={a ? `${a.title}: ${a.ok}/${n}` : ""}
      layoutId={LAYOUT_IDS.launchLive}
      sheetLabel={a?.title ?? ""}
      sheet={(close) => (
        <div className="p-2">
          <LaunchProgress variant="island" onPick={(ids) => (close(), onPick(ids))} />
        </div>
      )}
    >
      {a?.done ? <Icon name={a.failed ? "alert" : "check"} size={15} /> : <Ring value={n ? (a?.ok ?? 0) / n : 0} />}
      <span>{a?.ok ?? 0}/{n}</span>
      {a && !a.done && busy && NOTE[busy.stage.kind] && (
        <span className="capsule-detail">
          {/* Each step's words cross-fade while the capsule (a Morph) springs to their width. */}
          <Swap k={`${busy.accountId}:${busy.stage.kind}`} className="max-w-full">
            <span className="min-w-0 truncate">
              {busy.account ? accountName(busy.account) : t("launch.accountN", { id: busy.accountId })} · {t(NOTE[busy.stage.kind]!)}
            </span>
          </Swap>
        </span>
      )}
    </Capsule>
  );
}

/** How many clients run, as a light and a number; its sheet holds the window tools and the panic key. */
function RunningCapsule({ accounts }: { accounts: AccountSummary[] }) {
  const running = (useInstances().data ?? []).filter(isRunning);
  const tools = useWindowTools();
  // The state that needs the eye most, as a shape and (in its name) a word, not a hue alone.
  const lost = running.some((i) => i.state === "disconnected");
  const coming = running.some((i) => i.state === "starting" || i.state === "joining");
  const [tone, state] = lost ? (["attention", "disconnected"] as const) : coming ? (["info", "joining"] as const) : (["ok", null] as const);
  const label = [t("app.running", { count: running.length }), state && STATES[state][0]].filter(Boolean).join(" · ");
  const byId = new Map(accounts.map((a) => [a.id, a]));
  return (
    <Capsule
      show={running.length > 0}
      label={label}
      sheetLabel={t("app.running", { count: running.length })}
      sheet={(close) => (
        <div className="flex flex-col">
          <ul className="flex flex-col p-2 max-h-72 overflow-auto">
            {running.slice(0, 8).map((i) => {
              const a = byId.get(i.accountId);
              const [label, color] = STATES[i.state];
              return (
                <li key={i.id} className="flex items-center gap-2.5 px-2 min-h-10 rounded-[10px]">
                  {a && <Avatar account={a} size={22} />}
                  <span className="flex-1 min-w-0 truncate font-medium">{a ? accountName(a) : t("dashboard.account", { id: i.accountId })}</span>
                  <span className="inline-flex items-center gap-1.5 text-[12px] text-fg-2">
                    <span aria-hidden="true" className="led" style={{ color }} />
                    {label}
                  </span>
                </li>
              );
            })}
            {running.length > 8 && <li className="px-2 py-1.5 text-[12px] text-fg-3">+{running.length - 8}</li>}
          </ul>
          <div className="flex flex-wrap gap-2 px-3 pb-3">
            <Button size="md" icon="home" onClick={() => (close(), requestNavigation("dashboard"))}>{t("app.nav.dashboard")}</Button>
            <Button size="md" icon="minimize" disabled={tools.busy} onClick={() => void tools.minimizeAll()}>{t("dashboard.minimizeAll")}</Button>
            <Button size="md" icon="grid" disabled={tools.busy} onClick={() => void tools.tile(false)}>{t("dashboard.tile")}</Button>
          </div>
          <div className="px-3 pb-3">
            {/* Once it fires the sheet closes, so the focus goes back to the capsule, not nowhere. */}
            <KillAllButton onFired={close} />
          </div>
        </div>
      )}
    >
      <span aria-hidden="true" className={`status-mark ${coming ? "pulse" : ""}`} data-shape={TONE_SHAPE[tone]} style={{ color: TONE_COLOR[tone] }} />
      <span>{running.length}</span>
    </Capsule>
  );
}

/** An update on offer, or the public-network block: one slot, the update first. */
function NoticeCapsule({ settings }: { settings: Settings }) {
  const version = useUpdateOffer();
  const install = useUpdateInstall(version);
  const blocked = usePublicBlock(settings);
  // Update now asks first while clients run (a modal, which needs the sheet closed); otherwise the
  // sheet stays and shows the download.
  const asks = (useInstances().data ?? []).some(isRunning);
  const kind = version ? "update" : blocked ? "network" : null;
  // Named for the state it shows; the verbs are the sheet's buttons.
  const name = kind === "update" ? (install.busy ? t("settings.updateDownloading") : t("settings.updateAvailable", { version: version ?? "" })) : t("settings.publicNow");
  return (
    <>
      <Capsule
        show={kind !== null}
        label={name}
        sheetLabel={name}
        sheet={(close) =>
          kind === "update" && version ? (
            <div className="flex flex-col gap-3 p-4">
              <UpdateText version={version} busy={install.busy} />
              <div className="flex gap-2">
                <Button variant="primary" size="md" icon="download" busy={install.busy} aria-disabled={install.busy || undefined} onClick={() => install.busy || (asks && close(), install.start())}>{t("settings.updateNow")}</Button>
                <Button size="md" disabled={install.busy} onClick={() => (close(), install.later())}>{t("settings.updateLater")}</Button>
              </div>
            </div>
          ) : (
            <div className="flex flex-col gap-3 p-4">
              <p>{t("settings.publicBannerBlocked")}</p>
              <div>
                <Button size="md" icon="shield" onClick={() => (close(), jumpTo("publicNetworks"))}>{t("app.nav.settings")}</Button>
              </div>
            </div>
          )
        }
      >
        {kind === "update" && install.busy ? <Icon name="refresh" size={15} className="spin" /> : <Icon name={kind === "update" ? "download" : "globe"} size={15} />}
      </Capsule>
      {install.dialog}
    </>
  );
}
