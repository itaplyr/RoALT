import type { AccountSummary, Profile, Settings } from "../../api";
import type { RenderFlags } from "../../bindings/RenderFlags";
import { fmtList, t, type Key } from "../../i18n";
import { accountName } from "../accounts/shared";

/** What a preset's card lists; the review lists the rest. */
export const FACTS_SHOWN = 3;

/** Only Roblox defaults names these: the others leave a user's own memory limits alone. */
const noThrottle = { mb: 0, cpuCap: 10 };
const lightest: RenderFlags = { msaa: "x1", texture: "lowest", mode: "default", graySky: false, noGrass: true, lowestFrm: true, pauseVoxelizer: false };
/** What leaving Max alts must undo: a profile only changes what it names. */
const normalWindows = { tinyOnLaunch: false, microWidth: 320, microHeight: 240, fairCap: false };

export type Preset = { id: string; label: Key; tag: Key; facts: () => string[]; profile: Profile };

/** One-click setups: a profile each, reviewed like any other. `id`: its search entry. */
export const PRESETS: readonly Preset[] = [
  {
    id: "maxAlts",
    label: "performance.preset.maxAlts",
    tag: "performance.tagMaxAlts",
    facts: () => [
      t("dashboard.fps", { fps: 10 }), t("settings.lowGraphics"), t("performance.tinyOnLaunch"), t("settings.minimize"),
      t("performance.factFair", { n: 25 }), t("performance.factMainAccount"), t("performance.factBootstrapper"), t("performance.factAfk"), t("performance.factNextLaunch"),
    ],
    profile: {
      fpsCap: 10, lowGraphics: true, lighterRendering: true, renderFlags: { ...lightest, graySky: true, pauseVoxelizer: true }, minimizeOnLaunch: true, tinyOnLaunch: true, microWidth: 240, microHeight: 180, trimEveryMin: 15, autoSpread: true, fairCap: true,
      adaptive: { enabled: true, background: { cpuCap: 25, eco: true, trimEveryMin: null, affinity: null } }, previewFps: 5,
    },
  },
  {
    id: "balanced",
    label: "performance.preset.balanced",
    tag: "performance.tagBalanced",
    facts: () => [t("dashboard.fps", { fps: 30 }), t("settings.lowGraphics"), t("performance.factOnScreen"), t("performance.factBg", { n: 50 })],
    profile: {
      fpsCap: 30, lowGraphics: true, lighterRendering: true, renderFlags: lightest, minimizeOnLaunch: false, ...normalWindows, trimEveryMin: 30, autoSpread: true,
      adaptive: { enabled: true, background: { cpuCap: 50, eco: true, trimEveryMin: null, affinity: null } }, previewFps: 15,
    },
  },
  {
    id: "defaults",
    label: "performance.preset.defaults",
    tag: "performance.tagDefaults",
    facts: () => [t("performance.factNoFps"), t("performance.factOwnGraphics"), t("performance.factOnScreen"), t("performance.factNoCpu")],
    profile: {
      fpsCap: 0, lowGraphics: false, lighterRendering: false, minimizeOnLaunch: false, ...normalWindows, memRestartMb: 0, memThrottle: noThrottle, trimEveryMin: 0, autoSpread: false,
      adaptive: { enabled: false, background: { cpuCap: 25, eco: true, trimEveryMin: null, affinity: null } }, previewFps: 15,
    },
  },
];

/** A setting a profile can carry (the preview rate is this PC's, not a setting). */
export type ProfileKey = Exclude<keyof Profile, "previewFps">;

/** Each profile setting by the name the review gives it. */
export const LABELS: Record<ProfileKey, Key> = {
  fpsCap: "settings.fpsCap",
  lowGraphics: "settings.lowGraphics",
  lighterRendering: "settings.lighterRendering",
  renderFlags: "performance.graphicsTitle",
  minimizeOnLaunch: "settings.minimize",
  muteOnLaunch: "performance.muteOnLaunch",
  microWidth: "performance.tinyWidth",
  microHeight: "performance.tinyHeight",
  tinyOnLaunch: "performance.tinyOnLaunch",
  memRestartMb: "settings.memRestart",
  memThrottle: "settings.memThrottle",
  trimEveryMin: "settings.trim",
  autoSpread: "settings.spread",
  fairCap: "settings.fairCap",
  adaptive: "settings.adaptive",
};
export const KEYS = Object.keys(LABELS) as ProfileKey[];

export const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** Every setting the profile names is already so. */
export const isApplied = (p: Profile, s: Settings) => KEYS.every((k) => p[k] == null || same(p[k], s[k]));

/** The profile names at least one setting. */
export const named = (p: Profile) => KEYS.some((k) => p[k] != null);

/** The accounts with an FPS cap of their own, in words; empty with none. */
export const ownCaps = (accounts: AccountSummary[]) => fmtList(accounts.filter((a) => a.fpsCap).map((a) => `${accountName(a)} (${a.fpsCap})`));
