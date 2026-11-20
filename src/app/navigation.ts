import { useEffect } from "react";
import type { Key } from "../i18n";
import type { IconName } from "../ui/Icon";
import type { SettingsArea } from "../features/settings/areas";

export type View = "dashboard" | "accounts" | "launch" | "browse" | "performance" | "servers" | "monitor" | "discord" | "debug" | "settings";

export const VIEW_NAME: Record<View, Key> = {
  dashboard: "app.nav.dashboard",
  accounts: "app.nav.accounts",
  launch: "app.nav.launch",
  browse: "app.nav.browse",
  performance: "app.nav.performance",
  servers: "app.nav.servers",
  monitor: "app.nav.monitor",
  discord: "app.nav.discord",
  debug: "app.nav.debug",
  settings: "app.nav.settings",
};

/** The island's places, by the ids `roalt.sectionOrder` keeps (Debug moved into Tools). */
export const MAIN_SECTIONS = ["home", "play", "browse", "accounts", "performance"] as const;
export const TOOL_SECTIONS = ["servers", "monitor", "discord", "debug"] as const;
export type Section = (typeof MAIN_SECTIONS)[number] | (typeof TOOL_SECTIONS)[number];

export const SECTION_VIEW: Record<Section, View> = {
  home: "dashboard",
  play: "launch",
  browse: "browse",
  accounts: "accounts",
  performance: "performance",
  servers: "servers",
  monitor: "monitor",
  discord: "discord",
  debug: "debug",
};
export const SECTION_ICON: Record<Section, IconName> = {
  home: "home",
  play: "play",
  browse: "globe",
  accounts: "accounts",
  performance: "gauge",
  servers: "servers",
  monitor: "monitor",
  discord: "bell",
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
  Digit9: "performance",
};
export function shortcutFor(view: View) {
  const code = Object.keys(SHORTCUT_VIEW).find((c) => SHORTCUT_VIEW[c] === view);
  return code ? `Ctrl+${code.slice(5)}` : undefined;
}

/** Browse as this account, on this page of www.roblox.com (its home when none). */
export type BrowseRequest = { accountId: number; path?: string };

// Navigation asked for from outside the shell (the app menu, a notice): the shell listens.
export type Request = { view: View; area?: SettingsArea; browse?: BrowseRequest };
const subs = new Set<(r: Request) => void>();
export function requestNavigation(view: View, area?: SettingsArea, browse?: BrowseRequest) {
  subs.forEach((f) => f({ view, area, browse }));
}
/** Browse as an account (its "Open on roblox.com"). */
export const browseAs = (accountId: number, path?: string) => requestNavigation("browse", undefined, { accountId, path });
export function useNavigationRequests(handle: (r: Request) => void) {
  useEffect(() => {
    subs.add(handle);
    return () => void subs.delete(handle);
  }, [handle]);
}
