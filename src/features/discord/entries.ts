import type { Entry } from "../settings/registry";
import { RANGES } from "../../ui/ranges";

const events = { view: "discord", group: "discord.events" } as const;
const ev = (key: "launch" | "crash" | "hang" | "disconnect" | "restart" | "memRestart" | "breaker" | "batchDone", label: Entry["label"], hint: Entry["label"]): Entry => ({
  id: `ev.${key}`,
  place: events,
  label,
  hint,
  field: `discord.${key}`,
  control: { kind: "switch" },
});

/** Discord's settings for the settings registry (search, jumps, rows), in screen order. */
export const DISCORD_ENTRIES: readonly Entry[] = [
  { id: "presence", place: { view: "discord" }, label: "settings.discord", hint: "discord.presenceShort", more: "settings.discordHint", field: "discordPresence", control: { kind: "switch" }, aliases: ["rpc", "rich presence", "status"] },
  { id: "webhook", place: { view: "discord" }, label: "discord.webhook", control: { kind: "custom" }, aliases: ["url", "channel", "alerts", "notifications"] },
  {
    id: "ping", place: { view: "discord" }, label: "discord.ping", hint: "discord.pingHint", field: "discord.ping",
    control: { kind: "text", placeholder: "settings.placeholderOff", size: "md" }, aliases: ["mention", "notify", "user id", "role"],
  },
  ev("launch", "discord.evLaunch", "discord.evLaunchHint"),
  ev("crash", "discord.evCrash", "discord.evCrashHint"),
  ev("hang", "discord.evHang", "discord.evHangHint"),
  ev("disconnect", "discord.evDisconnect", "discord.evDisconnectHint"),
  ev("restart", "discord.evRestart", "discord.evRestartHint"),
  ev("memRestart", "discord.evMemRestart", "discord.evMemRestartHint"),
  ev("breaker", "discord.evBreaker", "discord.evBreakerHint"),
  ev("batchDone", "discord.evBatchDone", "discord.evBatchDoneHint"),
  {
    id: "statusSummary", place: events, label: "discord.statusSummary", hint: "discord.statusSummaryHint", field: "discord.statusEveryMin",
    control: { kind: "offOrValue", ...RANGES.statusEveryMin, unit: "settings.unit.minutes", short: "settings.unit.min" }, aliases: ["report", "ram", "cpu"],
  },
];
