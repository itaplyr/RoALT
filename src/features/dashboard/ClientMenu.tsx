import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, type AccountSummary, type Instance, type Limits, type WindowShow } from "../../api";
import { qk, queryClient, useInstances, useMacroRuns, useMacros, useSettings } from "../../data";
import { isRtl, t } from "../../i18n";
import { Icon } from "../../ui/Icon";
import { menuKeys } from "../../ui/menu";
import { MenuCheckItem, MenuItem, MenuLabel, MenuSeparator } from "../../ui/Popover";
import { toast, toastError } from "../../ui/Toast";
import { launchOne, useLaunchStep } from "../launch/launchQueue";
import { mb } from "../monitor/MonitorView";

const CAPS = [0, 50, 25, 10, 5];

/** `beside`: the left edge of the key that opened it, `x` its right edge. The menu opens on the
 * side the card's text starts from (left of the key, or right of it right to left), not over it,
 * where a second click would land on an item. */
export type MenuAt = { inst: Instance; hidden: boolean; x: number; y: number; from: HTMLElement; beside?: number };

/** Keyed by pid too: a close that reached only a bootstrapper ends with the client adopted under a
 * new pid, which can then be closed again. */
export const closeKey = (i: Instance) => `${i.id}:${i.pid}`;

/** A card's menu: a popover, so Esc and a click outside close it; arrows move. Placed by hand, right
 * after it shows, so it only fades and slides in (a scale would throw off that measure). */
export function ClientMenu({ at, account, onClose, afkOn, closing, onLimits, onCloseClient }: {
  at: MenuAt | null;
  account: AccountSummary | undefined;
  onClose: () => void;
  afkOn: boolean;
  closing: ReadonlySet<string>;
  onLimits: (id: string) => void;
  onCloseClient: (i: Instance, force: boolean) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: 0, top: 0 });
  // Showing the macros to run instead of the client's actions; `swapped`: since it opened.
  const [macroList, setMacroList] = useState(false);
  const [swapped, setSwapped] = useState(false);
  const macros = (useMacros().data ?? []).filter((m) => m.trusted);
  const macroRun = (useMacroRuns().data ?? []).find((r) => r.instanceId === at?.inst.id);
  const settings = useSettings().data;
  // While another launch starts, Play again says what that one is doing.
  const step = useLaunchStep();
  // The checks stay open while they change: they read the client as it is now.
  const i = useInstances().data?.find((x) => x.id === at?.inst.id && x.state !== "exited");

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !at) return;
    el.showPopover();
    const r = el.getBoundingClientRect();
    setPos({
      left: at.beside === undefined ? Math.max(4, Math.min(at.x, innerWidth - r.width - 4)) : isRtl() ? Math.min(at.x + 6, innerWidth - r.width - 4) : Math.max(4, at.beside - r.width - 6),
      top: Math.max(4, Math.min(at.y, innerHeight - r.height - 4)),
    });
    el.querySelector<HTMLElement>("[role^=menuitem]:not(:disabled)")?.focus();
  }, [at]);
  // Switched lists while open: the new one gets the focus, and room below as on opening.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el?.matches(":popover-open")) return;
    const h = el.getBoundingClientRect().height;
    setPos((p) => ({ ...p, top: Math.max(4, Math.min(p.top, innerHeight - h - 4)) }));
    el.querySelector<HTMLElement>("[role^=menuitem]:not(:disabled)")?.focus();
  }, [macroList]);

  useEffect(() => {
    const el = ref.current;
    const toggled = (e: Event) => {
      if ((e as ToggleEvent).newState !== "closed") return;
      // Focus goes back to the card unless the click went somewhere else.
      if (!document.activeElement || document.activeElement === document.body || el?.contains(document.activeElement)) at?.from.focus();
      setMacroList(false);
      setSwapped(false);
      onClose();
    };
    el?.addEventListener("toggle", toggled);
    return () => el?.removeEventListener("toggle", toggled);
  }, [onClose, at]);

  // Its client ended while the menu was open: nothing left to act on.
  const gone = !!at && !i;
  useEffect(() => {
    if (gone && ref.current?.matches(":popover-open")) ref.current.hidePopover();
  }, [gone]);

  const tiny = { w: settings?.microWidth ?? 320, h: settings?.microHeight ?? 240 };
  const list = (on: boolean) => {
    setMacroList(on);
    setSwapped(true);
  };
  const show = (what: WindowShow) => i && api.instanceShow(i.id, what).catch(toastError);
  const limits = (patch: Partial<Limits>) => i && api.instanceSetLimits(i.id, { ...i.limits, ...patch }).catch(toastError);
  const micro = () =>
    i &&
    api.instanceMicro(i.id, !i.micro).then(([w, h]) => {
      if (!i.micro && (w > tiny.w || h > tiny.h)) toast(t("dashboard.microKept", { w, h }));
    }, toastError);
  const trim = () => i && api.instanceTrim(i.id).then(([before, after]) => toast(t("dashboard.trimmedFrom", { before: mb(before), after: mb(after) })), toastError);
  const runMacro = (id: number) => i && api.macroRun(id, [i.id]).then((runs) => queryClient.setQueryData(qk.macroRuns, runs), toastError);

  return (
    <div
      ref={ref}
      popover="auto"
      role="menu"
      aria-label={t("dashboard.clientActions")}
      className="menu home-menu"
      dir={isRtl() ? "rtl" : "ltr"}
      onKeyDown={menuKeys}
      style={{ position: "fixed", positionArea: "none", inset: "auto", left: pos.left, top: pos.top, margin: 0, maxHeight: "calc(100vh - 8px)" }}
    >
      {i && at && (
        <div key={macroList ? "macros" : "actions"} className={swapped ? "home-menu-swap" : undefined}>
          {macroList ? (
            <>
              <MenuItem icon="back" closes={false} onSelect={() => list(false)}>{t("dashboard.back")}</MenuItem>
              <MenuSeparator />
              {macros.map((m) => (
                <MenuItem key={m.id} icon="key" onSelect={() => void runMacro(m.id)}>{m.name}</MenuItem>
              ))}
            </>
          ) : (
            <>
              <MenuLabel>{t("launch.running.pid", { pid: i.pid })}</MenuLabel>
              {i.state === "disconnected" && !i.relaunching && account && (
                <MenuItem icon="play" disabled={step !== null} onSelect={() => void launchOne(account, i.target, i.fpsCap)}>{step ?? t("home.playAgain")}</MenuItem>
              )}
              <MenuItem icon="eye" onSelect={() => void show("focus")}>{t("home.show")}</MenuItem>
              {at.hidden ? (
                <MenuItem icon="external" onSelect={() => void show("restore")}>{t("dashboard.restore")}</MenuItem>
              ) : (
                <MenuItem icon="minimize" onSelect={() => void show("minimize")}>{t("dashboard.minimize")}</MenuItem>
              )}
              <MenuItem icon="grid" onSelect={() => void micro()}>{i.micro ? t("dashboard.normalSize") : t("dashboard.microWindow", tiny)}</MenuItem>
              <MenuSeparator />
              <MenuItem icon="refresh" onSelect={() => void trim()}>{t("dashboard.trimMemory")}</MenuItem>
              <div role="group" aria-label={t("dashboard.cpuLimit")} className="home-menu-caps">
                <span className="flex-1 min-w-0 truncate">{t("dashboard.cpuLimit")}</span>
                {CAPS.map((c) => (
                  <button
                    key={c}
                    type="button"
                    role="menuitemradio"
                    aria-checked={(i.limits.cpuCap ?? 0) === c}
                    aria-label={c ? t("dashboard.cpuLimitPct", { pct: c }) : t("dashboard.noCpuLimit")}
                    className="seg"
                    onClick={() => (i.limits.cpuCap ?? 0) !== c && void limits({ cpuCap: c || null })}
                  >
                    {c || t("common.off")}
                  </button>
                ))}
              </div>
              <MenuCheckItem checked={i.limits.eco} onChange={(eco) => void limits({ eco })}>{t("dashboard.efficiencyMode")}</MenuCheckItem>
              <MenuCheckItem checked={i.antiAfk} onChange={(on) => api.instanceSetAntiAfk(i.id, on).catch(toastError)}>
                {afkOn ? t("dashboard.antiAfk") : t("dashboard.antiAfkOff")}
              </MenuCheckItem>
              <MenuItem icon="sliders" onSelect={() => onLimits(i.id)}>{t("home.limits")}</MenuItem>
              {macroRun ? (
                <MenuItem icon="stop" onSelect={() => api.macroStop(i.id).catch(toastError)}>{t("dashboard.stopMacro")}</MenuItem>
              ) : (
                <button type="button" role="menuitem" aria-haspopup="menu" className="menu-item" disabled={i.state !== "inGame" || !macros.length} onClick={() => list(true)}>
                  <Icon name="key" />
                  <span className="flex-1 min-w-0 truncate">{t("dashboard.runMacro")}</span>
                  <Icon name="chevron" size={14} className={isRtl() ? "rotate-90" : "-rotate-90"} />
                </button>
              )}
              <MenuSeparator />
              <MenuItem icon="x" disabled={closing.has(closeKey(i))} onSelect={() => onCloseClient(i, false)}>{t("common.close")}</MenuItem>
              <MenuItem icon="x" danger onSelect={() => onCloseClient(i, true)}>{t("dashboard.forceClose")}</MenuItem>
            </>
          )}
        </div>
      )}
    </div>
  );
}
