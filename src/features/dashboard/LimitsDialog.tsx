import { useState } from "react";
import "./dashboard.css";
import { api, type Instance, type Limits } from "../../api";
import { useInstances, useSettings } from "../../data";
import { t } from "../../i18n";
import { Disclosure } from "../../ui/Bits";
import { NumberField, Segmented, Switch } from "../../ui/controls";
import { Dialog } from "../../ui/Dialog";
import { toast, toastError } from "../../ui/Toast";
import { isRunning, uptime } from "../launch/Running";
import { mb } from "../monitor/format";

const CAPS = [0, 50, 25, 10, 5];
/** -1: the default from Settings; 0: never. */
const TRIMS = [-1, 0, 5, 15, 30, 60];
/** Logical processors a mask can name (JS numbers hold 53 bits exactly). */
const CORES = Math.min(53, navigator.hardwareConcurrency || 1);
// Not bit operators: those cut numbers to 32 bits.
const hasCore = (mask: number, c: number) => Math.floor(mask / 2 ** c) % 2 === 1;
const allCores = 2 ** CORES - 1;

/** One client's limits, from its menu on Home (and Monitor): CPU cap, efficiency mode, trimming,
 * window size, cores and anti-AFK. */
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
      .then(([before, after]) => toast(t("dashboard.trimmedFrom", { before: mb(before), after: mb(after) })), toastError)
      .finally(() => setTrimming(false));
  };
  return (
    <div className="flex flex-col gap-4 px-[22px] pb-5">
      {inst.throttled && <p role="status" className="alert">{t("launch.running.throttled")}</p>}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span>{t("launch.limits.cpu")}</span>
        <Segmented
          mono
          label={t("launch.limits.cpu")}
          value={inst.limits.cpuCap ?? 0}
          options={CAPS.map((c) => ({ value: c, label: c ? t("settings.percent", { n: c }) : t("common.off") }))}
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
            options={TRIMS.map((m) => ({ value: m, label: m < 0 ? t("common.default") : m ? t("settings.minutesShort", { n: m }) : t("common.off") }))}
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
            <label key={c} className="flex items-center gap-1.5 w-14 text-xs text-fg-2 tabular-nums">
              <input type="checkbox" className="cb" checked={hasCore(mask, c)} onChange={() => toggleCore(c)} />
              {c}
            </label>
          ))}
        </div>
      </fieldset>
      <div className="flex items-center justify-between gap-2">
        <span className="flex flex-col">
          <span>{t("launch.limits.antiAfk")}</span>
          <span className="text-xs text-fg-3">
            {!afk?.enabled ? t("launch.limits.afkOff") : inst.afkAt !== null ? t("launch.limits.lastKey", { ago: uptime(inst.afkAt, Date.now()) }) : t("launch.limits.noKey")}
          </span>
        </span>
        <Switch label={t("launch.limits.antiAfkClient")} checked={inst.antiAfk} onChange={(on) => api.instanceSetAntiAfk(inst.id, on).catch(toastError)} />
      </div>
      <Disclosure summary={t("settings.moreAbout", { label: t("monitor.limits") })} className="home-limits-help">
        {t("launch.limits.help")}
      </Disclosure>
    </div>
  );
}
