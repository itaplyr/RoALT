// Typed wrappers for every Tauri command and event. Types come from src/bindings (ts-rs).
import { invoke as rawInvoke, type InvokeArgs } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AccountInfo } from "./bindings/AccountInfo";
import type { AccountPatch } from "./bindings/AccountPatch";
import type { AccountPresence } from "./bindings/AccountPresence";
import type { AccountSummary } from "./bindings/AccountSummary";
import type { AppUpdate } from "./bindings/AppUpdate";
import type { BandwidthStatus } from "./bindings/BandwidthStatus";
import type { BulkProgress } from "./bindings/BulkProgress";
import type { BulkRequest } from "./bindings/BulkRequest";
import type { ClientMetrics } from "./bindings/ClientMetrics";
import type { SystemUsage } from "./bindings/SystemUsage";
import type { Profile } from "./bindings/Profile";
import type { ServerVisit } from "./bindings/ServerVisit";
import type { ConflictPolicy } from "./bindings/ConflictPolicy";
import type { Crash } from "./bindings/Crash";
import type { DataDir } from "./bindings/DataDir";
import type { DiscordStatus } from "./bindings/DiscordStatus";
import type { ErrorPayload } from "./bindings/ErrorPayload";
import type { Favourite } from "./bindings/Favourite";
import type { FavouriteInput } from "./bindings/FavouriteInput";
import type { GameInfo } from "./bindings/GameInfo";
import type { Group } from "./bindings/Group";
import type { GroupInput } from "./bindings/GroupInput";
import type { Headshot } from "./bindings/Headshot";
import type { ImportReport } from "./bindings/ImportReport";
import type { InstallProgress } from "./bindings/InstallProgress";
import type { Instance } from "./bindings/Instance";
import type { JoinCandidate } from "./bindings/JoinCandidate";
import type { LaunchInfo } from "./bindings/LaunchInfo";
import type { LaunchTarget } from "./bindings/LaunchTarget";
import type { Limits } from "./bindings/Limits";
import type { LogsCleaned } from "./bindings/LogsCleaned";
import type { MultiInstanceStatus } from "./bindings/MultiInstanceStatus";
import type { Notice } from "./bindings/Notice";
import type { ParsedInput } from "./bindings/ParsedInput";
import type { PrivateServerPage } from "./bindings/PrivateServerPage";
import type { NetworkStatus } from "./bindings/NetworkStatus";
import type { RobloxClient } from "./bindings/RobloxClient";
import type { ServerOrder } from "./bindings/ServerOrder";
import type { ServerPage } from "./bindings/ServerPage";
import type { Settings } from "./bindings/Settings";
import type { SettingsPatch } from "./bindings/SettingsPatch";
import type { UserRef } from "./bindings/UserRef";
import type { VaultMode } from "./bindings/VaultMode";
import type { VaultStatus } from "./bindings/VaultStatus";

export type {
  AccountInfo, AccountPresence, Headshot,
  Favourite, FavouriteInput, JoinCandidate, LaunchInfo, LaunchTarget, ParsedInput, UserRef,
  BandwidthStatus, ClientMetrics, Crash, DiscordStatus, Instance, MultiInstanceStatus, BulkProgress, BulkRequest, ServerOrder, ServerPage, GameInfo, PrivateServerPage, DataDir, LogsCleaned, Notice, RobloxClient,
  AccountPatch, AccountSummary, AppUpdate, ConflictPolicy, ErrorPayload, Group, GroupInput,
  ImportReport, InstallProgress, Settings, SettingsPatch, VaultMode, VaultStatus,
};
export type { AccountStatus } from "./bindings/AccountStatus";
export type { Adaptive } from "./bindings/Adaptive";
export type { Profile } from "./bindings/Profile";
export type { SystemUsage } from "./bindings/SystemUsage";
export type { ServerVisit } from "./bindings/ServerVisit";
export type { AfkMode } from "./bindings/AfkMode";
export type { AntiAfk } from "./bindings/AntiAfk";
export type { BulkItem } from "./bindings/BulkItem";
export type { BulkStage } from "./bindings/BulkStage";
export type { CrashKind } from "./bindings/CrashKind";
export type { Density } from "./bindings/Density";
export type { DiscordEvents } from "./bindings/DiscordEvents";
export type { GameServer } from "./bindings/GameServer";
export type { PrivateServer } from "./bindings/PrivateServer";
export type { SameServer } from "./bindings/SameServer";
export type { InstanceState } from "./bindings/InstanceState";
export type { Limits } from "./bindings/Limits";
export type { Presence } from "./bindings/Presence";
export type { PresenceKind } from "./bindings/PresenceKind";
export type { Subscription } from "./bindings/Subscription";
export type { Theme } from "./bindings/Theme";

/** What instance_show does with a client's window (window::Show). */
export type WindowShow = "focus" | "minimize" | "restore";

/** Rejected invokes carry an ErrorPayload; anything else becomes an "internal" one. */
export function asError(e: unknown): ErrorPayload {
  if (e && typeof e === "object" && "kind" in e && "message" in e) return e as ErrorPayload;
  return { kind: "internal", message: String(e) };
}

/** A line into roalt.log (the Debug tab), target "ui". Never throws. */
export function debugLog(level: "error" | "warn" | "info" | "debug", message: string) {
  rawInvoke("debug_log", { level, message }).catch(() => {});
}

/** `invoke`, logging rejections Rust didn't make (unknown command, bad arguments, not allowed):
 * Rust logs its own AppErrors. Never logs arguments or results (passwords, cookies, PINs). */
function invoke<T>(cmd: string, args?: InvokeArgs): Promise<T> {
  return rawInvoke<T>(cmd, args).catch((e: unknown) => {
    if (!(e && typeof e === "object" && "kind" in e && "message" in e)) debugLog("error", `invoke ${cmd} failed: ${String(e)}`);
    throw e;
  });
}

/** One `log://log` event (tauri-plugin-log's Webview target): level 1 trace … 5 error. */
export type LogRecord = { level: number; message: string };

export const api = {
  vaultStatus: () => invoke<VaultStatus>("vault_status"),
  vaultUnlock: (password: string) => invoke<void>("vault_unlock", { password }),
  vaultLock: () => invoke<void>("vault_lock"),
  vaultSetMode: (mode: VaultMode, currentPassword?: string, newPassword?: string) =>
    invoke<void>("vault_set_mode", { mode, currentPassword, newPassword }),
  /** Sets the new password and unlocks; returns the fresh code, since this one is used up. */
  vaultRecover: (code: string, newPassword: string) => invoke<string>("vault_recover", { code, newPassword }),
  /** A new recovery code; the one before stops working. */
  vaultRecoveryCreate: (currentPassword: string) => invoke<string>("vault_recovery_create", { currentPassword }),
  vaultRecoverySave: (path: string, code: string) => invoke<void>("vault_recovery_save", { path, code }),
  vaultRecoveryCopy: (code: string) => invoke<void>("vault_recovery_copy", { code }),
  appActivity: () => invoke<void>("app_activity"),
  /** The splash reached its grow: resolves once the window has grown into the app. */
  splashDone: () => invoke<void>("splash_done"),

  accountsList: () => invoke<AccountSummary[]>("accounts_list"),
  accountAddCookie: (cookie: string) => invoke<AccountSummary>("account_add_cookie", { cookie }),
  /** Every .ROBLOSECURITY in the text, added one by one (slow: each is checked with Roblox). */
  accountsImportCookies: (text: string) => invoke<ImportReport>("accounts_import_cookies", { text }),
  /** Roblox Account Manager's AccountData.json (plain or DPAPI, not password-locked). */
  accountsImportRam: (path: string) => invoke<ImportReport>("accounts_import_ram", { path }),
  /** Opens the Roblox sign-in window; resolves once the cookie is captured. */
  accountAddLogin: () => invoke<AccountSummary>("account_add_login"),
  accountUpdate: (id: number, patch: AccountPatch) => invoke<AccountSummary>("account_update", { id, patch }),
  accountsReorder: (ids: number[]) => invoke<void>("accounts_reorder", { ids }),
  accountsRemove: (ids: number[]) => invoke<void>("accounts_remove", { ids }),
  accountsValidate: (ids?: number[]) => invoke<AccountSummary[]>("accounts_validate", { ids }),
  /** Master mode needs the password; DPAPI mode shows Windows Hello. */
  accountRevealCookie: (id: number, password?: string) => invoke<string>("account_reveal_cookie", { id, password }),
  accountCopyCookie: (id: number, password?: string) => invoke<void>("account_copy_cookie", { id, password }),

  /** Cached for settings.cacheMinutes unless refresh. Parts Roblox didn't answer are null and named in `unavailable`. */
  accountInfo: (id: number, refresh = false) => invoke<AccountInfo>("account_info", { id, refresh }),
  /** Avatar URLs for all accounts (cached 6 h); accounts without one are missing from the list. */
  accountsHeadshots: (refresh = false) => invoke<Headshot[]>("accounts_headshots", { refresh }),
  /** Presence of these accounts, each asked with its own cookie (accounts without a valid cookie are left out). */
  accountsPresence: (ids: number[]) => invoke<AccountPresence[]>("accounts_presence", { ids }),
  /** Opens roblox.com signed in as the account in its own window. */
  accountOpenSession: (id: number) => invoke<void>("account_open_session", { id }),

  /** Local, instant: what pasted text points at. */
  targetParse: (input: string) => invoke<ParsedInput>("target_parse", { input }),
  /** A Roblox username (current or previous) to its user. */
  userLookup: (name: string) => invoke<UserRef>("user_lookup", { name }),
  /** Starts the Roblox client as this account. */
  /** `fpsCap`: this launch's own, instead of the account's and the default; 0 or none: those. */
  launch: (accountId: number, target: LaunchTarget, favouriteId?: number, fpsCap?: number | null) =>
    invoke<LaunchInfo>("launch", { accountId, target, favouriteId, fpsCap: fpsCap || null }),
  favouritesList: () => invoke<Favourite[]>("favourites_list"),
  favouriteSave: (input: FavouriteInput) => invoke<Favourite>("favourite_save", { input }),
  favouriteRemove: (id: number) => invoke<void>("favourite_remove", { id }),
  favouritesReorder: (ids: number[]) => invoke<void>("favourites_reorder", { ids }),
  /** Friends online plus the other stored accounts, for "Join user". */
  joinCandidates: (accountId: number) => invoke<JoinCandidate[]>("join_candidates", { accountId }),

  instancesList: () => invoke<Instance[]>("instances_list"),
  /** Graceful unless `force`; not counted as an unexpected exit. */
  instanceClose: (id: string, force: boolean) => invoke<void>("instance_close", { id, force }),
  instancesCloseAll: (force: boolean) => invoke<void>("instances_close_all", { force }),
  instancesClearEnded: () => invoke<void>("instances_clear_ended"),
  /** CPU cap, efficiency mode, trim interval and cores for one running client (they win over adaptive ones). */
  instanceSetLimits: (id: string, limits: Limits) => invoke<void>("instance_set_limits", { id, limits }),
  /** Pages the client's working set out now. */
  instanceTrim: (id: string) => invoke<void>("instance_trim", { id }),
  /** Opts one running client in or out of anti-AFK. */
  instanceSetAntiAfk: (id: string, enabled: boolean) => invoke<void>("instance_set_anti_afk", { id, enabled }),
  /** The client's preview at most maxW wide: always a 24-byte header, then RGBA unless its picture number is `known` or it has no picture (`window::Preview::to_bytes`). */
  instanceThumbnail: (id: string, maxW: number, known: number) => invoke<ArrayBuffer>("instance_thumbnail", { id, maxW, known }),
  /** Brings the client's window to the front, minimizes or restores it. */
  instanceShow: (id: string, what: WindowShow) => invoke<void>("instance_show", { id, what }),
  /** Minimizes or restores every running client's window. */
  instancesShowAll: (what: Exclude<WindowShow, "focus">) => invoke<void>("instances_show_all", { what }),
  /** Makes the client's window 320×240 (or puts it back); resolves to the client area it got, [w, h]. */
  instanceMicro: (id: string, on: boolean, size?: [number, number]) => invoke<[number, number]>("instance_micro", { id, on, size }),
  /** Lays every running client that isn't hidden or minimized out in a grid on RoALT's monitor; how many it laid out. */
  instancesTile: (micro: boolean) => invoke<number>("instances_tile", { micro }),
  /** CPU, memory and IO of each running client (sampled at most every 2 s). */
  instancesMetrics:() => invoke<ClientMetrics[]>("instances_metrics"),
  /** How busy the whole PC is (sampled at most every 2 s); null if Windows doesn't say. */
  systemUsage: () => invoke<SystemUsage | null>("system_usage"),
  /** Stops trying to bring this client back. */
  instanceStopRelaunch: (id: string) => invoke<void>("instance_stop_relaunch", { id }),
  /** Relaunches waiting to try again do so now. */
  instancesRetryRelaunch: () => invoke<void>("instances_retry_relaunch"),
  /** The Roblox bandwidth limit and whether it is in force now. */
  bandwidthStatus: () => invoke<BandwidthStatus>("bandwidth_status"),
  /** Sets the outbound limit for all Roblox clients (0 removes it); Windows asks for admin rights. */
  bandwidthApply: (kbps: number) => invoke<BandwidthStatus>("bandwidth_apply", { kbps }),
  multiInstanceStatus: () => invoke<MultiInstanceStatus>("multi_instance_status"),
  multiInstanceSet: (enabled: boolean) => invoke<MultiInstanceStatus>("multi_instance_set", { enabled }),
  /** Takes the lock from the running Roblox by closing its singleton event in each client (ask first). */
  multiInstanceTakeOver: () => invoke<MultiInstanceStatus>("multi_instance_take_over"),
  /** Every Roblox client, tracked or not; then takes the multi-instance lock if enabled. */
  robloxCloseAll: (force: boolean) => invoke<MultiInstanceStatus>("roblox_close_all", { force }),
  /** Panic button: cancels the batch, no rejoins or restarts, terminates every Roblox client. */
  panicKill: () => invoke<MultiInstanceStatus>("panic_kill"),
  /** Clients that crashed, were disconnected or hung, newest first (the last 200). */
  crashesList: () => invoke<Crash[]>("crashes_list"),
  /** A cleaned report to send when something goes wrong (`diagnostics`). */
  diagnosticsText: () => invoke<string>("diagnostics_text"),
  diagnosticsSave: (path: string) => invoke<void>("diagnostics_save", { path }),
  serverHistory: (accountId: number) => invoke<ServerVisit[]>("server_history", { accountId }),
  /** Opens that crash's client log in the default app. */
  crashOpenLog: (id: number) => invoke<void>("crash_open_log", { id }),

  /** Whether a Discord webhook is set (and its id's last digits; never the URL). */
  discordGet: () => invoke<DiscordStatus>("discord_get"),
  /** Stores a Discord webhook URL in the vault; null or "" removes it. */
  discordSet: (url: string | null) => invoke<DiscordStatus>("discord_set", { url }),
  /** Posts a test message to the webhook. */
  discordTest: () => invoke<void>("discord_test"),

  /** Starts a batch in the background; progress comes as bulk://progress. Returns its id. */
  bulkLaunch: (request: BulkRequest) => invoke<string>("bulk_launch", { request }),
  bulkCancel: (batchId: string) => invoke<void>("bulk_cancel", { batchId }),
  bulkCurrent: () => invoke<BulkProgress | null>("bulk_current"),
  serversList: (placeId: number, order: ServerOrder, excludeFull: boolean, limit: 10 | 25 | 50 | 100, cursor?: string, accountId?: number) =>
    invoke<ServerPage>("servers_list", { placeId, order, excludeFull, limit, cursor, accountId }),
  gameInfo: (placeId: number) => invoke<GameInfo>("game_info", { placeId }),
  /** The place a target leads to (universes, share links with the account's cookie). */
  targetPlace: (target: LaunchTarget, accountId?: number) => invoke<number>("target_place", { target, accountId }),
  privateServersList: (placeId: number, accountId: number, cursor?: string) =>
    invoke<PrivateServerPage>("private_servers_list", { placeId, accountId, cursor }),

  groupsList: () => invoke<Group[]>("groups_list"),
  groupSave: (input: GroupInput) => invoke<Group>("group_save", { input }),
  groupRemove: (id: number) => invoke<void>("group_remove", { id }),

  /** Master mode needs the master password (currentPassword); DPAPI mode shows Windows Hello. */
  /** The current performance settings as a profile (JSON text). */
  performanceProfile: (previewFps: number) => invoke<string>("performance_profile", { previewFps }),
  performanceExport: (path: string, previewFps: number) => invoke<void>("performance_export", { path, previewFps }),
  /** A profile file or text, checked; nothing is saved until it goes through settingsSet. */
  performanceRead: (path: string) => invoke<Profile>("performance_read", { path }),
  performanceParse: (text: string) => invoke<Profile>("performance_parse", { text }),
  backupExport: (path: string, password: string, currentPassword?: string, accountIds?: number[]) =>
    invoke<number>("backup_export", { path, password, currentPassword, accountIds }),
  backupImport: (path: string, password: string, onConflict: ConflictPolicy) =>
    invoke<ImportReport>("backup_import", { path, password, onConflict }),

  robloxClient: () => invoke<RobloxClient>("roblox_client"),
  /** Downloads Roblox's installer, checks it is signed by Roblox Corporation, runs it; progress on onRobloxInstall. */
  robloxInstall: () => invoke<RobloxClient>("roblox_install"),
  /** RoALT's version and the newer release the last check found (no network). */
  selfUpdateStatus: () => invoke<AppUpdate>("self_update_status"),
  /** Asks GitHub for a newer RoALT now. */
  selfUpdateCheck: () => invoke<AppUpdate>("self_update_check"),
  /** Downloads the newer RoALT, checks its signature, runs its installer and quits (the installer starts RoALT again). */
  selfUpdateInstall: () => invoke<void>("self_update_install"),
  dataDirInfo: () => invoke<DataDir>("data_dir_info"),
  dataDirOpen: () => invoke<void>("data_dir_open"),
  /** Copies the database to `path` and restarts RoALT to use it (resolves only on failure). */
  dataDirMove: (path: string) => invoke<void>("data_dir_move", { path }),
  /** RoALT's own log folder (data folder\logs) in Explorer. */
  logsOpen: () => invoke<void>("logs_open"),
  /** The last lines of roalt.log, oldest first. */
  debugTail: (lines: number) => invoke<string[]>("debug_tail", { lines }),
  backgroundSet: (path: string) => invoke<void>("background_set", { path }),
  backgroundClear: () => invoke<void>("background_clear"),
  backgroundGet: () => invoke<ArrayBuffer>("background_get"),
  /** Deletes Roblox client logs over a day old or 50 MB, never a running client's. */
  logsClean: () => invoke<LogsCleaned>("logs_clean"),
  /** Windows shuts down in `inSecs` or at `atMs` (1 min to 7 days); the deadline (Unix ms). */
  shutdownSchedule: (when: { atMs?: number; inSecs?: number }, closeClients: boolean) =>
    invoke<number | null>("shutdown_schedule", { atMs: when.atMs ?? null, inSecs: when.inSecs ?? null, closeClients }),
  shutdownCancel: () => invoke<number | null>("shutdown_cancel"),
  /** The deadline of the shutdown RoALT scheduled, or null. */
  shutdownStatus: () => invoke<number | null>("shutdown_status"),

  /** Whether a Public network is connected; app://network sends changes. */
  networkStatus: () => invoke<NetworkStatus>("network_status"),

  settingsGet: () => invoke<Settings>("settings_get"),
  settingsSet: (patch: SettingsPatch) => invoke<Settings>("settings_set", { patch }),
};

export const events = {
  onAccountsChanged: (f: () => void): Promise<UnlistenFn> => listen("accounts://changed", f),
  onVaultLocked: (f: () => void): Promise<UnlistenFn> => listen("vault://locked", f),
  onInstancesChanged: (f: () => void): Promise<UnlistenFn> => listen("instances://changed", f),
  onNotice: (f: (n: Notice) => void): Promise<UnlistenFn> => listen<Notice>("app://notice", (e) => f(e.payload)),
  onLog: (f: (r: LogRecord) => void): Promise<UnlistenFn> => listen<LogRecord>("log://log", (e) => f(e.payload)),
  onBulkProgress: (f: (p: BulkProgress) => void): Promise<UnlistenFn> => listen<BulkProgress>("bulk://progress", (e) => f(e.payload)),
  /** The client or its versions changed (version check at start and every 30 min, an install). */
  onRobloxClient: (f: (c: RobloxClient) => void): Promise<UnlistenFn> => listen<RobloxClient>("roblox://client", (e) => f(e.payload)),
  /** A newer RoALT is out (checked 10 s after start and every 6 h). */
  onAppUpdate: (f: (u: AppUpdate) => void): Promise<UnlistenFn> => listen<AppUpdate>("app://update", (e) => f(e.payload)),
  onRobloxInstall: (f: (p: InstallProgress) => void): Promise<UnlistenFn> => listen<InstallProgress>("roblox://install", (e) => f(e.payload)),
  /** Joined or left a Public network (checked every 10 s). */
  onNetwork: (f: (n: NetworkStatus) => void): Promise<UnlistenFn> => listen<NetworkStatus>("app://network", (e) => f(e.payload)),
};
