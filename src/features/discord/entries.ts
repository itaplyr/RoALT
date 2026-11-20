import type { Entry } from "../settings/registry";
import { RANGES } from "../../ui/ranges";

const events = { view: "discord", group: "discord.events" } as const;
const ev = (key: "launch" | "crash" | "hang" | "disconnect" | "restart" | "memRestart" | "breaker" | "batchDone" | "signIn" | "throttle" | "update" | "pc", label: Entry["label"], hint: Entry["label"]): Entry => ({
  id: `ev.${key}`,
  place: events,
  label,
  hint,
  field: `discord.${key}`,
  control: { kind: "switch" },
});

const rpcPlace = { view: "discord" } as const;
const rpcOn = (c: { s: { discordPresence: boolean } }) => c.s.discordPresence;

/** Discord's settings for the settings registry (search, jumps, rows), in screen order. */
export const DISCORD_ENTRIES: readonly Entry[] = [
  { id: "presence", place: rpcPlace, label: "settings.discord", hint: "discord.presenceShort", more: "settings.discordHint", field: "discordPresence", control: { kind: "switch" }, aliases: ["rpc", "rich presence", "status"] },
  { id: "rpcShowGame", place: rpcPlace, parent: "presence", when: rpcOn, fallback: "presence", label: "discord.rpc.showGame", hint: "discord.rpc.showGameHint", field: "discordRpc.showGame", control: { kind: "switch" }, aliases: ["rpc", "rich presence", "game"] },
  { id: "rpcShowCount", place: rpcPlace, parent: "presence", when: rpcOn, fallback: "presence", label: "discord.rpc.showCount", hint: "discord.rpc.showCountHint", field: "discordRpc.showCount", control: { kind: "switch" }, aliases: ["rpc", "rich presence", "alts", "count"] },
  { id: "rpcShowElapsed", place: rpcPlace, parent: "presence", when: rpcOn, fallback: "presence", label: "discord.rpc.showElapsed", hint: "discord.rpc.showElapsedHint", field: "discordRpc.showElapsed", control: { kind: "switch" }, aliases: ["rpc", "rich presence", "timer", "time"] },
  { id: "rpcShowButtons", place: rpcPlace, parent: "presence", when: rpcOn, fallback: "presence", label: "discord.rpc.showButtons", hint: "discord.rpc.showButtonsHint", field: "discordRpc.showButtons", control: { kind: "switch" }, aliases: ["rpc", "rich presence", "buttons", "invite"] },
  { id: "rpcCustomDetails", place: rpcPlace, parent: "presence", when: rpcOn, fallback: "presence", label: "discord.rpc.customDetails", hint: "discord.rpc.customHint", field: "discordRpc.customDetails", control: { kind: "text", placeholder: "discord.rpc.autoPlaceholder", size: "lg" }, aliases: ["rpc", "rich presence", "custom", "status", "text", "details"] },
  { id: "rpcCustomState", place: rpcPlace, parent: "presence", when: rpcOn, fallback: "presence", label: "discord.rpc.customState", hint: "discord.rpc.customHint", field: "discordRpc.customState", control: { kind: "text", placeholder: "discord.rpc.autoPlaceholder", size: "lg" }, aliases: ["rpc", "rich presence", "custom", "status", "text", "state"] },
  { id: "webhook", place: rpcPlace, label: "discord.webhook", control: { kind: "custom" }, aliases: ["url", "channel", "alerts", "notifications"] },
  {
    id: "ping", place: { view: "discord" }, label: "discord.ping", hint: "discord.pingHint", field: "discord.ping",
    control: { kind: "text", placeholder: "settings.placeholderOff", size: "md" }, aliases: ["mention", "notify", "user id", "role"],
  },
  {
    id: "pingLevel", place: { view: "discord" }, label: "discord.pingLevel", hint: "discord.pingLevelHint", field: "discord.pingLevel",
    control: {
      kind: "segmented",
      label: "discord.pingLevel",
      options: [
        { value: "urgent", label: "discord.pingUrgent" },
        { value: "warnings", label: "discord.pingWarnings" },
      ],
    },
    aliases: ["mention", "notify", "severity", "urgent", "warnings"],
  },
  ev("launch", "discord.evLaunch", "discord.evLaunchHint"),
  ev("crash", "discord.evCrash", "discord.evCrashHint"),
  ev("hang", "discord.evHang", "discord.evHangHint"),
  ev("disconnect", "discord.evDisconnect", "discord.evDisconnectHint"),
  ev("signIn", "discord.evSignIn", "discord.evSignInHint"),
  ev("restart", "discord.evRestart", "discord.evRestartHint"),
  ev("memRestart", "discord.evMemRestart", "discord.evMemRestartHint"),
  ev("breaker", "discord.evBreaker", "discord.evBreakerHint"),
  ev("batchDone", "discord.evBatchDone", "discord.evBatchDoneHint"),
  ev("throttle", "discord.evThrottle", "discord.evThrottleHint"),
  ev("update", "discord.evUpdate", "discord.evUpdateHint"),
  ev("pc", "discord.evPc", "discord.evPcHint"),
  {
    id: "statusSummary", place: events, label: "discord.statusSummary", hint: "discord.statusSummaryHint", field: "discord.statusEveryMin",
    control: { kind: "offOrValue", ...RANGES.statusEveryMin, unit: "settings.unit.minutes", short: "settings.unit.min" }, aliases: ["report", "ram", "cpu"],
  },
];
