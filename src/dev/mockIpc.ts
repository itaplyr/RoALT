// Dev-only fake backend so the UI can be previewed in a normal browser (`npm run dev`).
// main.tsx loads it only when import.meta.env.DEV and there is no Tauri runtime.
// Master password for the fake vault: "password". A pasted cookie containing "bad" is rejected.
// Launching as "AFK 1" fails with an expired cookie; "Builder" launches through a bootstrapper.
// A username containing "nobody" doesn't exist.
// Launched clients start, join and play on timers; one fake Roblox client is already running,
// so turning multi-instance on first asks to close it.
// A bulk launch runs on timers too; "AFK 1" fails in it the same way and the batch goes on.
// Servers: place 1 doesn't exist; Mega Obby (7719004215) sells no private servers; "Builder" may
// not join private servers. Without an account the public list allows 3 requests a minute, as Roblox does.
// "Alt 02"'s first client is kicked 20 s into its game; with auto-rejoin on it is closed and comes back once.
// Each kick is recorded as a disconnect, shown under the ended row in Running clients. "Kill all" ends everything.
// Monitor: each running client's CPU, memory and IO drift over time; "Alt 01"'s memory keeps growing (a leak).
// The bandwidth limit "applies" after a pretend admin prompt; 1234 kbit/s fails as a declined prompt.
// Moving the data folder to a path containing "bad" fails as a folder that already has a RoALT database.
// Anti-AFK (when on) "presses a key" in each opted-in client in a game every 20 s, so the time moves.
// Discord: any discord.com webhook URL is accepted; one whose token contains "deleted" fails the test.
import { emit } from "@tauri-apps/api/event";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type {
  AccountInfo, AccountStatus, AccountSummary, AccountValue, BulkProgress, BulkRequest, BulkStage, Crash, Favourite, GameInfo, GameServer, GameStats, Group, Instance, LimitedItem, PlayerStats, RolimonsItem,
  JoinCandidate, LaunchTarget, MultiInstanceStatus, ParsedInput, Presence, PrivateServer, PrivateServerPage, ServerOrder, ServerPage,
  RobloxClient, RobloxLauncher, Settings, VaultMode,
} from "../api";
import type { ModItem } from "../bindings/ModItem";
import type { ModsLast } from "../bindings/ModsLast";
import type { ModsList } from "../bindings/ModsList";
import type { GroupDefaults } from "../bindings/GroupDefaults";
import type { PlaytimeDay } from "../bindings/PlaytimeDay";
import type { RobuxTransaction } from "../bindings/RobuxTransaction";

const MIN = 60_000;
const DAY = 86_400_000;
const now = Date.now();

const NO_DEFAULTS: GroupDefaults = { fpsCap: null, fullQuality: null, launchArgs: null, discordMuted: null, allowDisconnect: null, client: null };
const groups: Group[] = [
  { id: 1, name: "main", color: "#C6F432", sortOrder: 0, defaults: NO_DEFAULTS },
  // The farm runs capped and quiet unless an account says otherwise.
  { id: 2, name: "farm", color: "#7AB0FF", sortOrder: 1, defaults: { ...NO_DEFAULTS, fpsCap: 30, discordMuted: true } },
  { id: 3, name: "afk", color: "#5FE0C8", sortOrder: 2, defaults: NO_DEFAULTS },
  { id: 4, name: "trade", color: "#FFA05C", sortOrder: 3, defaults: NO_DEFAULTS },
  { id: 5, name: "dev", color: "#B49CFF", sortOrder: 4, defaults: NO_DEFAULTS },
];

const seed: [string, string, string, number, string | null, AccountStatus, number[], boolean][] = [
  ["Main", "Nova", "nova_builds", 2840197733, "#C6F432", "valid", [1], true],
  ["Alt 01", "Nova Alt", "nova_alt01", 4519920471, "#7AB0FF", "valid", [2], false],
  ["Alt 02", "Nova Alt", "nova_alt02", 4519987310, "#7AB0FF", "valid", [2], false],
  ["Trader", "Quiet Market", "quietmarket", 1977304452, "#FFA05C", "challenged", [4], true],
  ["AFK 1", "unit one", "afk_unit_1", 5102947781, "#5FE0C8", "valid", [3], false],
  ["AFK 2", "unit two", "afk_unit_2", 5102951106, "#5FE0C8", "invalid", [3], false],
  ["Builder", "Nova Studio", "studio_nova", 3308812290, "#B49CFF", "valid", [5], false],
  ["", "test acc", "nova_test_acc", 5560021849, null, "valid", [5], false],
  ["Event", "Nova Events", "nova_event", 4870033617, "#FF8FC7", "unknown", [], false],
];

let accounts: AccountSummary[] = seed.map(([alias, displayName, username, userId, color, status, groupIds, favourite], i) => ({
  id: i + 1,
  userId,
  username,
  displayName,
  alias,
  notes: i === 0 ? "Main account. Don't trade from here." : "",
  color,
  favourite,
  sortOrder: i,
  groupIds,
  status,
  statusCheckedAt: status === "unknown" ? null : now - (i + 2) * MIN,
  createdAt: now - (40 - i * 3) * DAY,
  allowDisconnect: false,
  launchArgs: null,
  discordMuted: false,
  fpsCap: i === 1 ? 15 : null,
  fullQuality: false,
  main: i === 0,
  customFlags: {},
  // Alt 02 has limits a client of its was given: its Overrides tab shows them.
  clientLimits: i === 2 ? { limits: { cpuCap: 25, eco: true, trimEveryMin: 15, affinity: null }, antiAfk: false } : null,
  profile: null,
}));

let settings: Settings = {
  theme: "dark", accent: "", look: { base: "dark", colors: {} }, corners: 100, backgroundDim: 60, backgroundMotion: "focused", density: "detailed", autoLockMinutes: 15, lockOnSessionLock: true, clipboardClearSeconds: 30, cacheMinutes: 10,
  multiInstance: true, protectSharedLogin: true, launchDelayMs: 6000, autoRejoin: false, autoRestartOnCrash: false,
  breakerMax: 3, breakerWindowMin: 10, closeGraceS: 5, launchArgs: "", closeToTray: false,
  memRestartMb: 0, memThrottle: { mb: 0, cpuCap: 10 }, trimEveryMin: 0, autoSpread: false, fairCap: false,
  adaptive: { enabled: false, background: { cpuCap: 25, eco: true, trimEveryMin: null, affinity: null } }, bandwidthKbps: 0,
  antiAfk: { enabled: false, intervalMin: 15 },
  discord: { crash: true, hang: true, memRestart: true, disconnect: true, restart: true, breaker: true, batchDone: true, launch: false, signIn: true, throttle: false, update: true, pc: false, statusEveryMin: 0, ping: "", pingLevel: "urgent" },
  autoUpdate: false, selfUpdate: true, tabLayout: "top", discordPresence: true, discordRpc: { showGame: true, showCount: true, showElapsed: true, showButtons: true, customDetails: "", customState: "" }, onlineCount: true, news: true, restartEveryH: 0, closeAfterH: 0, showSponsor: true, signInWatch: true, toggleHotkey: "", tileHotkey: "", minimizeHotkey: "", minimizeOnLaunch: false, microWidth: 320, microHeight: 240, headlessWidth: 640, headlessHeight: 360, tinyOnLaunch: false, panicHotkey: "", cleanLogs: false, fpsCap: 0, lowGraphics: true, skipSplash: false, protectMain: true, muteOnLaunch: false, windowLayout: "off", lighterRendering: true, renderFlags: { msaa: "x1", texture: "lowest", mode: "default", graySky: false, noGrass: true, lowestFrm: true, pauseVoxelizer: false },
  askBrowserLinks: false, customFlags: {}, customFlagsScope: "global", lockPurchases: true, rolimonsValues: false,
  allowPublicNetworks: true, language: "", headless: false, keepRunningOnClose: false, modsOn: true, modsAck: false, onboarded: false,
};

// Mods: two packs, the top one on; how the last launch went (null: none since start).
let mods: ModItem[] = [
  { id: "6f1c2a4e-0d3b-4c8a-9e7f-1a2b3c4d5e6f", name: "Night Sky", author: "skyrunner_k", version: "1.2", description: "A starry sky and a dark moon.", on: true, files: 8, bytes: 2_621_440 },
  { id: "8a9b0c1d-2e3f-4a5b-8c7d-9e0f1a2b3c4d", name: "Classic Oof", author: "", version: "", description: "", on: false, files: 1, bytes: 18_432 },
];
let modsLast: ModsLast | null = null;

// Outdated, so Settings offers the update and the Launch view warns before a batch.
let robloxClient: RobloxClient = {
  path: "C:\\Users\\you\\AppData\\Local\\Roblox\\Versions\\version-5e1f0c9a7d3b4e21\\RobloxPlayerBeta.exe",
  bootstrapper: false,
  version: "version-5e1f0c9a7d3b4e21",
  latest: "version-9ac8595712b447a7",
  outdated: true,
};
/** Who opens Roblox's links: Bloxstrap now, so the client card shows it as the bootstrapper. */
let launchers: RobloxLauncher[] = [
  { kind: "roblox", name: "Roblox", path: robloxClient.path ?? "", current: false },
  { kind: "bootstrapper", name: "Bloxstrap", path: "C:\\Users\\you\\AppData\\Local\\Bloxstrap\\Bloxstrap.exe", current: true },
  { kind: "bootstrapper", name: "Fishstrap", path: "C:\\Users\\you\\AppData\\Local\\Fishstrap\\Fishstrap.exe", current: false },
  { kind: "roalt", name: "RoALT", path: "C:\\Users\\you\\AppData\\Local\\Programs\\RoALT\\RoALT.exe", current: false },
];
/** RoALT's self-update: the background check "finds" the next minor version 10 s after start. */
const APP_VERSION: string = import.meta.env.APP_VERSION;
const NEXT_VERSION = APP_VERSION.replace(/^(\d+)\.(\d+)\..*$/, (_, a, b) => `${a}.${Number(b) + 1}.0`);
/** The website's demo (`npm run build:demo`) never invents a RoALT release or usage numbers visitors could believe. */
const DEMO = import.meta.env.MODE === "demo";
let appUpdate: string | null = null;
setTimeout(() => {
  if (DEMO || !settings.selfUpdate) return;
  appUpdate = NEXT_VERSION;
  void emit("app://update", { current: APP_VERSION, available: appUpdate });
}, 10_000);
/** The fake vault's Discord webhook (the real one is encrypted; the UI only ever sees its id's end). */
/** `roblox_install` runs: launches and a second install are refused, as in Rust. */
let installing = false;
/** Closes the mock sign-in window under way. */
let cancelLogin: (() => void) | null = null;
let shutdownAt: number | null = null;
let background = new ArrayBuffer(0);
const INSTALLING = "Roblox is being installed. Launch again once it is done.";
let webhook: { id: string; token: string } | null = null;
const WEBHOOK = /^https:\/\/(?:(?:ptb|canary)\.)?discord(?:app)?\.com\/api\/webhooks\/(\d{1,25})\/([A-Za-z0-9_-]{1,128})$/;
const discordStatus = () => ({ configured: webhook !== null, webhook: webhook && `…${webhook.id.slice(-4)}` });
let bandwidthActive = false;
/** Fake metrics: a wave per client, so the sparklines move. */
function fakeMetrics() {
  const t = Date.now() / 1000;
  return instances.filter((i) => i.state !== "exited").map((i, n) => {
    const age = (Date.now() - i.startedAt) / 1000;
    const leak = i.accountId === 2 ? age * 4 : 0;
    const wave = (p: number) => (Math.sin(t / p + n * 1.7) + 1) / 2;
    const capped = i.limits.cpuCap ?? 100;
    return {
      instanceId: i.id, pid: i.pid,
      cpuPct: age < 2 ? null : Math.min(capped, 2 + 9 * wave(7) + 3 * Math.random()) * (i.limits.eco ? 0.7 : 1),
      memoryMb: 600 + 250 * wave(23) + leak * 0.8,
      privateMb: 900 + 120 * wave(31) + leak,
      readBps: age < 2 ? null : 20_000 + 400_000 * wave(5) * Math.random(),
      writeBps: age < 2 ? null : 5_000 + 60_000 * wave(9),
      gpuPct: age < 2 ? null : 1 + 6 * wave(11),
      gpuMemMb: 190 + 40 * wave(29),
    };
  });
}
/** Clients hidden by Minimize: still drawing, as `window::ghost`. */
const ghosts = new Set<string>();
let thumbSeq = 0;
type FakeShot = { seq: number; w: number; h: number; rgba: Uint8ClampedArray; at: number };
/** Each client's newest picture, kept like `capture::SHOTS`. */
const shots = new Map<string, FakeShot>();
/** To see what the mock doesn't get into by itself, from the console:
 * `mockPreview.set(instanceId, "pending" | "unsupported" | "stale" | "minimized")`. */
const previewForced = new Map<string, "pending" | "unsupported" | "stale" | "minimized">();
(window as unknown as { mockPreview: typeof previewForced }).mockPreview = previewForced;
/** As `window::Preview::to_bytes`: the header, then the pixels unless they are `known`. */
function thumbReply(i: Instance, status: number, known: number, shot?: FakeShot) {
  const pixels = shot && shot.seq !== known ? shot.rgba : null;
  const buf = new ArrayBuffer(24 + (pixels?.length ?? 0));
  const head = new DataView(buf);
  head.setUint32(0, shot?.w ?? 0, true);
  head.setUint32(4, shot?.h ?? 0, true);
  head.setUint32(8, status, true);
  head.setUint32(12, ghosts.has(i.id) ? 1 : 0, true);
  head.setUint32(16, shot?.seq ?? 0, true);
  head.setUint32(20, shot ? Date.now() - shot.at : 0, true);
  if (pixels) new Uint8ClampedArray(buf, 24).set(pixels);
  return buf;
}
/** A fake 4:3 picture of a client's window: a moving sky over a green baseplate. */
function fakeShot(i: Instance, w: number): FakeShot {
  const h = Math.round((w * 3) / 4);
  const px = new Uint8ClampedArray(w * h * 4);
  const shift = (Date.now() / 40 + i.accountId * 60) % w;
  for (let y = 0; y < h; y++)
    for (let x = 0; x < w; x++) {
      const o = (y * w + x) * 4;
      const ground = y > h * 0.62;
      const block = !ground && Math.abs(((x + shift) % w) - w / 2) < w / 14 && y > h * 0.45;
      px[o] = ground ? 70 : block ? 200 : 110 + (y / h) * 80;
      px[o + 1] = ground ? 150 : block ? 60 : 170 + (y / h) * 60;
      px[o + 2] = ground ? 70 : block ? 60 : 240;
      px[o + 3] = 255;
    }
  return { seq: ++thumbSeq, w, h, rgba: px, at: Date.now() };
}
// A few ends from before this session, so Crash history and an account's History have something.
const crashes: Crash[] = [
  { id: 3, instanceId: "seed-3", accountId: 1, kind: "crash", at: now - 50 * MIN, uptimeMs: 14_000, reason: null, placeId: 4410028153, hasLog: true, verdict: "early" },
  { id: 2, instanceId: "seed-2", accountId: 2, kind: "hang", at: now - 5 * 60 * MIN, uptimeMs: 3 * 60 * MIN, reason: null, placeId: 7719004215, hasLog: true, verdict: "hang" },
  { id: 1, instanceId: "seed-1", accountId: 1, kind: "disconnect", at: now - 26 * 60 * MIN, uptimeMs: 95 * MIN, reason: "Sending disconnect with reason: 277", placeId: 4410028153, hasLog: false, verdict: "disconnect" },
];
// Instances: a stranger client blocks multi-instance until "Close Roblox".
let strangerRunning = true;
let lockHeld = false;
let instances: Instance[] = [];
const instancesChanged = () => {
  void emit("instances://changed");
  mirrorBatch();
};
const multiStatus = (): MultiInstanceStatus => {
  const blockedBy = lockHeld ? [] : [...(strangerRunning ? [9120] : []), ...instances.filter((i) => i.state !== "exited").map((i) => i.pid)];
  const message = !settings.multiInstance || lockHeld ? null
    : blockedBy.length ? "Roblox is already running. Close it (including its tray icon) to run several clients."
    : "Multi-instance isn't active yet. Turn it off and on again to retry.";
  return { enabled: settings.multiInstance, active: lockHeld, protectSharedLogin: settings.protectSharedLogin, blockedBy, message };
};
// A client launched without the lock owns Roblox's singleton event; clients started under the
// lock don't block taking it again.
let eventHolder: string | null = null;
const retakeLock = () => {
  const holder = instances.some((i) => i.id === eventHolder && i.state !== "exited");
  if (settings.multiInstance && !lockHeld && !strangerRunning && !holder) lockHeld = true;
};
function endInstance(id: string, note: string) {
  const i = instances.find((x) => x.id === id);
  if (!i || i.state === "exited") return;
  i.state = "exited";
  i.endedAt = Date.now();
  i.note = note;
  instancesChanged();
}
/** The last performance profile "saved" in the preview (performance_export). */
let lastProfile: string | null = null;
const PERF_KEYS = ["fpsCap", "lowGraphics", "lighterRendering", "renderFlags", "minimizeOnLaunch", "muteOnLaunch", "microWidth", "microHeight", "tinyOnLaunch", "memRestartMb", "memThrottle", "trimEveryMin", "autoSpread", "fairCap", "adaptive"] as const;

function perfProfile(previewFps: number) {
  const s: Record<string, unknown> = {};
  for (const k of PERF_KEYS) s[k] = settings[k];
  return JSON.stringify({ format: "roalt-performance", version: 1, exportedAt: Date.now(), settings: { ...s, previewFps } }, null, 2);
}

/** The profile in `text` as performance_parse returns it, or the error message. */
function perfParse(text: string): Record<string, unknown> | string {
  const refused = "This isn't a RoALT performance profile.";
  let file: { format?: unknown; version?: number; settings?: Record<string, unknown> };
  try {
    file = JSON.parse(text);
  } catch {
    return refused;
  }
  if (file?.format !== "roalt-performance") return refused;
  if ((file.version ?? 0) > 1) return "This profile was made by a newer RoALT; update RoALT first.";
  const p = file.settings ?? {};
  const fps = p.fpsCap as number | undefined;
  if (fps && (fps < 5 || fps > 360)) return "The FPS cap must be 0 (off) or 5 to 360.";
  const mem = p.memRestartMb as number | undefined;
  if (mem && mem < 512) return "The memory limit must be 0 (off) or 512 to 65 536 MB.";
  const out: Record<string, unknown> = {};
  for (const k of PERF_KEYS) if (p[k] !== undefined && p[k] !== null) out[k] = p[k];
  if ([5, 15, 30, 60].includes(p.previewFps as number)) out.previewFps = p.previewFps;
  return out;
}

function track(accountId: number, pid: number, target: LaunchTarget, placeId: number | null, viaBootstrapper: boolean, rejoins = 0, fpsCap: number | null = null) {
  // Without the lock a new client replaces the running one, as Roblox does.
  if (!lockHeld) for (const old of instances) endInstance(old.id, "Replaced by another Roblox client");
  const i: Instance = {
    id: crypto.randomUUID(), accountId, pid, state: "starting", target, placeId, jobId: null,
    startedAt: Date.now(), endedAt: null, viaBootstrapper, note: null, rejoins, limits: { cpuCap: null, eco: false, trimEveryMin: null, affinity: null },
    antiAfk: true, afkAt: null, throttled: false, micro: false, fpsCap, relaunching: false, relaunchProblem: null, signInProblem: false, appliedLimits: { cpuCap: 25, eco: true, affinity: null, trimEveryMin: 15 }, held: false,
  };
  instances.push(i);
  if (!lockHeld) eventHolder = i.id;
  instancesChanged();
  const step = (ms: number, f: () => void) => setTimeout(() => { if (i.state !== "exited") { f(); instancesChanged(); } }, ms);
  step(1500, () => { i.state = "joining"; });
  step(4000, () => { i.state = "inGame"; i.jobId = target.kind === "server" ? target.jobId : crypto.randomUUID(); i.placeId ??= 1818; });
  if (accountId === 3 && rejoins === 0) setTimeout(() => kick(i), 24_000);
  return i;
}
/** As instances::rejoin: a disconnected client is closed after the launch delay and the same
 * account launched into the same target again. */
function kick(i: Instance) {
  if (i.state === "exited") return;
  i.state = "disconnected";
  crashes.unshift({
    id: crashes.length + 1, instanceId: i.id, accountId: i.accountId, kind: "disconnect", at: Date.now(), uptimeMs: Date.now() - i.startedAt,
    reason: "Sending disconnect with reason: 267", placeId: i.placeId, hasLog: true, verdict: "disconnect",
  });
  instancesChanged();
  if (!settings.autoRejoin || i.held || find(i.accountId)?.allowDisconnect) return;
  setTimeout(() => {
    const a = find(i.accountId);
    if (i.state !== "disconnected") return;
    const busy = instances.some((x) => x.id !== i.id && x.accountId === i.accountId && x.state !== "exited" && x.state !== "disconnected");
    if (!a || !settings.autoRejoin || (batch && !batch.done) || busy) return;
    endInstance(i.id, "Closed");
    void emit("app://notice", { error: false, message: `${a.alias || a.displayName} was disconnected and is rejoining.` });
    retakeLock();
    track(a.id, 14000 + Math.floor(Math.random() * 9000), i.target, i.placeId, i.viaBootstrapper, i.rejoins + 1, i.fpsCap);
  }, settings.launchDelayMs);
}
// Debug tab: roalt.log as tauri-plugin-log writes it, and a new line every few seconds.
const LEVEL_NAMES = ["", "TRACE", "DEBUG", "INFO", "WARN", "ERROR"];
const fakeLog: string[] = [];
function logLine(level: number, target: string, message: string, live = true) {
  const d = new Date();
  const line = `[${d.toISOString().slice(0, 10)}][${d.toTimeString().slice(0, 8)}][${target}][${LEVEL_NAMES[level]}] ${message}`;
  fakeLog.push(line);
  if (live) emit("log://log", { level, message: line });
}
[
  [3, "roalt", "multi-instance not active at start: another client holds the singleton"],
  [4, "roalt::roblox_api", "users.roblox.com/v1/users/authenticated: HTTP 401, code Some((0, 0)): This cookie is not valid or has expired. Sign in again."],
  [3, "roalt::commands", "account 3 marked Invalid: This cookie is not valid or has expired. Sign in again."],
  [2, "roalt::instances::tracker", "instance 7f3a: client log not readable: The process cannot access the file"],
  [5, "roalt", "panic in thread tokio-runtime-worker at src/instances/tracker.rs:612:9: index out of bounds"],
  [4, "roblox", "account 2 instance 7f3a: [FLog::Error] Texture failed to load (x3)"],
].forEach(([l, t, m]) => logLine(l as number, t as string, m as string, false));
setInterval(() => logLine(1 + Math.ceil(Math.random() * 4), "roalt::mock", "periodic mock line"), 4000);

setInterval(() => {
  if (!settings.antiAfk.enabled) return;
  const due = instances.filter((i) => i.state === "inGame" && i.antiAfk);
  due.forEach((i) => (i.afkAt = Date.now()));
  if (due.length) instancesChanged();
}, 20_000);
// Fake recovery code: "ABC-DEF-234" until a new one is made.
const vault = { mode: "master" as VaultMode, locked: false, password: "password", recovery: "ABC-DEF-234" };
const normalCode = (s: string) => s.replace(/[-\s]/g, "").toUpperCase();
function fakeRecoveryCode() {
  const pick = () => "23456789ABCDEFGHJKMNPQRSTUVWXYZ"[Math.floor(Math.random() * 31)];
  return Array.from({ length: 3 }, () => pick() + pick() + pick()).join("-");
}
const FAKE_COOKIE = "_|WARNING:-DO-NOT-SHARE-THIS.--DEV-PREVIEW-FAKE-VALUE|_" + "0F1E2D3C4B5A69788796A5B4C3D2E1F0".repeat(6);

type Args = Record<string, any>;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const fail = (kind: string, message: string) => Promise.reject({ kind, message });
const MOVE_BUSY = "Wait for the running launch, batch or auto-rejoin to finish, then move the data folder.";
const changed = () => void emit("accounts://changed");
const find = (id: number) => accounts.find((a) => a.id === id);
/** As the backend: what a client was given stays with its account. */
const remember = (i: Instance) => {
  const a = find(i.accountId);
  if (!a) return;
  const plain = i.limits.cpuCap === null && !i.limits.eco && i.limits.trimEveryMin === null && i.limits.affinity === null && i.antiAfk;
  a.clientLimits = plain ? null : { limits: { ...i.limits }, antiAfk: i.antiAfk };
  changed();
};

function addFake(username: string): AccountSummary {
  const id = Math.max(0, ...accounts.map((a) => a.id)) + 1;
  const a: AccountSummary = {
    id, userId: 6000000000 + id, username, displayName: username.replace(/_/g, " "), alias: "", notes: "", color: null,
    favourite: false, sortOrder: accounts.length, groupIds: [], status: "valid", statusCheckedAt: Date.now(), createdAt: Date.now(),
    allowDisconnect: false, launchArgs: null, discordMuted: false, fpsCap: null, fullQuality: false, main: false, customFlags: {}, clientLimits: null, profile: null,
  };
  accounts.push(a);
  changed();
  return a;
}

// ---------- M2: headshots, presence, account info ----------

/** A blocky head on a transparent background, so the account colour shows behind it. */
function headshot(seed: number) {
  const skin = ["#F5CD9B", "#E0A878", "#C68642", "#8D5524", "#FFDBAC"][seed % 5];
  const shirt = ["#2B2F36", "#3A6EA5", "#7A3E9D", "#2E7D5B", "#B23A48"][seed % 7 % 5];
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48"><rect x="8" y="36" width="32" height="12" fill="${shirt}"/><rect x="13" y="9" width="22" height="24" fill="${skin}"/><rect x="18" y="18" width="3" height="4" fill="#111"/><rect x="27" y="18" width="3" height="4" fill="#111"/><rect x="20" y="27" width="8" height="2" fill="#111"/></svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}
const NO_HEADSHOT = 8; // shows the initials fallback

const SKY: Presence = {
  kind: "inGame", lastLocation: "Sky Islands", placeId: 4410028153, rootPlaceId: 4410028153, universeId: 1559201384,
  gameId: "8f3a1c2e-77b0-4c1d-9e52-0b6c1d3fa911",
};
const OBBY: Presence = {
  kind: "inGame", lastLocation: "Mega Obby", placeId: 7719004215, rootPlaceId: 7719004215, universeId: 2650339475,
  gameId: "1c0e9b44-3a2f-4f6e-a1d8-55e0c2b97a03",
};
const plain = (kind: Presence["kind"], lastLocation: string | null = null): Presence =>
  ({ kind, lastLocation, placeId: null, rootPlaceId: null, universeId: null, gameId: null });
const PRESENCE: Record<number, Presence> = { 1: SKY, 2: plain("online", "Website"), 3: SKY, 5: OBBY, 7: plain("studio", "Studio") };
const presenceOf = (a: AccountSummary) => PRESENCE[a.id] ?? plain("offline");

const ABOUT: Record<number, string> = {
  1: [
    "Builder and scripter. Sky Islands team since 2019.",
    "Commissions open for maps, UI and small systems; message me first.",
    "No trades, no group invites, please. Alts: nova_alt01, nova_alt02, nova_event.",
    "If you find a bug in one of my games, the group wall is the fastest way to reach me.",
  ].join("\n"),
  4: "Fair trades only. Check my rap before asking.",
  7: "Studio account for testing places.",
};

const infos = new Map<number, AccountInfo>();

function makeInfo(a: AccountSummary): AccountInfo {
  const i = a.id;
  const r = (n: number) => (a.userId % 9973) * (i + 3) % n;
  // A dead cookie still gets the public parts, like the real backend.
  const missing = a.status === "invalid" ? ["robux", "presence", "email", "2-step"] : [];
  if (i === NO_HEADSHOT) missing.push("avatar");
  const has = (part: string) => !missing.includes(part);
  return {
    accountId: i, userId: a.userId, username: a.username, displayName: a.displayName,
    description: ABOUT[i] ?? "",
    created: new Date(now - (420 + r(2400)) * DAY).toISOString(),
    isBanned: i === 9,
    verifiedBadge: i === 1,
    headshotUrl: has("avatar") ? headshot(a.userId) : null,
    robux: has("robux") ? (i === 1 ? 1_284_500 : r(15000)) : null,
    subscription: i === 1 ? "plus" : i === 4 ? "premium" : "none",
    friends: r(200), followers: r(1500), following: r(90),
    emailVerified: has("email") ? i % 3 !== 2 : null,
    twoStepMethods: has("2-step") ? (i % 4 === 3 ? [] : ["Authenticator", "Email"].slice(0, 1 + (i % 2))) : null,
    presence: has("presence") ? presenceOf(a) : null,
    fetchedAt: Date.now(),
    unavailable: missing,
  };
}

// ---------- M3: target parsing, launch, favourites, join ----------

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const SHARE_TYPES = ["Server", "ExperienceInvite", "ExperienceDetails", "ExperienceAffiliate", "ExperienceEvent", "Profile"];
const idOf = (v?: string) => (v && /^\d{1,15}$/.test(v) && Number(v) > 0 ? Number(v) : null);
const target = (t: LaunchTarget): ParsedInput => ({ kind: "target", target: t });
const bad = (reason: string): ParsedInput => ({ kind: "invalid", reason });
const follow = (v?: string) => (idOf(v) ? target({ kind: "followUser", userId: Number(v) }) : bad("That user ID isn't valid."));

/** A small stand-in for launcher::target::parse (the real rules: src-tauri/src/launcher/target.rs). */
function parseTarget(raw: string): ParsedInput {
  const s = raw.trim().replace(/^<|>$/g, "");
  if (!s) return bad("Paste a game link, an ID or @username.");
  if (idOf(s)) return target({ kind: "place", placeId: Number(s), launchData: null });
  if (UUID.test(s)) return { kind: "jobIdOnly", jobId: s.toLowerCase() };
  if (/^@[A-Za-z0-9_]{3,20}$/.test(s)) return { kind: "username", name: s.slice(1) };
  if (!s.includes(".") && !s.includes("/") && !s.includes(":")) return bad("To search for a game by name, use Browse games. To join a player, put @ before their username.");
  if (/^roblox-(player|studio)/i.test(s)) return bad("That launch link carries a sign-in ticket. Paste the game link instead.");
  let u: URL;
  try {
    u = new URL(/^[a-z][a-z0-9+.-]*:/i.test(s) ? s : `https://${s}`);
  } catch {
    return bad("That doesn't look like a link.");
  }
  // First occurrence of a key wins, as in the real parser.
  const q = new Map([...u.searchParams].reverse().map(([k, v]) => [k.toLowerCase(), v]));
  if (u.protocol === "roalt:" || /^(www\.)?roalt\.dev$/i.test(u.hostname)) {
    const route = (u.protocol === "roalt:" ? `${u.host}${u.pathname}` : u.pathname).replace(/^\/+|\/+$/g, "").toLowerCase();
    if (route !== "play") return { kind: "unsupported", reason: "That link doesn't point to a game." };
    const placeId = idOf(q.get("place"));
    if (!placeId) return bad("That place ID isn't valid.");
    const job = q.get("job");
    if (job === undefined) return target({ kind: "place", placeId, launchData: null });
    return UUID.test(job) ? target({ kind: "server", placeId, jobId: job.toLowerCase() }) : bad("That server ID isn't valid.");
  }
  if (u.protocol === "roblox:") {
    const route = `${u.host}${u.pathname}`.replace(/^\/+|\/+$/g, "").toLowerCase();
    if (route === "" || route === "experiences/start") return fromQuery(q);
    if (route === "navigation/share_links") return share(q);
    if (route === "navigation/profile") return follow(q.get("userid"));
    if (route === "navigation/game_details") {
      const universeId = idOf(q.get("gameid"));
      return universeId ? target({ kind: "universe", universeId }) : bad("That game ID isn't valid.");
    }
    return { kind: "unsupported", reason: "That Roblox app link doesn't open a game." };
  }
  if (!/^(www\.|web\.|m\.)?roblox\.com$/i.test(u.hostname)) return bad("Only roblox.com links work here.");
  const seg = u.pathname.split("/").filter(Boolean);
  if (seg.length > 1 && /^[a-z]{2}(-[a-z0-9]{2,4})?$/i.test(seg[0])) seg.shift();
  const [a = "", b] = seg.map((x) => x.toLowerCase());
  if (a === "games" && b === "start") return fromQuery(q);
  if (a === "games") return idOf(b) ? fromQuery(new Map([...q, ["placeid", b]])) : bad("That place ID isn't valid.");
  if (a === "share" || a === "share-links") return share(q);
  if (a === "users") return follow(b);
  return { kind: "unsupported", reason: "That roblox.com page isn't a game." };
}

function fromQuery(q: Map<string, string>): ParsedInput {
  const placeId = idOf(q.get("placeid") ?? q.get("id"));
  if (!placeId) return q.has("userid") ? follow(q.get("userid")) : bad("That place ID isn't valid.");
  const link = q.get("linkcode") ?? q.get("privateserverlinkcode");
  if (link !== undefined) return /^\d{1,64}$/.test(link) ? target({ kind: "privateLink", placeId, linkCode: link }) : bad("That private server link code isn't valid.");
  const access = q.get("accesscode");
  if (access !== undefined) return UUID.test(access) ? target({ kind: "privateAccess", placeId, accessCode: access.toLowerCase() }) : bad("That access code isn't valid.");
  const job = q.get("gameinstanceid");
  if (job !== undefined) return UUID.test(job) ? target({ kind: "server", placeId, jobId: job.toLowerCase() }) : bad("That job ID isn't valid.");
  return target({ kind: "place", placeId, launchData: q.get("launchdata") || null });
}

function share(q: Map<string, string>): ParsedInput {
  const code = q.get("code") ?? "";
  if (!/^[A-Za-z0-9_-]{1,128}$/.test(code)) return bad("That share link's code isn't valid.");
  if (q.get("v")?.toLowerCase() === "v2") return target({ kind: "shareLinkV2", code });
  if (!q.has("type")) return bad("That share link is missing its type.");
  const type = SHARE_TYPES.find((t) => t.toLowerCase() === q.get("type")?.toLowerCase());
  return type ? target({ kind: "shareLink", code, linkType: type }) : { kind: "unsupported", reason: "That share link isn't for a game." };
}

const FRIENDS: JoinCandidate[] = [
  { userId: 1203994411, username: "skyrunner_k", displayName: "Kai", accountId: null, presence: SKY },
  // In a game, but Roblox hides which server: picking this one follows.
  { userId: 998120347, username: "mossy_bricks", displayName: "Moss", accountId: null, presence: { ...OBBY, placeId: null, gameId: null } },
  { userId: 3001772200, username: "lunatrades", displayName: "Luna", accountId: null, presence: plain("online", "Website") },
];

const favSeed: [string, LaunchTarget, number[]][] = [
  ["Sky Islands", { kind: "place", placeId: 4410028153, launchData: null }, [1, 2, 3]],
  ["Sky Islands · VIP", { kind: "shareLink", code: "7c1f0e9a2b4d4e61", linkType: "Server" }, [1]],
  ["Mega Obby", { kind: "place", placeId: 7719004215, launchData: null }, [5]],
  ["Follow @nova_builds", { kind: "followUser", userId: 2840197733 }, [2]],
];
let favourites: Favourite[] = favSeed.map(([name, t, accountIds], i) => ({
  id: i + 1, name, target: t, accountIds, inTray: i === 0, sortOrder: i, options: { sameServer: i === 0 ? "randomPublic" : null, fpsCap: i === 0 ? 30 : null, groupIds: [], hotkey: "" },
}));

const placeOfTarget = (t: LaunchTarget) => ("placeId" in t ? t.placeId : t.kind === "shareLink" ? SKY.placeId : null);

// ---------- M5: bulk launch, servers ----------

const EXPIRED = "This cookie is not valid or has expired. Sign in again.";
let batch: BulkProgress | null = null;
let batchCancel = false;
function bulkChanged() {
  if (batch) void emit("bulk://progress", structuredClone(batch));
}
/** Started clients' stages follow their instances, as in launcher::bulk::stage_of. */
function mirrorBatch() {
  if (!batch || batch.done) return;
  let moved = false;
  for (const item of batch.items) {
    const i = instances.find((x) => x.id === item.instanceId);
    if (!i || item.stage.kind === "failed" || item.stage.kind === "cancelled") continue;
    const stage: BulkStage = i.state === "disconnected" && find(i.accountId)?.allowDisconnect
      ? { kind: "done" }
      : i.state === "disconnected" || i.state === "exited"
      ? { kind: "failed", message: i.note ?? (i.state === "exited" ? "Closed" : "Disconnected") }
      : { kind: i.state };
    if (stage.kind !== item.stage.kind) {
      item.stage = stage;
      moved = true;
    }
  }
  if (moved) bulkChanged();
}

/** A stable list of public servers per place. */
function fakeServers(placeId: number): GameServer[] {
  let seed = placeId % 2147483647 || 1;
  const rnd = () => (seed = (seed * 48271) % 2147483647) / 2147483647;
  const hex = (n: number) => Array.from({ length: n }, () => Math.floor(rnd() * 16).toString(16)).join("");
  return Array.from({ length: 140 }, () => ({
    id: `${hex(8)}-${hex(4)}-4${hex(3)}-a${hex(3)}-${hex(12)}`,
    playing: Math.floor(rnd() * 13), maxPlayers: 12, fps: Math.round((55 + rnd() * 5) * 10) / 10, ping: 30 + Math.floor(rnd() * 150), friendCount: null,
  }));
}
function serverPage(placeId: number, order: ServerOrder, excludeFull: boolean, limit: number, cursor?: string): ServerPage {
  const by: Record<ServerOrder, (a: GameServer, b: GameServer) => number> = {
    occupancyDesc: (a, b) => b.playing - a.playing,
    occupancyAsc: (a, b) => a.playing - b.playing,
    recommended: (a, b) => (a.ping ?? 0) - (b.ping ?? 0) + (b.playing - a.playing) * 10,
    bestLatency: (a, b) => (a.ping ?? 0) - (b.ping ?? 0),
  };
  const list = fakeServers(placeId).filter((s) => !excludeFull || s.playing < s.maxPlayers).sort(by[order]);
  const from = Number(cursor) || 0;
  // Pages overlap by 3, as Roblox's repeat servers.
  return { servers: list.slice(from, from + limit), nextCursor: from + limit < list.length ? String(from + limit - 3) : null };
}

// ---------- M6: server browser ----------

const SIGNED_IN_ONLY = "Pick an account to sort by Recommended or Best latency: Roblox only does that for a signed-in account.";
const ISLAND = `data:image/svg+xml,${encodeURIComponent(
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" fill="#7AB0FF"/><rect x="10" y="34" width="44" height="10" fill="#2E7D5B"/><rect x="18" y="44" width="28" height="8" fill="#8D5524"/><rect x="40" y="12" width="10" height="10" fill="#FFDBAC"/></svg>`,
)}`;
const GAMES: Record<number, [name: string, creator: string, universeId: number]> = {
  4410028153: ["Sky Islands", "Sky Islands Team", 1559201384],
  7719004215: ["Mega Obby", "ObbyWorks", 2650339475],
};
function fakeGame(placeId: number): GameInfo {
  const [name, creator, universeId] = GAMES[placeId] ?? [`Test Place ${placeId}`, "RoALT Dev", 1_000_000 + (placeId % 9_000_000)];
  return {
    placeId, universeId, rootPlaceId: placeId, name, creator, maxPlayers: 12,
    playing: fakeServers(placeId).reduce((n, s) => n + s.playing, 0), visits: (placeId % 997) * 1_000_003,
    iconUrl: placeId === SKY.placeId ? ISLAND : null, privateServers: placeId !== OBBY.placeId,
  };
}
/** A game's details: six passes (one off sale), twelve badges (the last never won); Mega Obby's
 * badge list is "cut short". */
function fakeStats(universeId: number): GameStats {
  const n = universeId % 97;
  const passNames = ["VIP", "Double Coins", "Radio", "Pet Slot", "Speed Coil", "Old Bundle"];
  const prices = [480, 299, 99, 150, 75, null];
  const badgeNames = ["Welcome", "First Island", "Sky High", "Collector", "Speedrunner", "Night Owl", "Builder", "Trader", "Champion", "Legend", "Mythic", "Impossible"];
  return {
    universeId, playing: 12_000 + n * 140, visits: 4_535_560_984 - n * 1_000_003, favourites: 2_985_238 - n * 1_001, maxPlayers: 12,
    created: "2019-07-14T19:26:21.347Z", updated: new Date(Date.now() - 2 * 86_400_000).toISOString(), genre: "RPG", subgenre: n % 2 ? "Action RPG" : null,
    upVotes: 96_236 - n * 10, downVotes: 15_161 + n * 7,
    passes: passNames.map((name, i) => ({ id: universeId * 10 + i, name, price: prices[i], iconUrl: i === 0 ? ISLAND : null })),
    badges: badgeNames.map((name, i) => ({ id: universeId * 100 + i, name, awarded: Math.round(5_000_000 / (i + 1) ** 2), awardedYesterday: Math.round(9_000 / (i + 1)), winRate: i === 11 ? 0 : 0.8 / (i + 1) ** 2.4, iconUrl: i === 0 ? ISLAND : null })),
    morePasses: false, moreBadges: universeId === GAMES[7719004215][2],
    ageRating: { label: n % 2 ? "Mild" : "Minimal", minimumAge: n % 2 ? 9 : null, descriptors: n % 2 ? ["Violence (Repeated/Mild)", "Fear (Occasional/Mild)"] : [] },
    avatarType: n % 3 ? "MorphToR15" : "PlayerChoice", copying: false, places: 1 + (n % 15),
    servers: Math.ceil((12_000 + n * 140) / 12), groupMembers: n % 2 ? 19_677_072 - n * 1_000 : null, archived: universeId === GAMES[7719004215][2],
  };
}

/** A player's profile: user 2 hides their inventory; any other id is a made-up player. */
function fakePlayer(userId: number): PlayerStats {
  const n = userId % 97;
  return {
    userId, name: userId === 156 ? "builderman" : `Player${userId}`, displayName: userId === 156 ? "builderman" : `Player ${userId}`,
    verified: userId === 156, banned: false, created: "2006-03-08T17:17:52.900Z", headshotUrl: ISLAND,
    friends: 120 + n, followers: 69_896_436 - n, following: 3 + n,
    presence: { kind: n % 3 === 0 ? 2 : n % 3, place: n % 3 === 0 ? "Sky Islands" : null },
    groups: [
      { id: 8, name: "Builders Club", role: "Owner", members: 1_377_559 },
      { id: 3829, name: "Staff", role: "ROBLOX Staff", members: 302_968 },
    ],
    badges: [
      { id: 1, name: "Administrator", imageUrl: null },
      { id: 12, name: "Veteran", imageUrl: null },
    ],
    games: n % 2 ? [] : [{ universeId: 1559201384, placeId: 4410028153, name: "Sky Islands", visits: 24_854 }],
    moreGames: false,
  };
}

// ---------- Limiteds ----------

const LIMITEDS: [assetId: number, name: string, rap: number][] = [
  [1029025, "The Classic ROBLOX Fedora", 431_023], [1365767, "Valkyrie Helm", 263_297], [1031429, "Domino Crown", 6_120_000],
  [20573078, "Shaggy", 1_438], [1125510, "The Void Star", 980_000], [19027209, "Perfectly Legitimate Business Hat", 42_100],
  [48474313, "Red Sparkle Time Fedora", 395_000], [1158416, "Sparkle Time Fedora", 22_500], [16630147, "Bluesteel Bathysphere", 7_800],
  [1081300, "Subspace Tripmine", 1_250], [4390891467, "Korblox Deathspeaker", 15_400], [11748356, "Clockwork's Headphones", 52_300],
];
const ROLI_SAYS: Record<number, Omit<RolimonsItem, "name" | "rap">> = {
  1029025: { value: 450_000, demand: 4, trend: 2, projected: false, hyped: false, rare: true, acronym: "CF" },
  1365767: { value: null, demand: 2, trend: 1, projected: true, hyped: false, rare: false, acronym: "Valk" },
  1031429: { value: 6_500_000, demand: 3, trend: 3, projected: false, hyped: true, rare: true, acronym: "DC" },
  1125510: { value: 1_100_000, demand: 1, trend: 0, projected: false, hyped: false, rare: false, acronym: null },
};
/** Rolimons' list (`market_items`): every limited above, valued or not. */
const ROLI: Record<number, RolimonsItem> = Object.fromEntries(
  LIMITEDS.map(([id, name, rap]) => [id, { name, rap, ...(ROLI_SAYS[id] ?? { value: null, demand: null, trend: null, projected: false, hyped: false, rare: false, acronym: null }) }]),
);
/** What each account holds as of its last count (`account_value`). */
const holdings = new Map<number, { fetchedAt: number; items: { assetId: number; name: string; copies: number; rap: number }[] }>();
function holdingsOf(a: AccountSummary) {
  // Main holds a lot (some twice), Trader nothing, the others a few.
  const picked = a.id === 1 ? LIMITEDS : a.alias === "Trader" ? [] : LIMITEDS.filter((_, i) => (i + a.id) % 4 === 0);
  return picked.map(([assetId, name, rap], i) => ({ assetId, name, rap, copies: a.id === 1 && i < 2 ? 2 : 1 }));
}
function valueOf(h: { fetchedAt: number; items: { assetId: number; name: string; copies: number; rap: number }[] }): AccountValue {
  const roli = settings.rolimonsValues;
  const items: LimitedItem[] = h.items.map((i) => ({ ...i, rolimons: roli ? (ROLI[i.assetId] ?? null) : null }));
  const worth = (i: LimitedItem) => i.rolimons?.value ?? i.rap;
  const top = [...items].sort((x, y) => worth(y) - worth(x)).slice(0, 10);
  return {
    count: items.reduce((n, i) => n + i.copies, 0), rap: items.reduce((n, i) => n + i.rap * i.copies, 0),
    value: roli ? items.reduce((n, i) => n + worth(i) * i.copies, 0) : null, top, partial: false, fetchedAt: h.fetchedAt,
  };
}

let guest = { minute: 0, calls: 0 };
/** Roblox's guest limit on the server list: 3 a minute, on a window aligned to the clock minute. */
function guestLimited() {
  const minute = Math.floor(Date.now() / MIN);
  guest = guest.minute === minute ? { minute, calls: guest.calls + 1 } : { minute, calls: 1 };
  return guest.calls > 3 ? Math.ceil(((minute + 1) * MIN - Date.now()) / 1000) : 0;
}
const robuxLine = (what: string, who: string, amount: number, hoursAgo: number, pending = false): RobuxTransaction => ({
  created: new Date(now - hoursAgo * 60 * MIN).toISOString(), amount, what, who, pending,
});
/** A month of days for two accounts, with a few days off. */
const playtimeDays = (): PlaytimeDay[] =>
  Array.from({ length: 30 }, (_, back) => {
    const d = new Date(now - back * DAY);
    const day = d.getFullYear() * 10_000 + (d.getMonth() + 1) * 100 + d.getDate();
    return back % 6 === 5 ? [] : [{ accountId: 1, day, ms: (40 + ((back * 37) % 150)) * MIN }, ...(back % 2 ? [{ accountId: 2, day, ms: (15 + ((back * 23) % 80)) * MIN }] : [])];
  }).flat();
function privatePage(placeId: number, a: AccountSummary): PrivateServerPage {
  const s = (vipServerId: number, name: string, ownerName: string | null, playing: number, maxPlayers: number, joinable: boolean): PrivateServer => {
    const ownerId = accounts.find((x) => x.username === ownerName)?.userId ?? (ownerName ? 1_000_000 + vipServerId : null);
    return {
      vipServerId, name, ownerId, ownerName, playing, maxPlayers,
      jobId: playing ? crypto.randomUUID() : null, accessCode: joinable ? crypto.randomUUID() : null,
    };
  };
  const servers = placeId === OBBY.placeId ? [] : [
    s(70011, "Nova's VIP", "nova_builds", 3, 12, true),
    s(70012, "Trade hub", "lunatrades", 0, 12, true),
    s(70013, "Kai's friends only", "skyrunner_k", 11, 12, false),
    s(70014, "", null, 2, 8, false),
  ];
  return { servers, nextCursor: null, joinRestricted: a.username === "studio_nova" };
}

async function runBatch(b: BulkProgress, fpsCap: number | null) {
  const until = async (ok: () => boolean, ms: number) => {
    for (const end = Date.now() + ms; Date.now() < end && !ok(); ) await sleep(200);
  };
  let target = b.target;
  let follow = b.sameServer === "followFirst" && target.kind === "place";
  if (b.sameServer === "pickFirst" && target.kind === "place") {
    await sleep(600);
    const fits = fakeServers(target.placeId).filter((s) => s.playing + b.items.length < s.maxPlayers).sort((x, y) => y.playing - x.playing)[0];
    if (fits) {
      b.jobId = fits.id;
      target = { kind: "server", placeId: target.placeId, jobId: fits.id };
    } else {
      follow = true;
      b.note = "No server had room for everyone, so the rest follow the first account.";
    }
  }
  // Spreading: each account to a different server with a spare seat.
  const spread = !["off", "pickFirst", "followFirst"].includes(b.sameServer) && target.kind === "place"
    ? fakeServers(target.placeId).filter((s) => s.playing + 2 <= s.maxPlayers).map((s) => s.id)
    : null;
  let prev: Instance | null = null;
  for (const item of b.items) {
    if (prev) {
      const p: Instance = prev;
      await until(() => p.state !== "starting" || batchCancel, 30_000);
      await until(() => batchCancel, b.delayMs - 1000 + Math.random() * 2000);
    }
    if (batchCancel) break;
    item.stage = { kind: "ticket" };
    bulkChanged();
    await sleep(800);
    const a = find(item.accountId);
    if (!a || a.username === "afk_unit_1") {
      if (a) {
        a.status = "invalid";
        changed();
      }
      item.stage = { kind: "failed", message: EXPIRED };
      bulkChanged();
      continue;
    }
    retakeLock();
    const job = spread?.shift();
    const own: LaunchTarget = job && target.kind === "place" ? { kind: "server", placeId: target.placeId, jobId: job } : target;
    const i = track(a.id, 14000 + Math.floor(Math.random() * 9000), own, placeOfTarget(own), a.username === "studio_nova", 0, fpsCap);
    item.instanceId = i.id;
    item.stage = { kind: "starting" };
    bulkChanged();
    if (!prev && follow && target.kind === "place") {
      await until(() => batchCancel || i.jobId !== null || i.state === "exited", 10_000);
      if (batchCancel) break;
      if (i.jobId) {
        b.jobId = i.jobId;
        target = { kind: "server", placeId: target.placeId, jobId: i.jobId };
      } else b.note = "Couldn't read the first account's server, so the rest join the game as given.";
    }
    prev = i;
  }
  // As the backend: a stopped batch is done at once; otherwise it waits for its clients to settle.
  const settling = (s: BulkStage) => s.kind === "ticket" || s.kind === "starting" || s.kind === "joining";
  if (!batchCancel) await until(() => batchCancel || !b.items.some((x) => settling(x.stage)), 120_000);
  if (batchCancel) {
    for (const item of b.items) if (item.stage.kind === "queued" || item.stage.kind === "ticket") item.stage = { kind: "cancelled" };
    b.stopped = "Cancelled";
  }
  b.done = true;
  bulkChanged();
}

function startBatch(r: BulkRequest) {
  const ids = r.accountIds;
  if (ids.length === 0) return fail("invalid", "Pick at least one account.");
  if (new Set(ids).size !== ids.length) return fail("invalid", "An account is in the list twice.");
  for (const id of ids) {
    const a = find(id);
    if (!a) return fail("notFound", "Not found.");
    if (a.status !== "valid") return fail("invalid", `${a.alias || a.displayName} can't launch: sign in to it again.`);
    const live = instances.some((i) => i.accountId === id && (i.state === "starting" || i.state === "joining" || i.state === "inGame"));
    if (live) return fail("invalid", `${a.alias || a.displayName} is already running. Untick it or close its client first.`);
  }
  if (batch && !batch.done) return fail("invalid", "A batch is already running.");
  retakeLock();
  if (ids.length > 1 && !lockHeld) return fail("invalid", multiStatus().message ?? "Turn on multi-instance to launch several accounts.");
  batchCancel = false;
  batch = {
    batchId: crypto.randomUUID(), target: r.target, sameServer: r.sameServer, delayMs: Math.min(60_000, Math.max(2000, r.delayMs)),
    jobId: null, items: ids.map((accountId) => ({ accountId, userId: find(accountId)?.userId ?? 0, stage: { kind: "queued" }, instanceId: null })), stopped: null, done: false, note: null,
  };
  bulkChanged();
  void runBatch(batch, r.fpsCap || null);
  return Promise.resolve(batch.batchId);
}

function reauth(password?: string) {
  if (vault.mode === "master" && password !== vault.password) return fail("wrongPassword", "Wrong password.");
  return Promise.resolve();
}

async function handle(cmd: string, args: Args): Promise<unknown> {
  switch (cmd) {
    case "vault_status":
      return { mode: vault.mode, locked: vault.locked, accounts: accounts.length, autoLockMinutes: settings.autoLockMinutes, hasRecovery: !!vault.recovery };
    case "vault_unlock":
      await sleep(450);
      if (args.password !== vault.password) return fail("wrongPassword", "Wrong password.");
      vault.locked = false;
      return null;
    case "vault_lock":
      if (vault.mode === "master") vault.locked = true;
      void emit("vault://locked");
      return null;
    case "vault_set_mode": {
      await sleep(400);
      if (vault.mode === "master" && args.currentPassword !== vault.password) return fail("wrongPassword", "Wrong password.");
      if (args.mode === "master") {
        if (!args.newPassword || args.newPassword.length < 8) return fail("invalid", "Use at least 8 characters.");
        vault.password = args.newPassword;
      } else vault.recovery = "";
      vault.mode = args.mode;
      return null;
    }
    case "vault_recovery_create":
      await sleep(800);
      if (args.currentPassword !== vault.password) return fail("wrongPassword", "Wrong password.");
      return (vault.recovery = fakeRecoveryCode());
    case "vault_recover": {
      await sleep(1200);
      if (!vault.recovery || normalCode(args.code) !== normalCode(vault.recovery)) return fail("wrongPassword", "Wrong password.");
      if (args.newPassword.length < 8) return fail("invalid", "Use at least 8 characters.");
      vault.password = args.newPassword;
      vault.locked = false;
      return (vault.recovery = fakeRecoveryCode());
    }
    case "vault_recovery_save":
    case "vault_recovery_copy":
      if (normalCode(args.code).length !== 9) return fail("invalid", "That isn't a recovery code.");
      return null;
    case "app_activity":
      return null;
    case "splash_done":
      return null;

    case "accounts_list":
      return structuredClone(accounts);
    case "account_add_cookie":
      await sleep(700);
      if (/bad/i.test(args.cookie)) return fail("cookieInvalid", "This cookie is not valid or has expired. Sign in again.");
      return addFake(`pasted_${accounts.length + 1}`);
    case "accounts_import_cookies": {
      const found = [...new Set(String(args.text).match(/_\|WARNING:-DO-NOT-SHARE-THIS\.[^\s"';,:]*/g) ?? [])];
      if (!found.length) return fail("invalid", "No .ROBLOSECURITY cookies found. Each starts with _|WARNING:-DO-NOT-SHARE-THIS.");
      await sleep(400 * found.length);
      const bad = found.filter((c) => /bad/i.test(c)).length;
      found.slice(bad).forEach(() => addFake(`imported_${accounts.length + 1}`));
      return { added: found.length - bad, updated: 0, skipped: 0, failed: bad, settings: null, failures: bad ? ["Line 1: This cookie is not valid or has expired. Sign in again."] : [] };
    }
    case "accounts_import_ram":
      await sleep(1200);
      ["ram_alt_1", "ram_alt_2", "ram_alt_3"].forEach(addFake);
      return { added: 3, updated: 1, skipped: 0, failed: 1, settings: null, failures: ["ram_alt_4: This cookie is not valid or has expired. Sign in again."] };
    case "account_add_login": {
      // As the sign-in window: the user signs in, or closes it (account_login_cancel does too).
      const closed = await new Promise<boolean>((done) => {
        cancelLogin = () => done(true);
        setTimeout(() => done(false), args.username ? 2500 : 3500);
      });
      cancelLogin = null;
      if (closed) return fail("cancelled", "Cancelled.");
      if (/^bad/i.test(args.username ?? "")) return fail("invalid", "Roblox refused this sign-in.");
      return addFake(args.username ?? `fresh_login_${accounts.length + 1}`);
    }
    case "account_login_cancel":
      cancelLogin?.();
      return null;
    case "accounts_robux":
      return accounts.filter((_, i) => i % 3 !== 2).map((a, i) => [a.id, [1840, 12, 0, 25_300, 96, 410][i % 6]]);
    case "account_update": {
      const a = find(args.id);
      if (!a) return fail("notFound", "Not found.");
      const p = args.patch;
      if (/["'&|<>^]/.test(p.launchArgs ?? "")) return fail("invalid", "Launch arguments can't contain that character.");
      if (p.fpsCap && p.fpsCap < 5) return fail("invalid", "The FPS cap must be 0 (off) or 5 to 360.");
      const badFlag = Object.keys(p.customFlags ?? {}).find((k) => !/^\w{1,128}$/.test(k));
      if (badFlag !== undefined) return fail("invalid", `"${badFlag}" isn't an FFlag name: use 1 to 128 letters, digits or _.`);
      if (p.main) for (const o of accounts) o.main = false;
      const { clearClientLimits, ...rest } = p;
      Object.assign(a, rest, p.color === undefined ? {} : { color: p.color || null }, p.launchArgs === undefined ? {} : { launchArgs: p.launchArgs.trim() || null }, p.fpsCap === undefined ? {} : { fpsCap: p.fpsCap || null }, clearClientLimits ? { clientLimits: null } : {}, p.profile === undefined ? {} : { profile: Object.keys(p.profile).length ? p.profile : null });
      changed();
      return structuredClone(a);
    }
    case "accounts_reorder":
      (args.ids as number[]).forEach((id, i) => {
        const a = find(id);
        if (a) a.sortOrder = i;
      });
      changed();
      return null;
    case "accounts_remove":
      accounts = accounts.filter((a) => !args.ids.includes(a.id));
      changed();
      return null;
    case "accounts_validate": {
      const ids: number[] = args.ids ?? accounts.map((a) => a.id);
      for (const id of ids) {
        await sleep(250);
        const a = find(id);
        if (a && a.status === "unknown") a.status = "valid";
        if (a) a.statusCheckedAt = Date.now();
      }
      changed();
      return structuredClone(accounts);
    }
    case "accounts_befriend": {
      const n = accounts.filter((a) => args.ids.includes(a.id) && a.status === "valid").length;
      await sleep(400);
      return { added: (n * (n - 1)) / 2, already: 0, failed: 0, challenged: [] };
    }
    case "account_reveal_cookie":
      await reauth(args.password);
      return FAKE_COOKIE;
    case "account_copy_cookie":
      await reauth(args.password);
      return null;
    case "accounts_export_cookies": {
      await reauth(args.password);
      const n = accounts.filter((a) => !args.accountIds || args.accountIds.includes(a.id)).length;
      await sleep(300);
      return { path: "C:\\Users\\you\\AppData\\Roaming\\RoALT\\exports\\RoALT-cookies-20261104-211500.txt", count: n, skipped: 0, deletesAt: Date.now() + 600_000 };
    }
    case "cookie_export_delete":
      return 1;
    case "cookie_export_reveal":
      return null;

    case "account_info": {
      const a = find(args.id);
      if (!a) return fail("notFound", "Not found.");
      await sleep(args.refresh ? 900 : 600);
      if (args.refresh || !infos.has(a.id)) infos.set(a.id, makeInfo(a));
      return structuredClone(infos.get(a.id));
    }
    case "accounts_headshots":
      await sleep(300);
      return accounts.filter((a) => a.id !== NO_HEADSHOT).map((a) => ({ accountId: a.id, url: headshot(a.userId) }));
    case "accounts_presence":
      await sleep(250);
      return accounts
        .filter((a) => (args.ids as number[]).includes(a.id) && a.status === "valid")
        .map((a) => ({ accountId: a.id, presence: presenceOf(a) }));
    // No native page in a browser preview: Browse's pane stays empty.
    case "browse_open":
      await sleep(300);
      if (!find(args.accountId)) return fail("notFound", "Not found.");
      // As the page reports itself once loaded (Browse's toolbar shows Details on a game's page).
      void emit("browse://page", { accountId: args.accountId, path: args.path });
      return null;
    case "browse_bounds":
    case "browse_nav":
    case "browse_close":
      return null;

    case "target_parse":
      return parseTarget(String(args.input ?? ""));
    case "market_items":
      await sleep(300);
      return settings.rolimonsValues ? { fetchedAt: Date.now() - 4 * MIN, items: Object.entries(ROLI).map(([id, r]) => [Number(id), r]) } : null;
    case "mods_list":
      await sleep(150);
      return { mods, last: modsLast } satisfies ModsList;
    case "mods_import": {
      await sleep(700);
      const name = String(args.path).split(/[\\/]/).pop()!.replace(/\.zip$/i, "") || "Mod";
      mods = [{ id: crypto.randomUUID(), name, author: "", version: "", description: "", on: true, files: 23, bytes: 4_194_304 }, ...mods];
      return { name, files: 23, skipped: 2 };
    }
    case "mods_export":
      await sleep(400);
      return null;
    case "mods_export_name":
      return `${mods.find((m) => m.id === args.id)?.name ?? "mod"}.zip`;
    case "mods_remove":
      mods = mods.filter((m) => m.id !== args.id);
      return null;
    case "mods_set_on":
      mods = mods.map((m) => (m.id === args.id ? { ...m, on: !!args.on } : m));
      return null;
    case "mods_reorder":
      mods = (args.ids as string[]).map((id) => mods.find((m) => m.id === id)!).filter(Boolean);
      return null;
    case "asset_thumbnails":
      // No pictures in the preview: the placeholder shows.
      return [];
    case "take_pending_link": {
      const link = pendingLink;
      pendingLink = null;
      return link;
    }
    case "user_lookup": {
      await sleep(400);
      const name = String(args.name ?? "").trim().replace(/^@/, "");
      if (!name) return fail("invalid", "Type a Roblox username.");
      if (/nobody/i.test(name)) return fail("invalid", `No Roblox user is called "${name}".`);
      const known = [...accounts, ...FRIENDS].find((x) => x.username.toLowerCase() === name.toLowerCase());
      return known
        ? { userId: known.userId, username: known.username, displayName: known.displayName }
        : { userId: 7000000000 + name.length * 7919, username: name, displayName: name.replace(/_/g, " ") };
    }
    case "launch": {
      if (installing) return fail("invalid", INSTALLING);
      if (args.fpsCap && args.fpsCap < 5) return fail("invalid", "The FPS cap must be 0 (off) or 5 to 360.");
      if (batch && !batch.done) return fail("invalid", "A batch is still running.");
      await sleep(1200);
      const a = find(args.accountId);
      if (!a) return fail("notFound", "Not found.");
      // As commands::launch_tracked: one live client per account.
      if (instances.some((i) => i.accountId === a.id && ["starting", "joining", "inGame"].includes(i.state)))
        return fail("invalid", `${a.alias || a.displayName} is already running. Close its client first.`);
      if (a.username === "afk_unit_1") {
        a.status = "invalid";
        changed();
        return fail("cookieInvalid", "This cookie is not valid or has expired. Sign in again.");
      }
      retakeLock();
      const pid = 14000 + Math.floor(Math.random() * 9000);
      track(a.id, pid, args.target, placeOfTarget(args.target), a.username === "studio_nova", 0, args.fpsCap || null);
      return {
        accountId: a.id, pid, browserTrackerId: Math.floor(Math.random() * 1e12),
        placeId: placeOfTarget(args.target), startedAt: Date.now(), viaBootstrapper: a.username === "studio_nova",
        warning: args.target.kind === "shareLink" && args.target.code.startsWith("0") ? "This share link has expired, so Roblox may refuse the join." : null,
        fpsCap: args.fpsCap || null,
      };
    }
    case "favourites_list":
      return structuredClone(favourites);
    case "favourite_save": {
      const { id, name, target: t, accountIds, inTray, options } = args.input;
      const old = favourites.find((x) => x.id === id);
      const f: Favourite = old
        ? Object.assign(old, { name, target: t, accountIds, inTray }, options ? { options } : {})
        : { id: Math.max(0, ...favourites.map((x) => x.id)) + 1, name, target: t, accountIds, inTray, sortOrder: favourites.length, options: options ?? { sameServer: null, fpsCap: null, groupIds: [], hotkey: "" } };
      if (!old) favourites.push(f);
      return structuredClone(f);
    }
    case "favourite_remove":
      favourites = favourites.filter((x) => x.id !== args.id);
      return null;
    case "favourites_reorder":
      (args.ids as number[]).forEach((id, i) => {
        const f = favourites.find((x) => x.id === id);
        if (f) f.sortOrder = i;
      });
      return null;
    case "join_candidates": {
      await sleep(500);
      if (!find(args.accountId)) return fail("notFound", "Not found.");
      const mine = accounts
        .filter((a) => a.id !== args.accountId && a.status === "valid")
        .map((a): JoinCandidate => ({ userId: a.userId, username: a.username, displayName: a.displayName, accountId: a.id, presence: presenceOf(a) }));
      return structuredClone([...FRIENDS, ...mine]);
    }

    case "bulk_launch":
      if (installing) return fail("invalid", INSTALLING);
      await sleep(300);
      return startBatch(args.request);
    case "bulk_cancel":
      if (!batch || batch.batchId !== args.batchId) return fail("notFound", "That batch isn't running.");
      if (!batch.done) batchCancel = true;
      return null;
    case "bulk_current":
      return batch && structuredClone(batch);
    case "servers_list": {
      await sleep(500);
      if (![10, 25, 50, 100].includes(args.limit)) return fail("invalid", "A server page holds 10, 25, 50 or 100 servers.");
      if (args.accountId == null) {
        const wait = guestLimited();
        if (wait) return fail("rateLimited", `Roblox is limiting requests. Try again in ${wait} s.`);
        if (args.order === "recommended" || args.order === "bestLatency") return fail("invalid", SIGNED_IN_ONLY);
      }
      if (args.placeId === 1) return fail("notFound", "Not found.");
      return serverPage(args.placeId, args.order, args.excludeFull, args.limit, args.cursor);
    }
    case "game_info":
      await sleep(400);
      return args.placeId === 1 ? fail("notFound", "Not found.") : fakeGame(args.placeId);
    case "game_stats":
      await sleep(700);
      return fakeStats(args.universeId);
    case "account_value": {
      const a = find(args.id);
      if (!a) return fail("notFound", "Not found.");
      if (args.refresh) {
        await sleep(700 + (a.id % 5) * 250);
        if (a.status !== "valid") return fail("cookieInvalid", "The cookie is no longer valid.");
        holdings.set(a.id, { fetchedAt: Date.now(), items: holdingsOf(a) });
      }
      const h = holdings.get(a.id);
      return h ? valueOf(h) : null;
    }
    case "browse_card":
      return null;
    case "player_stats":
      await sleep(600);
      return args.userId === 1 ? fail("notFound", "Not found.") : fakePlayer(args.userId);
    case "player_value": {
      await sleep(args.refresh ? 1500 : 900);
      if (args.userId === 2) return { private: true, busy: false, value: null };
      const items = LIMITEDS.filter((_, i) => (i + args.userId) % 3 === 0).map(([assetId, name, rap]) => ({ assetId, name, rap, copies: 1 }));
      return { private: false, busy: false, value: valueOf({ fetchedAt: Date.now(), items }) };
    }
    case "games_popular": {
      await sleep(500);
      const games = (names: string[], seed: number) =>
        names.map((name, i) => ({ universeId: seed + i, rootPlaceId: 4410028153 + seed + i, name, playing: 120_000 - i * 9_000 - seed, upVotes: 9_000 - i * 400, downVotes: 600 + i * 90, iconUrl: null }));
      return [
        { id: "top-trending", name: "Top Trending", games: games(["Sky Islands", "Mega Obby", "Pet Valley", "Tower Run", "Fruit Seas", "Mine Rush"], 0) },
        { id: "up-and-coming", name: "Up-and-Coming", games: games(["Tiny Kingdoms", "Neon Drift", "Garden Grow"], 100) },
        { id: "top-playing-now", name: "Top Playing Now", games: games(["Steal An Egg", "Brick Battle", "Doors Below", "Speed Simulator"], 200) },
      ];
    }
    case "playtime_list":
      await sleep(300);
      return [
        { accountId: accounts[0].id, placeId: 4410028153, ms: 3 * 3_600_000 + 12 * 60_000, lastPlayed: Date.now() - 20 * 60_000 },
        { accountId: accounts[1].id, placeId: 4410028153, ms: 95 * 60_000, lastPlayed: Date.now() - 3 * 3_600_000 },
        { accountId: accounts[2].id, placeId: 7719004215, ms: 42 * 60_000, lastPlayed: Date.now() - 26 * 3_600_000 },
      ];
    case "target_place": {
      await sleep(300);
      const t: LaunchTarget = args.target;
      if ("placeId" in t) return t.placeId;
      if (t.kind === "universe") return 4410028153;
      if (t.kind === "followUser") return fail("invalid", "That link doesn't lead to a game.");
      if (!args.accountId) return fail("invalid", "Pick an account first: share links open as an account.");
      return 4410028153;
    }
    case "private_servers_list": {
      await sleep(500);
      const a = find(args.accountId);
      if (!a) return fail("notFound", "Not found.");
      if (a.status !== "valid") return fail("cookieInvalid", EXPIRED);
      if (args.placeId === 1) return fail("notFound", "Not found.");
      return privatePage(args.placeId, a);
    }

    case "groups_list":
      return structuredClone(groups);
    case "group_save": {
      const { id, name, color, defaults } = args.input;
      if (/["'&|<>^]/.test(defaults?.launchArgs ?? "")) return fail("invalid", "Launch arguments can't contain that character.");
      const g = id ? groups.find((x) => x.id === id) : undefined;
      if (g) Object.assign(g, { name, color: color ?? g.color, defaults: defaults ?? g.defaults });
      const out = g ?? { id: Math.max(0, ...groups.map((x) => x.id)) + 1, name, color: color ?? null, sortOrder: groups.length, defaults: defaults ?? NO_DEFAULTS };
      if (!g) groups.push(out);
      changed();
      return structuredClone(out);
    }
    case "group_remove":
      groups.splice(groups.findIndex((g) => g.id === args.id), 1);
      accounts.forEach((a) => (a.groupIds = a.groupIds.filter((g) => g !== args.id)));
      favourites.forEach((f) => (f.options.groupIds = f.options.groupIds.filter((g) => g !== args.id)));
      changed();
      return null;
    case "group_clear_overrides": {
      const members = accounts.filter((a) => a.groupIds.includes(args.id));
      members.forEach((a) => {
        a.fpsCap = null;
        a.launchArgs = null;
        a.clientLimits = null;
      });
      changed();
      return members.length;
    }

    case "backup_export":
      await sleep(600);
      return accounts.length;
    case "backup_import":
      await sleep(800);
      if (args.password !== "password") return fail("wrongPassword", "Wrong password.");
      return { added: 2, updated: args.onConflict === "replace" ? 7 : 0, skipped: args.onConflict === "skip" ? 7 : 0, failed: 0, settings: args.settings ? true : null, failures: [] };

    case "instances_list":
      // ?instances=slow (Home's loading state) or =fail (its error state).
      if (preview.get("instances") === "slow") await sleep(2500);
      if (preview.get("instances") === "fail") return fail("internal", "Couldn't read the running clients.");
      return structuredClone(instances);
    case "instance_close":
      await sleep(600);
      endInstance(args.id, "Closed");
      return null;
    case "instance_move": {
      // As commands::instance_move: close the playing client, then launch again.
      const i = instances.find((x) => x.id === args.id);
      if (!i) return fail("notFound", "Not found.");
      await sleep(600);
      endInstance(i.id, "Closed");
      return handle("launch", { accountId: i.accountId, target: args.target, fpsCap: args.fpsCap });
    }
    case "instances_close_all":
      await sleep(900);
      instances.forEach((i) => endInstance(i.id, "Closed"));
      return null;
    case "instance_set_limits": {
      const i = instances.find((x) => x.id === args.id && x.state !== "exited");
      if (!i) return fail("notFound", "Not found.");
      if (args.limits.affinity === 0) return fail("invalid", "Pick at least one CPU core.");
      i.limits = args.limits;
      remember(i);
      instancesChanged();
      return null;
    }
    case "instances_apply_limits": {
      if (args.limits.affinity === 0) return fail("invalid", "Pick at least one CPU core.");
      const live = instances.filter((x) => x.state !== "exited");
      live.forEach((i) => (i.limits = args.limits));
      instancesChanged();
      return live.length;
    }
    case "instance_set_anti_afk": {
      const i = instances.find((x) => x.id === args.id && x.state !== "exited");
      if (!i) return fail("notFound", "Not found.");
      i.antiAfk = args.enabled;
      remember(i);
      instancesChanged();
      return null;
    }
    case "instance_take_control": {
      const i = instances.find((x) => x.id === args.id && x.state !== "exited");
      if (!i) return fail("notFound", "Not found.");
      i.held = args.on;
      if (args.on) i.micro = false;
      instancesChanged();
      return null;
    }
    case "instance_trim":
      await sleep(300);
      return instances.some((x) => x.id === args.id && x.state !== "exited") ? [840, 210] : fail("notFound", "Not found.");
    case "instance_thumbs":
      // Windows draws these; the preview has none.
      return null;
    case "instance_thumbnail": {
      const i = instances.find((x) => x.id === args.id);
      if (!i) return fail("notFound", "Not found.");
      if (i.state === "exited" || i.state === "starting") return thumbReply(i, 2, args.known);
      const forced = previewForced.get(i.id);
      if (forced === "pending" || forced === "unsupported") return thumbReply(i, forced === "pending" ? 3 : 4, args.known);
      if (forced === "minimized") return thumbReply(i, 1, args.known, shots.get(i.id));
      // maxW 0: the status only (previews Off, between stills).
      if (forced !== "stale" && args.maxW > 0) shots.set(i.id, fakeShot(i, Math.min(args.maxW, i.micro ? 320 : 800)));
      return thumbReply(i, 0, args.known, shots.get(i.id));
    }
    case "instance_show": {
      const i = instances.find((x) => x.id === args.id && x.state !== "exited");
      if (!i) return fail("notFound", "Not found.");
      if (args.what === "minimize") ghosts.add(i.id); else ghosts.delete(i.id);
      return null;
    }
    case "instances_show_all":
      if (args.what === "focus") return fail("invalid", "Only one client can be in front.");
      for (const i of instances) if (args.what === "minimize") ghosts.add(i.id); else ghosts.delete(i.id);
      return null;
    case "instance_micro": {
      const i = instances.find((x) => x.id === args.id && x.state !== "exited");
      if (!i) return fail("notFound", "Not found.");
      if (i.state === "starting") return fail("invalid", "That client hasn't started yet, or has closed.");
      i.micro = args.on;
      instancesChanged();
      return args.on ? (args.size ?? [settings.microWidth, settings.microHeight]) : [800, 600];
    }
    case "layout_preview_stop":
      return null;
    case "layout_preview":
      // A browser has no screen to draw on; the real preview opens a window over the monitor.
      if (settings.windowLayout === "off") return fail("invalid", "Pick Fit or Tiny to see where the windows go.");
      return null;
    case "instances_tile": {
      const shown = instances.filter((i) => i.state !== "exited" && i.state !== "starting" && !ghosts.has(i.id));
      if (args.micro) shown.forEach((i) => { i.micro = true; });
      instancesChanged();
      return shown.length;
    }
    case "instances_metrics":
      return fakeMetrics();
    case "system_usage": {
      const m = fakeMetrics();
      return { cpuPct: Math.min(100, 12 + m.reduce((n, x) => n + (x.cpuPct ?? 0), 0)), memUsedMb: 7000 + m.reduce((n, x) => n + x.privateMb, 0), memTotalMb: 32_768 };
    }
    case "instance_stop_relaunch": {
      const i = instances.find((x) => x.id === args.id);
      if (i) {
        i.relaunching = false;
        i.relaunchProblem = null;
      }
      instancesChanged();
      return null;
    }
    case "instances_retry_relaunch":
      for (const i of instances.filter((x) => x.relaunching)) {
        i.relaunching = false;
        i.relaunchProblem = null;
        i.state = "joining";
        setTimeout(() => {
          i.state = "inGame";
          instancesChanged();
        }, 2000);
      }
      instancesChanged();
      return null;
    case "mock_trouble": {
      const i = instances.find((x) => x.state === "inGame");
      if (i) {
        i.state = "disconnected";
        i.relaunching = true;
        i.relaunchProblem = "Couldn't reach Roblox. Check your connection and try again.";
        instancesChanged();
      }
      return null;
    }
    case "bandwidth_status":
      if (preview.get("bandwidth") === "error") return fail("internal", "Windows didn't say which QoS policies are set.");
      return { kbps: settings.bandwidthKbps, active: settings.bandwidthKbps > 0 && bandwidthActive };
    case "bandwidth_apply":
      await sleep(1500);
      if (args.kbps !== 0 && (args.kbps < 64 || args.kbps > 10_000_000)) return fail("invalid", "The bandwidth limit must be 0 (none) or 64 to 10000000 kbit/s.");
      if (args.kbps === 1234) return fail("invalid", "The bandwidth limit needs admin rights; the prompt was declined.");
      settings.bandwidthKbps = args.kbps;
      bandwidthActive = true;
      return { kbps: settings.bandwidthKbps, active: args.kbps > 0 };
    case "instances_clear_ended":
      instances = instances.filter((i) => i.state !== "exited");
      instancesChanged();
      return null;
    case "multi_instance_status":
      retakeLock();
      return multiStatus();
    case "multi_instance_set":
      settings.multiInstance = args.enabled;
      if (args.enabled) retakeLock();
      else lockHeld = false;
      return multiStatus();
    case "multi_instance_take_over":
      await sleep(600);
      if (!settings.multiInstance) return fail("invalid", "Turn on multi-instance first.");
      strangerRunning = false;
      eventHolder = null;
      lockHeld = true;
      return multiStatus();
    case "roblox_close_all":
      await sleep(1200);
      strangerRunning = false;
      instances.forEach((i) => endInstance(i.id, "Closed"));
      retakeLock();
      return multiStatus();
    case "panic_kill":
      await sleep(300);
      if (batch && !batch.done) batchCancel = true;
      strangerRunning = false;
      instances.forEach((i) => endInstance(i.id, "Closed"));
      retakeLock();
      return multiStatus();
    case "server_history":
      return args.accountId === 2
        ? [
            { placeId: 1818, jobId: "0d6c3f4e-5b2a-4c1d-9e8f-7a6b5c4d3e2f", private: false, joinedAt: Date.now() - 20 * MIN },
            { placeId: 1818, jobId: "1e7d4f5a-6c3b-4d2e-8f90-8b7c6d5e4f30", private: true, joinedAt: Date.now() - 180 * MIN },
          ]
        : [];
    case "server_history_recent":
      await sleep(300);
      return [
        { placeId: 4410028153, jobId: "2f8e5a6b-7d4c-4e3f-9a01-9c8d7e6f5a41", private: false, joinedAt: Date.now() - 6 * MIN, accountIds: accounts.slice(0, 2).map((a) => a.id) },
        { placeId: 1818, jobId: "0d6c3f4e-5b2a-4c1d-9e8f-7a6b5c4d3e2f", private: false, joinedAt: Date.now() - 20 * MIN, accountIds: [2] },
        { placeId: 1818, jobId: "1e7d4f5a-6c3b-4d2e-8f90-8b7c6d5e4f30", private: true, joinedAt: Date.now() - 180 * MIN, accountIds: [2] },
      ];
    case "diagnostics_text":
      return "RoALT (preview) diagnostics";
    case "diagnostics_save":
      return null;
    case "crashes_list":
      return structuredClone(crashes);
    case "crash_open_log":
      return crashes.some((c) => c.id === args.id && c.hasLog) ? null : fail("notFound", "Not found.");
    case "crashes_export":
    case "playtime_export":
      await sleep(300);
      return null;
    case "playtime_days":
      return DEMO ? [] : playtimeDays();

    case "robux_totals":
      await sleep(350);
      return args.accountId === 5 ? fail("roblox", "Roblox didn't answer. Try again.") : { incoming: 1250, outgoing: -430, pending: 75 };
    case "robux_transactions":
      await sleep(450);
      return args.kind === "purchase"
        ? [robuxLine("Sparkle Time Fedora", "Roblox", -250, 6), robuxLine("VIP pass", "Sky Islands", -150, 30), robuxLine("Speed coil", "obby_queen", -30, 80)]
        : args.kind === "sale"
          ? [robuxLine("Donation pass", "nova_builds", 70, 3, true), robuxLine("Donation pass", "pixel_pat", 35, 50)]
          : [];

    case "discord_get":
      return discordStatus();
    case "discord_set": {
      await sleep(300);
      const url = String(args.url ?? "").trim();
      if (!url) {
        webhook = null;
        return discordStatus();
      }
      const m = WEBHOOK.exec(url);
      if (!m) return fail("invalid", "Paste a Discord webhook URL: https://discord.com/api/webhooks/<id>/<token>.");
      webhook = { id: m[1], token: m[2] };
      return discordStatus();
    }
    case "discord_test":
      await sleep(700);
      if (!webhook) return fail("invalid", "Set a webhook first.");
      if (webhook.token.includes("deleted")) return fail("invalid", "Discord refused the webhook; it may have been deleted. Paste a new one.");
      return null;
    case "news": {
      if (!settings.news) return { items: [], failed: [] };
      await sleep(700);
      const at = (hours: number) => new Date(Date.now() - hours * 3_600_000).toISOString();
      const release = (v: string, hours: number, notes: string) => ({ id: `roalt:${v}`, source: "roalt", title: `RoALT ${v}`, summary: notes, url: `https://github.com/inspectsoftware/RoALT/releases/tag/v${v}`, published: at(hours), version: v });
      const forum = (id: number, title: string, hours: number, summary: string) => ({ id: `devforum:${id}`, source: "devforum", title, summary, url: `https://devforum.roblox.com/t/topic/${id}`, published: at(hours), version: null });
      const month = new Date().toISOString().slice(0, 7);
      return {
        items: [
          ...(DEMO ? [] : [release(NEXT_VERSION, 3, "## New\n- **A redesigned app**: the island, client cards, Home tabs and a searchable Settings.\n- Export cookies from Accounts.\n- Private servers: change their settings, or buy one.\n\n## Fixed\n- Tooltips no longer jump to the corner of the window.\n- Switching to the side layout no longer flashes the tab strip.\n- The Performance icon sits centred.\n\nNo Authenticode signature yet.")]),
          forum(9001, "Studio MCP Server Updates: Open Cloud Access in Your MCP Client", 5, "Key Takeaways MCP now has full access to Open Cloud; connected agents can help you manage your game beyond building…"),
          forum(9002, "PlayerCharacterDestroyBehavior Will Be Default on November 11", 9, "Hi Creators, What’s Changing We introduced PlayerCharacterDestroyBehavior as an opt-in property almost three years ago…"),
          forum(9003, "Release Notes for 745", 200, ""),
          release(APP_VERSION, 72, "## Fixes\n- **Anti-AFK now really keeps clients online.**"),
          { id: `newsroom:${month}/the-hollow`, source: "newsroom", title: "The Hollow: New Platform-Wide Survival Event for Halloween", summary: "", url: "https://about.roblox.com/newsroom", published: month, version: null },
        ],
        failed: [],
      };
    }
    case "online_ping":
      return !DEMO && settings.onlineCount ? { online: 1284, playing: 5120 } : null;
    case "self_update_status":
      return { current: APP_VERSION, available: appUpdate };
    case "self_update_check":
      await sleep(800);
      appUpdate = DEMO ? null : NEXT_VERSION;
      return { current: APP_VERSION, available: appUpdate };
    case "self_update_install":
      if (!appUpdate) return fail("invalid", "RoALT is up to date.");
      if (batch && !batch.done) return fail("invalid", "Wait for the running launch, batch or auto-rejoin to finish, then update RoALT.");
      // Downloading, checking, then the installer restarts the app.
      await sleep(2500);
      location.reload();
      return new Promise(() => {});
    case "roblox_client":
      return { ...robloxClient, bootstrapper: launchers.some((l) => l.current && l.kind === "bootstrapper") };
    case "roblox_launchers":
      return launchers.map((l) => ({ ...l }));
    case "roblox_launcher_set": {
      const { path } = args as { path: string };
      if (!launchers.some((l) => l.path === path)) return fail("invalid", "That launcher isn't installed anymore. Pick another one.");
      await sleep(300);
      launchers = launchers.map((l) => ({ ...l, current: l.path === path }));
      return launchers.map((l) => ({ ...l }));
    }
    case "roblox_install": {
      if (installing) return fail("invalid", "Roblox is already being installed.");
      if (instances.some((i) => i.state !== "exited") || strangerRunning) return fail("invalid", "Close every Roblox client first: installing Roblox ends them.");
      installing = true;
      try {
        for (let percent = 0; percent <= 100; percent += 20) {
          void emit("roblox://install", { stage: "downloading", percent });
          await sleep(250);
        }
        void emit("roblox://install", { stage: "verifying" });
        await sleep(500);
        void emit("roblox://install", { stage: "installing" });
        await sleep(1500);
        const latest = robloxClient.latest ?? "version-9ac8595712b447a7";
        robloxClient = { ...robloxClient, path: robloxClient.path?.replace(/version-[0-9a-f]{16}/, latest) ?? null, version: latest, outdated: false };
        void emit("roblox://install", { stage: "done" });
        void emit("roblox://client", robloxClient);
        return { ...robloxClient };
      } finally {
        installing = false;
      }
    }
    case "data_dir_info":
      return { path: "C:\\Users\\you\\AppData\\Local\\RoALT", overridden: false };
    case "data_dir_open":
    case "window_to_tray":
    case "app_quit":
    case "logs_open":
      return null;
    case "debug_tail":
      return fakeLog.slice(-(args as { lines: number }).lines);
    case "debug_log": {
      const { level, message } = args as { level: string; message: string };
      logLine(({ error: 5, warn: 4, info: 3 } as Record<string, number>)[level] ?? 2, "ui", message);
      return null;
    }
    case "background_set": {
      // ?bg=gif: a 1×1 GIF that flips between two colours every second (a moving background).
      if (new URLSearchParams(location.search).get("bg") === "gif") {
        const gce = [0x21, 0xf9, 4, 0, 100, 0, 0, 0];
        const frame = (lzw: number) => [...gce, 0x2c, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, lzw, 1, 0];
        const loop = [0x21, 0xff, 11, ...[..."NETSCAPE2.0"].map((ch) => ch.charCodeAt(0)), 3, 1, 0, 0, 0];
        const head = [..."GIF89a"].map((ch) => ch.charCodeAt(0));
        background = new Uint8Array([...head, 1, 0, 1, 0, 0x80, 0, 0, 0x2a, 0x1a, 0x5e, 0xf6, 0xa6, 0x5a, ...loop, ...frame(0x44), ...frame(0x4c), 0x3b]).buffer;
        return null;
      }
      // A generated picture stands in for the picked file.
      const c = document.createElement("canvas");
      c.width = 640;
      c.height = 400;
      const g = c.getContext("2d")!;
      const grad = g.createLinearGradient(0, 0, 640, 400);
      grad.addColorStop(0, "#2a1a5e");
      grad.addColorStop(0.5, "#c2416b");
      grad.addColorStop(1, "#f6a65a");
      g.fillStyle = grad;
      g.fillRect(0, 0, 640, 400);
      background = await new Promise<Blob>((ok) => c.toBlob((b) => ok(b!), "image/png")).then((b) => b.arrayBuffer());
      return null;
    }
    case "background_clear":
      background = new ArrayBuffer(0);
      return null;
    case "background_get":
      return background;
    case "browser_data_reset":
      await sleep(300);
      return true;
    case "logs_clean":
      await sleep(400);
      return { files: 7, bytes: 183_500_800 };
    case "shutdown_schedule": {
      const secs = args.inSecs ?? Math.ceil((args.atMs - Date.now()) / 1000);
      if (!(secs >= 60 && secs <= 7 * 24 * 3600)) return fail("invalid", "The shutdown must be 1 minute to 7 days from now.");
      shutdownAt = Date.now() + secs * 1000;
      return shutdownAt;
    }
    case "shutdown_cancel":
      shutdownAt = null;
      return null;
    case "shutdown_status":
      return shutdownAt;
    case "network_status":
      return { public: publicNetwork };
    case "data_dir_move":
      await sleep(1200);
      if (batch && !batch.done) return fail("invalid", MOVE_BUSY);
      if (/bad/i.test(args.path)) return fail("invalid", "That folder already has a RoALT database. Pick another one.");
      location.reload();
      return new Promise(() => {});

    case "settings_get":
      return { ...settings };
    case "settings_set":
      if (/["'&|<>^]/.test(args.patch.launchArgs ?? "")) return fail("invalid", "Launch arguments can't contain that character.");
      if (args.patch.memRestartMb && args.patch.memRestartMb < 512) return fail("invalid", "The memory limit must be 0 (off) or 512 to 65 536 MB.");
      if (args.patch.fpsCap && args.patch.fpsCap < 5) return fail("invalid", "The FPS cap must be 0 (off) or 5 to 360.");

      if (args.patch.antiAfk && (args.patch.antiAfk.intervalMin < 1 || args.patch.antiAfk.intervalMin > 19))
        return fail("invalid", "Anti-AFK must press a key every 1 to 19 minutes.");
      if (args.patch.discord && args.patch.discord.statusEveryMin !== 0 && (args.patch.discord.statusEveryMin < 5 || args.patch.discord.statusEveryMin > 1440))
        return fail("invalid", "The status summary must be off (0) or every 5 minutes to 24 hours.");
      if (args.patch.panicHotkey && !/^((ctrl|shift|alt|super)\+)*[^+]+$/i.test(args.patch.panicHotkey))
        return fail("invalid", `"${args.patch.panicHotkey}" isn't a key combination, like Ctrl+Shift+F12.`);
      if (args.patch.selfUpdate === false && settings.selfUpdate) {
        appUpdate = null;
        void emit("app://update", { current: APP_VERSION, available: null });
      }
      settings = { ...settings, ...args.patch };
      return { ...settings };

    case "performance_profile":
      return perfProfile(args.previewFps);
    case "performance_export":
      lastProfile = perfProfile(args.previewFps);
      return null;
    case "performance_read":
    case "performance_parse": {
      const r = perfParse(cmd === "performance_parse" ? args.text : lastProfile ?? perfProfile(15));
      return typeof r === "string" ? fail("invalid", r) : r;
    }
    case "performance_benchmark": {
      // A canned Tight result (the real one takes ~22 s); planning for 13 clients fails.
      await sleep(2000);
      if (args.target === 13) return fail("internal", "Windows stopped answering about the clients' memory. Try again.");
      const n = args.target ?? 5;
      return {
        bench: { cores: 8, ramMb: 16_384, clients: 5, background: 4, perMb: 1400, peakPrivateMb: 1900, perCores: 0.6, freeCores: 1.2, underLimits: false, otherMb: 3000, otherCores: 0.6, gpuPeak: 42, caps: "unknown", memRestarts: 0, longCrashes: 0 },
        advice: {
          n, tier: "tight", ramRatio: 0.61, cpuRatio: 0.45, room: 7,
          suggestions: [
            { id: "fpsCap", params: { fps: 20 }, profile: { fpsCap: 20 } },
            { id: "minimizeOnLaunch", params: { n }, profile: { minimizeOnLaunch: true } },
            { id: "muteOnLaunch", params: { n }, profile: { muteOnLaunch: true } },
            { id: "adaptive", params: { cap: 25 }, profile: { adaptive: { enabled: true, background: { cpuCap: 25, eco: true, trimEveryMin: null, affinity: null } } } },
            { id: "trimEveryMin", params: { min: 30 }, profile: { trimEveryMin: 30 } },
            { id: "previewFps", params: { fps: 15 }, profile: { previewFps: 15 } },
          ],
        },
      };
    }

    case "extras":
      // ?sponsor=none: what ships until a sponsor signs. The site's demo names no made-up sponsor.
      return DEMO || preview.get("sponsor") === "none" ? { sponsor: null, supporters: [] } :{ sponsor: { name: "Example Host", text: "Cloud PCs that run more alts than yours can. From $5 a month." }, supporters: ["nova", "kit", "studio_nova"] };

    case "performance_specs":
      await sleep(300);
      return { cpu: "AMD Ryzen 5 5600 6-Core Processor", threads: 12, ramMb: 16_384, gpu: "NVIDIA GeForce RTX 3060", vramMb: 12_288, integrated: false, rooms: { maxAlts: 18, balanced: 12, defaults: 7 } };

    case "plugin:dialog|save":
      return `C:\\Users\\you\\Documents\\${args.options?.defaultPath ?? "backup.roalt"}`;
    case "plugin:dialog|open":
      if (args.options?.filters?.[0]?.extensions?.includes("zip")) return "C:\\Users\\you\\Downloads\\Purple Cursor.zip";
      return args.options?.directory ? "D:\\RoALT data" : "C:\\Users\\you\\Documents\\RoALT-backup-2026-10-01.roalt";
    case "plugin:window|is_maximized":
      return false;
    default:
      // Window controls, opener and clipboard calls do nothing in the preview.
      if (!/^plugin:(window|opener|clipboard-manager)\|/.test(cmd)) console.warn("[mock ipc] unhandled", cmd);
      return null;
  }
}

// Preview switches, for states that are slow or awkward to reach by hand:
//   ?locked=1  vault locked at start      ?public=1   on a Public network with the guard on
//   ?norecovery=1  no recovery code set
//   ?theme=light|dark|system  ?lang=ar  ?accent=c6f432  ?layout=side
//   ?accounts=200  that many more accounts (long-list checks)
//   ?scenario=busy  three clients start, then a batch of three ("AFK 1" fails in it)
//   ?bandwidth=lapsed  a limit Windows has dropped (it restarted)   ?bandwidth=error  its status can't be read
//   ?instances=slow  Home's client list comes late   ?instances=fail  it can't be read
//   ?link=roalt://play?place%3D1818  opened by a RoALT link (encode its ? and &)
//   ?rolimons=1  Rolimons values on (Browse › Limiteds lists)
//   ?fresh  a first start: no accounts, groups or favourites, Windows protection (the setup opens)
//   ?closeask=1  the window's close asks (as with clients RoALT looks after)   ?news=off  News is off
//   ?mods=none  no mods yet  ?mods=bootstrapper  the last launch skipped them (a bootstrapper)
//   (?splash=1 replays the splash: Splash.tsx; ?rm=1 reduces motion: ui/motion/reduced.ts)
const preview = new URLSearchParams(location.search);
let publicNetwork = false;
if (preview.has("locked")) vault.locked = true;
// A first start: nothing saved yet, Windows protection; the setup opens after the splash.
if (preview.has("fresh")) {
  accounts = [];
  groups.length = 0;
  favourites = [];
  vault.mode = "dpapi";
}
if (preview.has("norecovery")) vault.recovery = "";
if (preview.has("public")) {
  publicNetwork = true;
  settings.allowPublicNetworks = false;
}
const themeArg = preview.get("theme");
if (themeArg === "light" || themeArg === "dark" || themeArg === "system") settings.theme = themeArg;
if (preview.get("lang")) settings.language = preview.get("lang")!;
const accentArg = preview.get("accent")?.replace(/^#/, "");
if (accentArg && /^[0-9a-f]{6}$/i.test(accentArg)) settings.accent = `#${accentArg.toLowerCase()}`;
if (preview.get("layout") === "side") settings.tabLayout = "side";
if (preview.has("rolimons")) settings.rolimonsValues = true;
if (preview.get("mods") === "none") mods = [];
if (preview.has("closeask")) setTimeout(() => void emit("app://close-ask"), 1500);
if (preview.get("news") === "off") settings.news = false;
if (preview.get("mods") === "bootstrapper") modsLast = { applied: false, note: "Roblox runs through a bootstrapper, which has its own mods." };
// As commands::open_link: a public game or server only.
let pendingLink: LaunchTarget | null = null;
const linkArg = preview.get("link");
const linked = linkArg ? parseTarget(linkArg) : null;
if (linked?.kind === "target" && (linked.target.kind === "place" || linked.target.kind === "server")) pendingLink = linked.target;
if (preview.get("bandwidth") === "lapsed") settings.bandwidthKbps = 2500;
const extra = Math.min(5000, Number(preview.get("accounts")) || 0);
for (let n = 0; n < extra; n++) {
  const id = accounts.length + 1;
  accounts.push({ ...accounts[1], id, userId: 7_000_000_000 + n, username: `bulk_alt_${n + 1}`, displayName: `Bulk ${n + 1}`, alias: "", sortOrder: id, main: false, favourite: false, fpsCap: null, groupIds: n % 3 ? [3] : [] });
}
if (preview.get("scenario") === "busy") {
  strangerRunning = false;
  lockHeld = true;
  [1, 2, 7].forEach((id, n) => setTimeout(() => track(id, 9000 + n, { kind: "place", placeId: 1818, launchData: null }, 1818, id === 7), 200 + n * 180));
  setTimeout(() => void startBatch({ accountIds: [3, 5, 8], target: { kind: "place", placeId: 1818, launchData: null }, delayMs: 6000, sameServer: "off", fpsCap: null }), 1600);
}

mockWindows("main");
mockIPC((cmd, args) => handle(cmd, (args ?? {}) as Args), { shouldMockEvents: true });
