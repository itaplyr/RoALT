import type { Key } from "../../i18n";
import { shortcutFor, type View } from "../../app/navigation";

/** A keyboard shortcut as the Shortcuts area lists it; "Ctrl+L" shows as keycaps. */
export type Shortcut = { id: string; label: Key; combo: string };

// The view keys come from the shell's own table, so the list can't drift from what Ctrl+digit does.
const go = (view: View, label: Key): Shortcut => ({ id: view, label, combo: shortcutFor(view) ?? "" });

export const SHORTCUTS: readonly Shortcut[] = [
  go("dashboard", "settings.shortcut.dashboard"),
  go("accounts", "settings.shortcut.accounts"),
  go("launch", "settings.shortcut.launch"),
  go("servers", "settings.shortcut.servers"),
  go("settings", "settings.shortcut.settings"),
  go("monitor", "settings.shortcut.monitor"),
  go("discord", "settings.shortcut.discord"),
  go("debug", "settings.shortcut.debug"),
  go("performance", "settings.shortcut.performance"),
  { id: "add", label: "settings.shortcut.add", combo: "Ctrl+N" },
  { id: "find", label: "settings.shortcut.filter", combo: "Ctrl+F" },
  { id: "selectAll", label: "settings.shortcut.selectAll", combo: "Ctrl+A" },
  { id: "validate", label: "settings.shortcut.validate", combo: "Ctrl+R" },
  { id: "remove", label: "settings.shortcut.remove", combo: "Delete" },
  { id: "launchNow", label: "settings.shortcut.launchNow", combo: "Ctrl+Enter" },
  { id: "refresh", label: "settings.shortcut.refresh", combo: "F5" },
  { id: "lock", label: "settings.shortcut.lock", combo: "Ctrl+L" },
  { id: "close", label: "settings.shortcut.close", combo: "Esc" },
  { id: "help", label: "settings.shortcut.help", combo: "?" },
];
