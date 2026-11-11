import { useEffect, useState } from "react";
import type { Instance, InstanceState } from "../../api";
import { t } from "../../i18n";

// What a client is doing, in words and status hues (never the accent: a user's accent must not
// repaint "in game"). Getters: labels are translated when read (at render), not once at load.
export const STATES: Record<InstanceState, [label: string, color: string]> = {
  get starting(): [string, string] { return [t("launch.state.starting"), "var(--fg-3)"]; },
  get joining(): [string, string] { return [t("launch.state.joining"), "var(--info)"]; },
  get inGame(): [string, string] { return [t("launch.state.inGame"), "var(--ok)"]; },
  get disconnected(): [string, string] { return [t("launch.state.disconnected"), "var(--attention)"]; },
  get exited(): [string, string] { return [t("launch.state.ended"), "var(--fg-3)"]; },
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
