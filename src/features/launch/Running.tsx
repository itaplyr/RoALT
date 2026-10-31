import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api, type AccountSummary, type Crash, type CrashKind, type Instance, type InstanceState, type Limits, type MultiInstanceStatus } from "../../api";
import { qk, useBulk, useCrashes, useInstances, useMultiStatus, useSettings } from "../../data";
import { t, type Key } from "../../i18n";
import { NumberField, Segmented, Switch } from "../../ui/controls";
import { ConfirmDialog, Dialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { toast, toastError } from "../../ui/Toast";
import { accountName, Avatar } from "../accounts/shared";
import { launchBlocked, launched, launchFailed } from "./LaunchView";

// Getters: labels are translated when read (at render), not once at load.
export const STATES: Record<InstanceState, [label: string, color: string]> = {
  get starting(): [string, string] { return [t("launch.state.starting"), "var(--dim)"]; },
  get joining(): [string, string] { return [t("launch.state.joining"), "var(--blue)"]; },
  get inGame(): [string, string] { return [t("launch.state.inGame"), "var(--accent-text)"]; },
  get disconnected(): [string, string] { return [t("launch.state.disconnected"), "var(--orange)"]; },
  get exited(): [string, string] { return [t("launch.state.ended"), "var(--dim)"]; },
};

export const isRunning = (i: Instance) => i.state !== "exited";
/** Roblox has it running or on its way: its account can't start another. */
export const isLive = (i: Instance) => i.state === "starting" || i.state === "joining" || i.state === "inGame";

export function uptime(from: number, to: number) {
  const m = Math.max(0, Math.floor((to - from) / 60_000));
  return m < 60 ? t("launch.minutes", { m }) : t("launch.hoursMinutes", { h: Math.floor(m / 60), m: m % 60 });
}

/** Re-renders every `ms` so uptimes move. */
export function useNow(ms: number) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(t);
  }, [ms]);
  return now;
}

const CAPS = [0, 50, 25, 10, 5];
/** -1: the default from Settings; 0: never. */
const TRIMS = [-1, 0, 5, 15, 30, 60];
/** Logical processors a mask can name (JS numbers hold 53 bits exactly). */
const CORES = Math.min(53, navigator.hardwareConcurrency || 1);
// Not bit operators: those cut numbers to 32 bits.
const hasCore = (mask: number, c: number) => Math.floor(mask / 2 ** c) % 2 === 1;
const allCores = 2 ** CORES - 1;

/** One client's limits, from its menu on Home: CPU cap, efficiency mode, trimming, cores and anti-AFK. */
export function LimitsDialog({ instanceId, name, onClose }: { instanceId: string | null; name: string; onClose: () => void }) {
  const inst = useInstances().data?.find((i) => i.id === instanceId && isRunning(i));
  return (
    <Dialog open={!!instanceId && !!inst} onClose={onClose} title={t("launch.running.limitsFor", { name })} width={560}>
      {inst && <LimitsBody inst={inst} />}
    </Dialog>
  );
}

function LimitsBody({ inst }: { inst: Instance }) {
  const set = (patch: Partial<Limits>) => api.instanceSetLimits(inst.id, { ...inst.limits, ...patch }).catch(toastError);
  const afk = useSettings().data?.antiAfk;
  const [trimming, setTrimming] = useState(false);
  const s = useSettings().data;
  const [winW, setWinW] = useState(s?.microWidth ?? 320);
  const [winH, setWinH] = useState(s?.microHeight ?? 240);
  const resize = (on: boolean) =>
    api.instanceMicro(inst.id, on, [winW, winH]).then(([w, h]) => {
      if (on && (w > winW || h > winH)) toast(t("dashboard.microKept", { w, h }));
    }, toastError);
  const mask = inst.limits.affinity ?? allCores;
  const toggleCore = (c: number) => {
    const next = hasCore(mask, c) ? mask - 2 ** c : mask + 2 ** c;
    if (next === 0) return toastError({ kind: "invalid", message: t("launch.limits.oneCore") });
    void set({ affinity: next === allCores ? null : next });
  };
  const trimNow = () => {
    setTrimming(true);
    api
      .instanceTrim(inst.id)
      .then(() => toast(t("launch.limits.trimmed")), toastError)
      .finally(() => setTrimming(false));
  };
  return (
    <div className="flex flex-col gap-4 px-5 pb-5">
      {inst.throttled && <p role="status" className="alert">{t("launch.running.throttled")}</p>}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span>{t("launch.limits.cpu")}</span>
        <Segmented
          mono
          label={t("launch.limits.cpu")}
          value={inst.limits.cpuCap ?? 0}
          options={CAPS.map((c) => ({ value: c, label: c ? t("launch.limits.pct", { pct: c }) : t("common.off") }))}
          onChange={(c) => set({ cpuCap: c || null })}
        />
      </div>
      <div className="flex items-center justify-between gap-2">
        <span>{t("launch.limits.eco")}</span>
        <Switch label={t("launch.limits.eco")} checked={inst.limits.eco} onChange={(eco) => set({ eco })} />
      </div>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span>{t("launch.limits.trim")}</span>
        <span className="flex flex-wrap items-center gap-2">
          <Segmented
            mono
            label={t("launch.limits.trimEvery")}
            value={inst.limits.trimEveryMin ?? -1}
            options={TRIMS.map((m) => ({ value: m, label: m < 0 ? t("common.default") : m ? t("launch.minutes", { m }) : t("common.off") }))}
            onChange={(m) => set({ trimEveryMin: m < 0 ? null : m })}
          />
          <button className="btn btn-sm" disabled={trimming} onClick={trimNow}>
            {trimming ? t("launch.limits.trimming") : t("launch.limits.trimNow")}
          </button>
        </span>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span>{t("launch.limits.windowSize")}</span>
        <span className="flex flex-wrap items-center gap-2">
          <NumberField unit={t("performance.tinyWidth")} short="×" min={16} max={7680} value={winW} onCommit={setWinW} />
          <NumberField unit={t("performance.tinyHeight")} short="px" min={16} max={4320} value={winH} onCommit={setWinH} />
          <button className="btn btn-sm" onClick={() => void resize(true)}>{t("performance.apply")}</button>
          {inst.micro && <button className="btn btn-sm" onClick={() => void resize(false)}>{t("dashboard.normalSize")}</button>}
        </span>
      </div>
      <fieldset className="flex flex-col gap-1.5">
        <legend className="flex w-full items-center justify-between gap-2">
          {t("launch.limits.cores")}
          {inst.limits.affinity !== null && (
            <button type="button" className="btn btn-sm" onClick={() => set({ affinity: null })}>{t("launch.limits.allCores")}</button>
          )}
        </legend>
        <div className="flex flex-wrap gap-2 mt-1.5">
          {Array.from({ length: CORES }, (_, c) => (
            <label key={c} className="mono flex items-center gap-1.5 text-xs text-dim w-14">
              <input type="checkbox" className="cb" checked={hasCore(mask, c)} onChange={() => toggleCore(c)} />
              {c}
            </label>
          ))}
        </div>
      </fieldset>
      <div className="flex items-center justify-between gap-2">
        <span className="flex flex-col">
          <span>{t("launch.limits.antiAfk")}</span>
          <span className="mono text-xs text-dim">
            {!afk?.enabled ? t("launch.limits.afkOff") : inst.afkAt !== null ? t("launch.limits.lastKey", { ago: uptime(inst.afkAt, Date.now()) }) : t("launch.limits.noKey")}
          </span>
        </span>
        <Switch label={t("launch.limits.antiAfkClient")} checked={inst.antiAfk} onChange={(on) => api.instanceSetAntiAfk(inst.id, on).catch(toastError)} />
      </div>
      <p className="text-xs text-dim">{t("launch.limits.help")}</p>
    </div>
  );
}

const KINDS: Record<CrashKind, Key> = {
  crash: "launch.crash.crash",
  disconnect: "launch.crash.disconnect",
  hang: "launch.crash.hang",
  mem: "launch.crash.mem",
};

/** What RoALT recorded about an ended client: how it ended, what its log said, its log. */
function CrashDetails({ crash: c }: { crash: Crash }) {
  return (
    <div className="well flex flex-col gap-2 px-4 py-3">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <span>{t(KINDS[c.kind])}</span>
        <span className="mono text-xs text-dim">{c.placeId !== null ? t("launch.crash.afterPlace", { time: uptime(0, c.uptimeMs), place: c.placeId }) : t("launch.crash.after", { time: uptime(0, c.uptimeMs) })}</span>
      </div>
      {c.reason && <p className="mono text-xs text-dim break-words selectable">{c.reason}</p>}
      {c.hasLog && (
        <button className="btn btn-sm self-start" onClick={() => api.crashOpenLog(c.id).catch(toastError)}>
          {t("launch.crash.openLog")}
          <Icon name="external" size={12} />
        </button>
      )}
    </div>
  );
}

/** Clients that ended, newest first, on Home: how they ended, and a way to start them again. */
export function EndedPanel({ accounts }: { accounts: AccountSummary[] }) {
  const list = useInstances().data ?? [];
  const now = useNow(30_000);
  const [details, setDetails] = useState<string | null>(null);
  const crashes = new Map(useCrashes().data?.map((c) => [c.instanceId, c]));
  const [relaunching, setRelaunching] = useState<string | null>(null);
  const multi = useMultiStatus().data;
  const batch = useBulk().data;
  const byId = new Map(accounts.map((a) => [a.id, a]));
  const running = list.filter(isRunning);
  const ended = list.filter((i) => !isRunning(i));

  /** Same account, same target. */
  const relaunch = (i: Instance, a: AccountSummary) => {
    setRelaunching(i.id);
    api
      .launch(a.id, i.target, undefined, i.fpsCap)
      .then((info) => launched(a, info), launchFailed)
      .finally(() => setRelaunching(null));
  };
  // Starting next to running clients needs multi-instance, as a bulk launch does.
  const blocked = launchBlocked(running.length > 0, multi, batch) ?? (relaunching ? t("launch.launching") : null);

  if (!ended.length) return null;
  return (
    <section aria-labelledby="ended-title" className="panel flex flex-col gap-2 p-5">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 id="ended-title" className="section-title">{t("home.ended")}</h2>
        <button className="btn btn-sm" onClick={() => api.instancesClearEnded().catch(toastError)}>
          {t("launch.running.clearEnded")}
        </button>
      </div>
      {running.length > 0 && !multi?.active && <MultiNotice wanted />}
      <ul className="flex flex-col">
        {ended.map((i) => {
          const a = byId.get(i.accountId);
          const name = a ? accountName(a) : t("launch.accountN", { id: i.accountId });
          const crash = crashes.get(i.id);
          return (
            <li key={i.id} className="flex flex-col gap-2 py-2.5 border-t border-line first:border-t-0">
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1 min-h-11" title={t("launch.running.pid", { pid: i.pid })}>
                {a && <Avatar account={a} size={28} />}
                <span className="font-medium min-w-0 truncate">{name}</span>
                <span className="flex-1 min-w-[120px] text-dim truncate">{i.note ?? t("launch.state.ended")}</span>
                <span className="mono text-xs text-dim">{uptime(i.startedAt, i.endedAt ?? now)}</span>
                {crash && (
                  <button className="btn btn-sm btn-ghost" aria-expanded={details === i.id} aria-label={t("launch.running.howEndedName", { name })} onClick={() => setDetails((d) => (d === i.id ? null : i.id))}>
                    {t("launch.running.howEnded")}
                    <Icon name="chevron" size={12} className={details === i.id ? "rotate-180" : ""} />
                  </button>
                )}
                {a?.status === "valid" && !running.some((r) => r.accountId === a.id && isLive(r)) && (
                  <button className="btn btn-sm" aria-label={t("launch.running.relaunch", { name })} title={blocked ?? t("launch.running.relaunchHint")} disabled={blocked !== null} onClick={() => relaunch(i, a)}>
                    <Icon name="play" size={12} fill />
                    {t("home.playAgain")}
                  </button>
                )}
              </div>
              {crash && details === i.id && <CrashDetails crash={crash} />}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

const HOLD_MS = 1000;

/** The panic button: held for a second (pointer, Space or Enter), it cancels the batch, stops
 * rejoins and restarts, and terminates every Roblox client at once, RoALT's or not. */
export function KillAllButton() {
  const qc = useQueryClient();
  const [held, setHeld] = useState(false);
  const [busy, setBusy] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const button = useRef<HTMLButtonElement>(null);
  useEffect(() => () => clearTimeout(timer.current), []);
  const fire = () => {
    if (!matchMedia("(prefers-reduced-motion: reduce)").matches) button.current?.animate([{ filter: "brightness(1.9)" }, { filter: "none" }], { duration: 350, easing: "ease-out" });
    setHeld(false);
    setBusy(true);
    api
      .panicKill()
      .then((st) => {
        qc.setQueryData(qk.multi, st);
        toast(t("launch.kill.done"));
      }, toastError)
      .finally(() => setBusy(false));
  };
  const start = () => {
    if (busy || held) return;
    setHeld(true);
    timer.current = window.setTimeout(fire, HOLD_MS);
  };
  const stop = () => {
    clearTimeout(timer.current);
    setHeld(false);
  };
  const key = (e: { key: string }) => e.key === " " || e.key === "Enter";
  return (
    <button
      ref={button}
      type="button"
      className="btn btn-danger hold-btn relative overflow-hidden w-full whitespace-normal leading-tight py-1.5"
      style={{ "--hold": `${HOLD_MS}ms` } as React.CSSProperties}
      data-held={held || undefined}
      data-busy={busy || undefined}
      disabled={busy}
      title={t("launch.kill.hint")}
      // Screen readers and voice control activate without pressing anything to hold.
      onClick={(e) => e.detail === 0 && !busy && !held && fire()}
      onPointerDown={(e) => e.button === 0 && start()}
      onPointerUp={stop}
      onPointerLeave={stop}
      onKeyDown={(e) => {
        if (!key(e)) return;
        e.preventDefault();
        if (!e.repeat) start();
      }}
      onKeyUp={(e) => key(e) && stop()}
      onBlur={stop}
    >
      <span aria-hidden="true" className="hold-fill" />
      <span className="hold-label relative min-w-0">{busy ? t("launch.kill.killing") : held ? t("launch.kill.holding") : t("launch.kill.hold")}</span>
    </button>
  );
}

const NOTICE_KEY = "roalt.multiInstanceNotice";

function noticeSeen() {
  try {
    return localStorage.getItem(NOTICE_KEY) === "1";
  } catch {
    return false;
  }
}

/** Turns multi-instance on or off. The first time it is turned on, Roblox's stance on several
 * clients is shown first (DESIGN §2); render `dialog` next to the switch. */
export function useMultiToggle(onStatus?: (s: MultiInstanceStatus) => void) {
  const qc = useQueryClient();
  const [asking, setAsking] = useState(false);
  const set = (enabled: boolean) =>
    api.multiInstanceSet(enabled).then((s) => {
      qc.setQueryData(qk.multi, s);
      qc.invalidateQueries({ queryKey: qk.settings });
      onStatus?.(s);
    }, toastError);
  const toggle = (enabled: boolean) => (enabled && !noticeSeen() ? setAsking(true) : void set(enabled));
  const dialog = (
    <ConfirmDialog
      open={asking}
      onClose={() => setAsking(false)}
      onConfirm={() => {
        try {
          localStorage.setItem(NOTICE_KEY, "1");
        } catch {
          // Shown again next time; harmless.
        }
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
        <div className={`enter well flex flex-wrap items-center gap-x-5 gap-y-3 px-4 py-3.5 ${className}`}>
          {off ? (
            <>
              <div className="flex-[1_1_320px] min-w-0">
                <p className="font-medium">{wanted ? t("launch.multi.needTitle") : t("launch.multi.replaceTitle")}</p>
                <p className="text-dim text-[13px]">{wanted ? t("launch.multi.needText") : t("launch.multi.replaceText")}</p>
              </div>
              <button className="btn" onClick={() => toggle(true)}>{t("launch.multi.turnOn")}</button>
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
