import { useEffect } from "react";
import type { Key } from "../i18n";
import type { IconName } from "../ui/Icon";
import type { SettingsArea } from "../features/settings/areas";
import type { LaunchTarget } from "../api";

export type View = "dashboard" | "accounts" | "launch" | "browse" | "servers" | "monitor" | "discord" | "mods" | "crashes" | "debug" | "settings";

export const VIEW_NAME: Record<View, Key> = {
  dashboard: "app.nav.dashboard",
  accounts: "app.nav.accounts",
  launch: "app.nav.launch",
  browse: "app.nav.browse",
  servers: "app.nav.servers",
  monitor: "app.nav.monitor",
  discord: "app.nav.discord",
  mods: "app.nav.mods",
  crashes: "app.nav.crashes",
  debug: "app.nav.debug",
  settings: "app.nav.settings",
};

/** The island's places, by the ids `roalt.sectionOrder` keeps (Debug moved into Tools). */
export const MAIN_SECTIONS = ["home", "play", "browse", "accounts"] as const;
export const TOOL_SECTIONS = ["servers", "monitor", "discord", "mods", "crashes", "debug"] as const;
export type Section = (typeof MAIN_SECTIONS)[number] | (typeof TOOL_SECTIONS)[number];

export const SECTION_VIEW: Record<Section, View> = {
  home: "dashboard",
  play: "launch",
  browse: "browse",
  accounts: "accounts",
  servers: "servers",
  monitor: "monitor",
  discord: "discord",
  mods: "mods",
  crashes: "crashes",
  debug: "debug",
};
export const SECTION_ICON: Record<Section, IconName> = {
  home: "home",
  play: "play",
  browse: "globe",
  accounts: "accounts",
  servers: "servers",
  monitor: "monitor",
  discord: "bell",
  mods: "palette",
  crashes: "alert",
  debug: "terminal",
};

/** Ctrl and a digit, by key position (so Ctrl+1 works where the digits need Shift). */
export const SHORTCUT_VIEW: Record<string, View> = {
  Digit1: "dashboard",
  Digit2: "accounts",
  Digit3: "launch",
  Digit4: "servers",
  Digit5: "settings",
  Digit6: "monitor",
  Digit7: "discord",
  Digit8: "debug",
};
export function shortcutFor(view: View) {
  const code = Object.keys(SHORTCUT_VIEW).find((c) => SHORTCUT_VIEW[c] === view);
  return code ? `Ctrl+${code.slice(5)}` : undefined;
}

/** Browse as this account, on this page of www.roblox.com (its home when none). `pick`: for Play, so
 * a Play or Join there fills Play's target and starts nothing. */
export type BrowseRequest = { accountId: number; path?: string; pick?: boolean };

// Navigation asked for from outside the shell (the app menu, a notice): the shell listens.
// `play`: a target Play opens filled in with.
export type Request = { view: View; area?: SettingsArea; browse?: BrowseRequest; play?: LaunchTarget };
const subs = new Set<(r: Request) => void>();
export function requestNavigation(view: View, area?: SettingsArea, browse?: BrowseRequest, play?: LaunchTarget) {
  subs.forEach((f) => f({ view, area, browse, play }));
}
/** Opens Play with `target` filled in: the accounts are picked there. */
export const playTarget = (target: LaunchTarget) => requestNavigation("launch", undefined, undefined, target);
/** Browse as an account (its "Open on roblox.com"; `pick`: Play's "Browse games"). */
export const browseAs = (accountId: number, path?: string, pick?: boolean) => requestNavigation("browse", undefined, { accountId, path, pick });
export function useNavigationRequests(handle: (r: Request) => void) {
  useEffect(() => {
    subs.add(handle);
    return () => void subs.delete(handle);
  }, [handle]);
}
