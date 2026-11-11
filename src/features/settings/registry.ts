// Every setting RoALT has, as data: where its row lives, its words, its control and when it shows.
// Settings renders from it, and one search covers Settings, Performance, Discord and Macros, plus
// links to per-account, per-launch and per-client settings in other views.
import { LANGUAGES, t, type Key } from "../../i18n";
import { motionPreference } from "../../ui/motion";
import type { View } from "../../app/navigation";
import type { AntiAfk, DiscordEvents, Settings, VaultStatus } from "../../api";
import type { MemThrottle } from "../../bindings/MemThrottle";
import type { RenderFlags } from "../../bindings/RenderFlags";
import type { AreaDef, SettingsArea } from "./areas";
import { SHORTCUTS } from "./shortcuts";
import { PERFORMANCE_ENTRIES } from "../performance/entries";
import { DISCORD_ENTRIES } from "../discord/entries";
import { MACROS_ENTRIES } from "../macros/entries";

export type { AreaDef, SettingsArea };

/** Where a row lives. A link-only place is another view's: search and jumps go there. */
export type Place =
  | { view: "settings"; area: SettingsArea; group?: Key }
  | { view: "performance"; group: Key }
  | { view: "discord" | "macros"; group?: Key }
  | { view: Extract<View, "accounts" | "launch" | "dashboard">; link: true };

/** What conditions and changing lines read. `background`: a background image is set. */
export type Ctx = { s: Settings; vault: VaultStatus; background: boolean };
export type Dyn = Key | ((c: Ctx) => Key);
/** Words: a key, a key with values, or text no language changes ("MB", a language's own name). */
export type Label = Key | { key: Key; params: Record<string, string | number> } | { text: string };

/** What a row reads and writes: a Settings field, a nested one, the vault's mode or this PC's own store. */
export type Field =
  | Exclude<keyof Settings, "antiAfk" | "discord" | "adaptive" | "memThrottle" | "renderFlags">
  | `antiAfk.${keyof AntiAfk}`
  | `discord.${keyof DiscordEvents}`
  | `memThrottle.${keyof MemThrottle}`
  | "adaptive.enabled"
  | `adaptive.background.${"cpuCap" | "eco" | "trimEveryMin"}`
  | `renderFlags.${keyof RenderFlags}`
  | "vault.mode"
  | "local:sounds"
  | "local:motion"
  | "local:previewFps";

/** `unit` names the number for screen readers; `short` is the suffix on screen. */
type Units = { unit: Label; short: Label };
export type Control =
  | { kind: "switch" }
  | { kind: "segmented"; label?: Key; options: readonly { value: string | number; label: Label; title?: Label }[]; mono?: boolean }
  | { kind: "select"; options: readonly { value: string; label: Label; lang?: string }[] }
  /** `scale`: stored = shown × scale (seconds on screen, milliseconds stored). */
  | ({ kind: "number"; min: number; max: number; step?: number; scale?: number } & Units)
  /** 0 is Off; `on` is the value switching it on starts at. */
  | ({ kind: "offOrValue"; min: number; max: number; step?: number; on: number } & Units)
  | { kind: "text"; placeholder: Key; size: "sm" | "md" | "lg" }
  | { kind: "shortcut"; combo: string }
  /** Drawn by its view's renderer (`Renderers`). */
  | { kind: "custom" }
  /** A one-off action: drawn by its view's renderer, never shows a saved state. */
  | { kind: "action" }
  /** Lives in a link-only place. */
  | { kind: "link" };

export type Entry = {
  /** Stable: the row's DOM id is `set-{id}`; the jump target; the React key. */
  id: string;
  place: Place;
  /** The row's name; also what search reads for a row that draws its own title. */
  label: Key;
  /** The one visible line. */
  hint?: Dyn;
  /** The "More about" paragraphs; searched while closed. */
  more?: Dyn | readonly Dyn[];
  /** What the row reads and writes; a declarative control has exactly one. */
  field?: Field | readonly Field[];
  control: Control;
  /** Drawn inside that row, while `when` holds. */
  parent?: string;
  /** Drawn only while this holds. */
  when?: (c: Ctx) => boolean;
  /** Where search and jumps go while `when` is false (else the parent). */
  fallback?: string;
  /** Queries a successful write makes stale. */
  invalidates?: readonly ("multi" | "ahk")[];
  /** ASCII search words (fps, afk, qos): never shown, never translated. */
  aliases?: readonly string[];
};

const SWITCH = { kind: "switch" } as const;
const CUSTOM = { kind: "custom" } as const;
const ACTION = { kind: "action" } as const;
const LINK = { kind: "link" } as const;
const launching = { view: "settings", area: "launching" } as const;
const appearance = { view: "settings", area: "appearance" } as const;
const security = { view: "settings", area: "security" } as const;
const data = { view: "settings", area: "data" } as const;
const about = { view: "settings", area: "about" } as const;
const master = (c: Ctx) => c.vault.mode === "master";

/** Settings' own rows, in screen order (areas.ts groups them). */
const SETTINGS_ENTRIES: readonly Entry[] = [
  { id: "multi", place: launching, label: "settings.multi", hint: "settings.multiShort", more: "settings.multiHint", field: "multiInstance", control: CUSTOM, aliases: ["mutex", "instances", "several"] },
  {
    id: "protectLogin", place: launching, label: "settings.protectLogin", hint: "settings.protectLoginShort", more: "settings.protectLoginHint", field: "protectSharedLogin",
    control: SWITCH, invalidates: ["multi"], aliases: ["773", "session"],
  },
  {
    id: "launchDelay", place: launching, label: "settings.launchDelay", more: "settings.launchDelayHint", field: "launchDelayMs",
    control: { kind: "number", min: 2, max: 60, step: 0.5, scale: 1000, unit: "settings.unit.secondsBetween", short: "settings.unit.s" }, aliases: ["stagger", "wait"],
  },
  {
    id: "launchArgs", place: launching, label: "settings.launchArgs", hint: "settings.launchArgsShort", more: "settings.launchArgsHint", field: "launchArgs",
    control: { kind: "text", placeholder: "settings.placeholderNone", size: "lg" }, aliases: ["args", "bootstrapper", "command"],
  },
  { id: "protectMain", place: launching, label: "settings.protectMain", more: "settings.protectMainHint", field: "protectMain", control: SWITCH, aliases: ["select all"] },
  { id: "autoRejoin", place: launching, label: "settings.autoRejoin", hint: "settings.autoRejoinHint", field: "autoRejoin", control: SWITCH, aliases: ["reconnect", "disconnect", "kick"] },
  { id: "autoRestart", place: launching, label: "settings.autoRestart", hint: "settings.autoRestartShort", more: "settings.autoRestartHint", field: "autoRestartOnCrash", control: SWITCH, aliases: ["crash", "hang", "relaunch"] },
  { id: "breaker", place: launching, label: "settings.breaker", more: "settings.breakerHint", field: ["breakerMax", "breakerWindowMin"], control: CUSTOM, aliases: ["breaker", "retries", "tries"] },
  { id: "antiAfk", place: launching, label: "settings.antiAfk", hint: "settings.antiAfkShort", more: ["settings.antiAfkHint", "settings.antiAfkHowHint"], field: "antiAfk.enabled", control: SWITCH, aliases: ["afk", "idle", "kick"] },
  {
    id: "antiAfkEvery", place: launching, parent: "antiAfk", when: (c) => c.s.antiAfk.enabled, label: "settings.antiAfkEvery", hint: "settings.antiAfkEveryHint", field: "antiAfk.intervalMin",
    control: { kind: "number", min: 1, max: 19, unit: "settings.unit.minutes", short: "settings.unit.min" }, aliases: ["afk", "interval"],
  },
  {
    id: "panic", place: launching, label: "settings.panic", hint: "settings.panicShort", more: "settings.panicHint", field: "panicHotkey",
    control: { kind: "text", placeholder: "settings.placeholderOff", size: "md" }, aliases: ["kill", "hotkey", "emergency"],
  },
  { id: "closeGrace", place: launching, label: "settings.closeGrace", hint: "settings.closeGraceHint", field: "closeGraceS", control: { kind: "number", min: 1, max: 60, unit: "settings.unit.seconds", short: "settings.unit.s" } },
  { id: "shutdown", place: launching, label: "settings.shutdown", hint: "settings.shutdownShort", more: "settings.shutdownHintClose", control: ACTION, aliases: ["timer", "power", "turn off"] },
  { id: "client", place: launching, label: "settings.client", more: "settings.clientInstallHint", control: ACTION, aliases: ["roblox", "install", "update", "version", "bloxstrap"] },
  { id: "autoUpdate", place: launching, label: "settings.autoUpdate", more: "settings.autoUpdateHint", field: "autoUpdate", control: SWITCH, aliases: ["roblox", "version"] },

  {
    id: "theme", place: appearance, label: "settings.theme", hint: "settings.themeHint", field: "theme", aliases: ["dark", "light", "mode"],
    control: { kind: "segmented", options: [{ value: "dark", label: "settings.themeDark" }, { value: "light", label: "settings.themeLight" }, { value: "system", label: "settings.themeSystem" }] },
  },
  { id: "accent", place: appearance, label: "settings.accentColour", hint: "settings.accentHint", field: "accent", control: CUSTOM, aliases: ["color", "colour", "highlight"] },
  { id: "background", place: appearance, label: "settings.background", more: "settings.backgroundHint", control: CUSTOM, aliases: ["wallpaper", "picture", "image"] },
  {
    id: "backgroundDim", place: appearance, parent: "background", when: (c) => c.background, label: "settings.backgroundDim", field: "backgroundDim",
    control: { kind: "segmented", mono: true, options: [40, 60, 80].map((n) => ({ value: n, label: { key: "settings.percent", params: { n } }, title: { key: "settings.backgroundCovers", params: { n } } })) },
  },
  {
    id: "density", place: appearance, label: "settings.density", hint: "settings.densityHint", field: "density", aliases: ["compact", "rows"],
    control: { kind: "segmented", options: [{ value: "detailed", label: "settings.densityDetailed" }, { value: "compact", label: "settings.densityCompact" }] },
  },
  {
    id: "tabLayout", place: appearance, label: "settings.tabLayout", hint: "settings.tabLayoutHint", field: "tabLayout", aliases: ["sidebar", "rail", "tabs"],
    control: { kind: "segmented", options: [{ value: "top", label: "settings.tabLayoutTop" }, { value: "side", label: "settings.tabLayoutSide" }] },
  },
  {
    id: "language", place: appearance, label: "settings.language", hint: "settings.languageHint", field: "language", aliases: ["lang", "translation", "locale"],
    control: { kind: "select", options: [{ value: "", label: "settings.languageWindows" }, ...LANGUAGES.map(([value, text]) => ({ value, label: { text }, lang: value }))] },
  },
  { id: "sounds", place: appearance, label: "settings.soundEffects", hint: "settings.soundHint", field: "local:sounds", control: CUSTOM, aliases: ["audio", "volume", "sfx"] },
  {
    id: "motion", place: appearance, label: "settings.motion", field: "local:motion", control: CUSTOM, aliases: ["animation", "animations", "movement", "reduced"],
    hint: () => ({ on: "settings.motionHintOn", reduced: "settings.motionHintReduced", off: "settings.motionHintOff" } as const)[motionPreference()],
  },
  { id: "closeToTray", place: appearance, label: "settings.closeToTray", hint: "settings.closeToTrayHint", field: "closeToTray", control: SWITCH, aliases: ["minimize", "hide"] },
  { id: "skipSplash", place: appearance, label: "settings.skipSplash", hint: "settings.skipSplashHint", field: "skipSplash", control: SWITCH, aliases: ["intro", "startup", "animation"] },

  {
    id: "encryption", place: security, label: "settings.encryption", field: "vault.mode", control: CUSTOM, aliases: ["dpapi", "aes", "password", "hello", "vault"],
    hint: (c) => (master(c) ? "settings.encryptionHintMaster" : "settings.encryptionHintDpapi"),
    more: (c) => (master(c) ? "settings.encryptionMoreMaster" : "settings.clipboardHintHello"),
  },
  { id: "masterPassword", place: security, when: master, fallback: "encryption", label: "settings.masterPassword", hint: "settings.masterPasswordHint", control: ACTION, aliases: ["password", "change"] },
  {
    id: "recoveryCode", place: security, when: master, fallback: "encryption", label: "vault.recovery.code", control: ACTION, aliases: ["forgot", "recover"],
    hint: (c) => (c.vault.hasRecovery ? "vault.recovery.rowHintOn" : "vault.recovery.rowHintOff"),
  },
  {
    id: "autoLock", place: security, when: master, fallback: "encryption", label: "settings.autoLock", hint: "settings.autoLockHint", field: "autoLockMinutes", aliases: ["lock", "idle", "timeout"],
    control: { kind: "segmented", label: "settings.autoLockLabel", mono: true, options: [0, 5, 15, 30, 60].map((n) => ({ value: n, label: n ? { key: "settings.minutes", params: { n } } : "common.off" })) },
  },
  { id: "clipboard", place: security, label: "settings.clipboard", field: "clipboardClearSeconds", control: { kind: "number", min: 5, max: 300, unit: "settings.unit.seconds", short: "settings.unit.s" }, aliases: ["copy", "cookie"] },
  { id: "publicNetworks", place: security, label: "settings.public", hint: "settings.publicShort", more: "settings.publicHint", field: "allowPublicNetworks", control: CUSTOM, aliases: ["wifi", "network", "cafe", "hotel"] },

  { id: "dataFolder", place: data, label: "settings.dataFolder", control: ACTION, aliases: ["folder", "path", "appdata", "logs", "move"] },
  { id: "backup", place: data, label: "settings.backup", hint: "settings.backupHint", control: ACTION, aliases: ["export", "import", "restore"] },
  { id: "cache", place: data, label: "settings.cache", more: "settings.cacheHint", field: "cacheMinutes", control: { kind: "number", min: 1, max: 1440, unit: "settings.unit.minutes", short: "settings.unit.min" }, aliases: ["info", "refresh"] },
  { id: "cleanLogs", place: data, label: "settings.cleanLogs", more: "settings.cleanLogsHint", field: "cleanLogs", control: CUSTOM, aliases: ["logs", "disk", "space"] },
  { id: "browserData", place: data, label: "settings.browserData", hint: "settings.browserDataShort", more: "settings.browserDataHint", control: ACTION, aliases: ["captcha", "webview", "reset"] },

  { id: "selfUpdate", place: about, label: "settings.selfUpdate", more: "settings.selfUpdateHint", field: "selfUpdate", control: CUSTOM, aliases: ["update", "github", "version"] },
  { id: "version", place: about, label: "settings.openReleases", hint: "settings.aboutShort", more: "settings.aboutHint", control: ACTION, aliases: ["version", "about", "changelog", "privacy"] },
  { id: "support", place: about, label: "app.joinDiscord", control: ACTION, aliases: ["donate", "robux", "community", "help"] },
];

const accounts = { view: "accounts", link: true } as const;
/** Settings that live in other views: an account's own, one launch's, one client's. */
const LINK_ENTRIES: readonly Entry[] = [
  { id: "acct.main", place: accounts, label: "accounts.detail.main", hint: "accounts.detail.mainHint", control: LINK },
  { id: "acct.fullQuality", place: accounts, label: "accounts.detail.fullQuality", hint: "accounts.detail.fullQualityHint", control: LINK, aliases: ["graphics", "quality"] },
  { id: "acct.bringBack", place: accounts, label: "accounts.detail.bringBack", hint: "accounts.detail.bringBackHint", control: LINK, aliases: ["rejoin", "disconnect", "restart"] },
  { id: "acct.discordMuted", place: accounts, label: "accounts.detail.muteDiscord", hint: "accounts.detail.muteDiscordHint", control: LINK, aliases: ["alerts", "webhook"] },
  { id: "acct.launchArgs", place: accounts, label: "accounts.detail.launchArgs", control: LINK, aliases: ["args"] },
  { id: "acct.fpsCap", place: accounts, label: "accounts.detail.fpsCap", hint: "accounts.detail.fpsCapHint", control: LINK, aliases: ["fps"] },
  { id: "launch.windowLayout", place: { view: "launch", link: true }, label: "launch.layout", hint: "launch.layoutHint", field: "windowLayout", control: LINK, aliases: ["tile", "fit", "tiny"] },
  { id: "launch.fpsCap", place: { view: "launch", link: true }, label: "launch.fpsCap", hint: "launch.fpsHint", control: LINK, aliases: ["fps"] },
  { id: "client.antiAfk", place: { view: "dashboard", link: true }, label: "launch.limits.antiAfkClient", control: LINK, aliases: ["afk"] },
];

const KEY_ENTRIES: readonly Entry[] = SHORTCUTS.map((s) => ({
  id: `key.${s.id}`,
  place: { view: "settings", area: "shortcuts" },
  label: s.label,
  control: { kind: "shortcut", combo: s.combo },
  // The keys too ("f5", "ctrl+l"): aliases aren't folded, the query is lower-cased.
  aliases: ["shortcut", "hotkey", s.combo.toLowerCase()],
}));

export const ENTRIES: readonly Entry[] = [...SETTINGS_ENTRIES, ...PERFORMANCE_ENTRIES, ...DISCORD_ENTRIES, ...MACROS_ENTRIES, ...LINK_ENTRIES, ...KEY_ENTRIES];

const BY_ID = new Map(ENTRIES.map((e) => [e.id, e]));
const KIDS = new Map<string, Entry[]>();
for (const e of ENTRIES) if (e.parent) KIDS.set(e.parent, [...(KIDS.get(e.parent) ?? []), e]);

export function entry(id: string): Entry {
  const e = BY_ID.get(id);
  if (!e) throw new Error(`No setting "${id}" in the registry`);
  return e;
}

/** The rows drawn inside `id`'s row, in registry order. */
export const childrenOf = (id: string): readonly Entry[] => KIDS.get(id) ?? [];

/** Whether the row is drawn now: its own condition and its parents'. */
export const shown = (e: Entry, c: Ctx): boolean => (!e.when || e.when(c)) && (!e.parent || shown(entry(e.parent), c));

export const say = (l: Label) => (typeof l === "string" ? t(l) : "key" in l ? t(l.key, l.params) : l.text);
export const dyn = (d: Dyn, c: Ctx) => t(typeof d === "function" ? d(c) : d);
/** The row's "More about" paragraphs, in the current language. */
export const paragraphs = (e: Entry, c: Ctx) => (e.more === undefined ? [] : typeof e.more === "object" ? e.more : [e.more]).map((d) => dyn(d, c));

if (import.meta.env.DEV) {
  if (BY_ID.size !== ENTRIES.length) console.warn("settings registry: two entries share an id");
  for (const e of ENTRIES)
    for (const ref of [e.parent, e.fallback]) if (ref && !BY_ID.has(ref)) console.warn(`settings registry: "${e.id}" points at "${ref}", which isn't an entry`);
}
