import { useEffect, useState } from "react";
import type { Instance, InstanceState } from "../../api";
import { t } from "../../i18n";
import { TONE_COLOR, type Tone } from "../../ui/Status";

/** A client's state as a status tone (shape and hue), the way cards, tabs and rail rows show it. */
export const STATE_TONE: Record<InstanceState, Tone> = { starting: "neutral", joining: "info", inGame: "ok", disconnected: "attention", exited: "neutral" };
const hue = (s: InstanceState) => TONE_COLOR[STATE_TONE[s]];

// What a client is doing, in words and status hues (never the accent: a user's accent must not
// repaint "in game"). Getters: labels are translated when read (at render), not once at load.
export const STATES: Record<InstanceState, [label: string, color: string]> = {
  get starting(): [string, string] { return [t("launch.state.starting"), hue("starting")]; },
  get joining(): [string, string] { return [t("launch.state.joining"), hue("joining")]; },
  get inGame(): [string, string] { return [t("launch.state.inGame"), hue("inGame")]; },
  get disconnected(): [string, string] { return [t("launch.state.disconnected"), hue("disconnected")]; },
  get exited(): [string, string] { return [t("launch.state.ended"), hue("exited")]; },
};

export const isRunning = (i: Instance) => i.state !== "exited";
/** Roblox has it running or on its way: its account can't start another. */
export const isLive = (i: Instance) => i.state === "starting" || i.state === "joining" || i.state === "inGame";

export function uptime(from: number, to: number) {
  const m = Math.max(0, Math.floor((to - from) / 60_000));
  return m < 60 ? t("settings.minutesShort", { n: m }) : t("launch.hoursMinutes", { h: Math.floor(m / 60), m: m % 60 });
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
