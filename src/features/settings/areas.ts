import type { Key } from "../../i18n";
import type { IconName } from "../../ui/Icon";
import { ENTRIES } from "./registry";
import { SHORTCUTS } from "./shortcuts";

/** Settings' areas, one screen each, picked from the island's Settings group. */
export type SettingsArea = "launching" | "appearance" | "security" | "data" | "about" | "shortcuts";

/** An area's rows by group, in screen order. A group without a label is the area's first block. */
export type AreaDef = { id: SettingsArea; label: Key; icon: IconName; groups: readonly { label?: Key; ids: readonly string[] }[] };

export const AREAS: readonly AreaDef[] = [
  {
    id: "launching",
    label: "settings.section.launching",
    icon: "rocket",
    groups: [
      { ids: ["multi", "protectLogin", "launchDelay", "launchArgs", "protectMain"] },
      { label: "settings.group.recovery", ids: ["autoRejoin", "autoRestart", "breaker", "antiAfk"] },
      { label: "settings.group.stopping", ids: ["panic", "closeGrace", "shutdown"] },
      { label: "settings.client", ids: ["client", "autoUpdate"] },
    ],
  },
  {
    id: "appearance",
    label: "settings.section.appearance",
    icon: "palette",
    groups: [
      { ids: ["theme", "accent", "background", "density", "tabLayout", "language", "sounds", "motion"] },
      { label: "settings.group.window", ids: ["closeToTray", "skipSplash"] },
    ],
  },
  {
    id: "security",
    label: "settings.section.security",
    icon: "shield",
    groups: [{ ids: ["encryption", "masterPassword", "recoveryCode", "autoLock", "clipboard", "publicNetworks"] }],
  },
  { id: "data", label: "settings.section.data", icon: "database", groups: [{ ids: ["dataFolder", "backup", "cache", "cleanLogs", "browserData"] }] },
  { id: "about", label: "settings.section.about", icon: "info", groups: [{ ids: ["selfUpdate", "version", "support"] }] },
  { id: "shortcuts", label: "settings.section.shortcuts", icon: "keyboard", groups: [{ ids: SHORTCUTS.map((s) => `key.${s.id}`) }] },
];

if (import.meta.env.DEV) {
  for (const a of AREAS)
    for (const id of a.groups.flatMap((g) => g.ids)) {
      const e = ENTRIES.find((x) => x.id === id);
      if (e?.place.view !== "settings" || e.place.area !== a.id) console.warn(`settings: AREAS lists "${id}" in ${a.id}, but the registry ${e ? "places it elsewhere" : "has no such entry"}`);
    }
}
