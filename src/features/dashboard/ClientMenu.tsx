import { useEffect, useRef } from "react";
import { api, type AccountSummary, type Instance, type Limits, type WindowShow } from "../../api";
import { useInstances, useSettings } from "../../data";
import { t } from "../../i18n";
import { ContextMenu, MenuCheckItem, MenuItem, MenuLabel, MenuSeparator } from "../../ui/Popover";
import { toast, toastError } from "../../ui/Toast";
import { launchOne, useLaunchStep } from "../launch/launchQueue";
import { mb } from "../monitor/format";

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
  const settings = useSettings().data;
  // While another launch starts, Play again says what that one is doing.
  const step = useLaunchStep();
  // The checks stay open while they change: they read the client as it is now.
  const i = useInstances().data?.find((x) => x.id === at?.inst.id && x.state !== "exited");

  // Its client ended while the menu was open: nothing left to act on.
  const gone = !!at && !i;
  useEffect(() => {
    if (gone && ref.current?.matches(":popover-open")) ref.current.hidePopover();
  }, [gone]);

  const tiny = { w: settings?.microWidth ?? 320, h: settings?.microHeight ?? 240 };
  const show = (what: WindowShow) => i && api.instanceShow(i.id, what).catch(toastError);
  const limits = (patch: Partial<Limits>) => i && api.instanceSetLimits(i.id, { ...i.limits, ...patch }).catch(toastError);
  const micro = () =>
    i &&
    api.instanceMicro(i.id, !i.micro).then(([w, h]) => {
      if (!i.micro && (w > tiny.w || h > tiny.h)) toast(t("dashboard.microKept", { w, h }));
    }, toastError);
  const trim = () => i && api.instanceTrim(i.id).then(([before, after]) => toast(t("dashboard.trimmedFrom", { before: mb(before), after: mb(after) })), toastError);

  return (
    <ContextMenu ref={ref} at={at} label={t("dashboard.clientActions")} className="home-menu" style={{ maxHeight: "calc(100vh - 8px)" }} onClose={onClose}>
      {i && at && (
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
          <MenuSeparator />
          <MenuItem icon="x" disabled={closing.has(closeKey(i))} onSelect={() => onCloseClient(i, false)}>{t("common.close")}</MenuItem>
          <MenuItem icon="x" danger onSelect={() => onCloseClient(i, true)}>{t("dashboard.forceClose")}</MenuItem>
        </>
      )}
    </ContextMenu>
  );
}
