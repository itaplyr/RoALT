import type { Key } from "../../i18n";
import type { IconName } from "../../ui/Icon";
import { SHORTCUTS } from "./shortcuts";

/** Settings' areas: the parts of its one page, listed at its side. */
export type SettingsArea = "performance" | "launching" | "appearance" | "security" | "data" | "about" | "shortcuts";

/** A group that isn't rows: Settings draws that panel under its name (SettingsView's PANELS). */
export type Panel = "presets" | "bench" | "share" | "keys";

/** An area's rows by group, in page order. A group has no label only when it is the area's one group.
 * A `panel` group's ids are the entries it stands for (search names their place by it), not rows. */
export type AreaDef = { id: SettingsArea; label: Key; icon: IconName; groups: readonly { label?: Key; ids: readonly string[]; panel?: Panel }[] };

export const AREAS: readonly AreaDef[] = [
  {
    id: "performance",
    label: "performance.title",
    icon: "gauge",
    groups: [
      { label: "performance.presets", ids: ["presets", "maxAlts", "balanced", "defaults"], panel: "presets" },
      { label: "performance.bench.title", ids: ["bench"], panel: "bench" },
      { label: "performance.fineTune", ids: ["fpsCap", "lowGraphics", "lighterRendering", "customFlags", "minimize", "tinyOnLaunch", "mute"] },
      { label: "performance.section.cpuMemory", ids: ["adaptive", "fairCap", "spread", "trim", "memThrottle", "memRestart"] },
      { label: "performance.section.roalt", ids: ["previewFps"] },
      { label: "performance.section.network", ids: ["bandwidth"] },
      { label: "performance.shareTitle", ids: ["share"], panel: "share" },
    ],
  },
  {
    id: "launching",
    label: "settings.section.launching",
    icon: "rocket",
    groups: [
      { label: "settings.group.starting", ids: ["multi", "protectLogin", "launchDelay", "launchArgs", "protectMain"] },
      { label: "settings.group.recovery", ids: ["autoRejoin", "autoRestart", "breaker", "restartEvery", "closeAfter", "antiAfk"] },
      { label: "settings.group.stopping", ids: ["panic", "closeGrace", "shutdown"] },
      { label: "settings.client", ids: ["defaultLauncher", "askBrowserLinks", "client", "autoUpdate"] },
    ],
  },
  {
    id: "appearance",
    label: "settings.section.appearance",
    icon: "palette",
    groups: [
      { label: "settings.group.look", ids: ["theme", "themes", "look", "lookShare", "accent", "corners", "background", "density", "tabLayout", "language", "sounds", "motion"] },
      { label: "settings.group.window", ids: ["closeToTray", "skipSplash"] },
    ],
  },
  {
    id: "security",
    label: "settings.section.security",
    icon: "shield",
    groups: [{ ids: ["encryption", "masterPassword", "recoveryCode", "autoLock", "lockOnSessionLock", "clipboard", "signInWatch", "lockPurchases", "publicNetworks"] }],
  },
  { id: "data", label: "settings.section.data", icon: "database", groups: [{ ids: ["dataFolder", "backup", "cache", "cleanLogs", "browserData"] }] },
  { id: "about", label: "settings.section.about", icon: "info", groups: [{ ids: ["selfUpdate", "onlineCount", "news", "showSponsor", "rolimons", "version", "replayTour", "support"] }] },
  {
    id: "shortcuts",
    label: "settings.section.shortcuts",
    icon: "keyboard",
    groups: [
      { label: "settings.group.ownKeys", ids: SHORTCUTS.map((s) => `key.${s.id}`), panel: "keys" },
      { label: "settings.group.globalKeys", ids: ["keyToggle", "keyTile", "keyMinimize"] },
    ],
  },
];

/** Settings' rows, in page order: what the panels stand for is left out. */
export const ROW_IDS: readonly string[] = AREAS.flatMap((a) => a.groups.flatMap((g) => (g.panel ? [] : g.ids)));
