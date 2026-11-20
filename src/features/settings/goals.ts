import type { Key } from "../../i18n";
import type { IconName } from "../../ui/Icon";
import { ROW_IDS, type Panel } from "./areas";

/** Settings' rows a second way: by what a person wants to do, for someone who doesn't know RoALT's
 * own words yet. `start`: the rows to begin with; `tune`: the rest; `links`: entries of other views,
 * shown as the way there; `top` and `end`: panels (the preset cards, the key table) before its rows
 * and after them. */
export type Goal = { id: string; icon: IconName; title: Key; say: Key; start: readonly string[]; tune: readonly string[]; links: readonly string[]; top?: readonly Panel[]; end?: readonly Panel[] };

// A Settings row goes in exactly one goal: the registry's dev check says when one has none, or two.
const LISTED: readonly Goal[] = [
  {
    id: "many", icon: "accounts", title: "settings.goal.many", say: "settings.goal.manySay",
    start: ["multi", "launchDelay"], tune: ["protectLogin", "protectMain", "launchArgs"], links: ["launch.windowLayout", "launch.fpsCap", "acct.main"],
  },
  {
    id: "online", icon: "refresh", title: "settings.goal.online", say: "settings.goal.onlineSay",
    start: ["autoRejoin", "autoRestart", "antiAfk"], tune: ["closeToTray", "breaker", "restartEvery", "closeAfter", "memRestart", "signInWatch"], links: ["acct.bringBack", "client.antiAfk"],
  },
  { id: "stop", icon: "stop", title: "settings.goal.stop", say: "settings.goal.stopSay", start: ["panic"], tune: ["closeGrace", "shutdown"], links: [] },
  {
    id: "roblox", icon: "rocket", title: "settings.goal.roblox", say: "settings.goal.robloxSay",
    start: ["client", "autoUpdate"], tune: ["defaultLauncher", "askBrowserLinks", "browserData", "cleanLogs"], links: ["mods.on"],
  },
  {
    id: "lighter", icon: "gauge", title: "settings.goal.lighter", say: "settings.goal.lighterSay",
    top: ["presets", "bench"],
    start: ["fpsCap", "lowGraphics", "minimize", "mute"],
    tune: ["lighterRendering", "customFlags", "tinyOnLaunch", "adaptive", "fairCap", "spread", "trim", "memThrottle", "previewFps", "bandwidth"],
    end: ["share"],
    links: [],
  },
  {
    id: "look", icon: "palette", title: "settings.goal.look", say: "settings.goal.lookSay",
    start: ["theme", "themes", "accent", "language"], tune: ["look", "lookShare", "corners", "background", "density", "tabLayout", "sounds", "motion", "skipSplash"], links: [],
  },
  {
    id: "protect", icon: "shield", title: "settings.goal.protect", say: "settings.goal.protectSay",
    start: ["encryption", "masterPassword", "recoveryCode", "autoLock", "lockPurchases"], tune: ["lockOnSessionLock", "clipboard", "publicNetworks"], links: [],
  },
  { id: "data", icon: "database", title: "settings.goal.data", say: "settings.goal.dataSay", start: ["backup"], tune: ["dataFolder", "cache"], links: [] },
  {
    id: "alerts", icon: "bell", title: "settings.goal.alerts", say: "settings.goal.alertsSay",
    start: [], tune: [], links: ["presence", "webhook", "ev.crash", "ev.disconnect", "statusSummary", "acct.discordMuted"],
  },
  { id: "keys", icon: "keyboard", title: "settings.goal.keys", say: "settings.goal.keysSay", start: ["keyToggle", "keyTile", "keyMinimize"], tune: [], links: [], end: ["keys"] },
  { id: "about", icon: "info", title: "settings.goal.about", say: "settings.goal.aboutSay", start: ["selfUpdate"], tune: ["onlineCount", "news", "showSponsor", "rolimons", "version", "replayTour", "support"], links: [] },
];

const placed = new Set(LISTED.flatMap((g) => [...g.start, ...g.tune]));
/** Settings' rows no goal lists yet. */
export const UNPLACED: readonly string[] = ROW_IDS.filter((id) => !placed.has(id));

/** The goals, in list order. A row no goal lists shows under the last one, never nowhere. */
export const GOALS: readonly Goal[] = LISTED.map((g, i) => (i === LISTED.length - 1 ? { ...g, tune: [...g.tune, ...UNPLACED] } : g));
