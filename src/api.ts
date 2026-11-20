// Typed wrappers for every Tauri command and event. Types come from src/bindings (ts-rs).
import { invoke as rawInvoke, type InvokeArgs } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AccountInfo } from "./bindings/AccountInfo";
import type { BrowseNav } from "./bindings/BrowseNav";
import type { GameSort } from "./bindings/GameSort";
import type { GameStats } from "./bindings/GameStats";
import type { AccountValue } from "./bindings/AccountValue";
import type { PlaytimeRow } from "./bindings/PlaytimeRow";
import type { PaneRect } from "./bindings/PaneRect";
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
import type { Benchmark } from "./bindings/Benchmark";
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
import type { FriendReport } from "./bindings/FriendReport";
import type { CookieExport } from "./bindings/CookieExport";
import type { InstallProgress } from "./bindings/InstallProgress";
import type { Instance } from "./bindings/Instance";
import type { JoinCandidate } from "./bindings/JoinCandidate";
import type { LaunchInfo } from "./bindings/LaunchInfo";
import type { LaunchTarget } from "./bindings/LaunchTarget";
import type { Limits } from "./bindings/Limits";
import type { LogsCleaned } from "./bindings/LogsCleaned";
import type { MultiInstanceStatus } from "./bindings/MultiInstanceStatus";
import type { Notice } from "./bindings/Notice";
import type { Online } from "./bindings/Online";
import type { ParsedInput } from "./bindings/ParsedInput";
import type { PrivateServerPage } from "./bindings/PrivateServerPage";
import type { PrivateServerBuy } from "./bindings/PrivateServerBuy";
import type { PrivateServerPatch } from "./bindings/PrivateServerPatch";
import type { PrivateServerSettings } from "./bindings/PrivateServerSettings";
import type { NetworkStatus } from "./bindings/NetworkStatus";
import type { News } from "./bindings/News";
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
export type { News } from "./bindings/News";
export type { NewsItem } from "./bindings/NewsItem";
export type { NewsSource } from "./bindings/NewsSource";
export type { Adaptive } from "./bindings/Adaptive";
export type { Profile } from "./bindings/Profile";
export type { SystemUsage } from "./bindings/SystemUsage";
export type { ServerVisit } from "./bindings/ServerVisit";
export type { AntiAfk } from "./bindings/AntiAfk";
export type { BulkItem } from "./bindings/BulkItem";
export type { BulkStage } from "./bindings/BulkStage";
export type { CrashKind } from "./bindings/CrashKind";
export type { Density } from "./bindings/Density";
export type { DiscordEvents } from "./bindings/DiscordEvents";
export type { GameServer } from "./bindings/GameServer";
export type { PrivateServer } from "./bindings/PrivateServer";
export type { AllowedUser } from "./bindings/AllowedUser";
export type { PrivateServerBuy, PrivateServerPatch, PrivateServerSettings };
export type { SameServer } from "./bindings/SameServer";
export type { InstanceState } from "./bindings/InstanceState";
export type { Limits } from "./bindings/Limits";
export type { Presence } from "./bindings/Presence";
export type { PresenceKind } from "./bindings/PresenceKind";
export type { Subscription } from "./bindings/Subscription";
export type { Theme } from "./bindings/Theme";
export type { BrowsePage } from "./bindings/BrowsePage";
export type { BrowsePlay } from "./bindings/BrowsePlay";
export type { BrowseBlocked } from "./bindings/BrowseBlocked";
export type { BrowseNav, PaneRect, GameSort, PlaytimeRow, GameStats, AccountValue };
export type { PopularGame } from "./bindings/PopularGame";
export type { GamePass } from "./bindings/GamePass";
export type { GameBadge } from "./bindings/GameBadge";
export type { LimitedItem } from "./bindings/LimitedItem";
export type { RolimonsItem } from "./bindings/RolimonsItem";
export type { AgeRating } from "./bindings/AgeRating";
export type { PlayerGame } from "./bindings/PlayerGame";
export type { PlayerGroup } from "./bindings/PlayerGroup";
export type { RobloxBadge } from "./bindings/RobloxBadge";
import type { PlayerStats } from "./bindings/PlayerStats";
import type { PlayerValue } from "./bindings/PlayerValue";
export type { PlayerStats, PlayerValue };
import type { PageCard } from "./bindings/PageCard";

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
type LogRecord = { level: number; message: string };

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
  /** Roblox's sign-in window; `fill` goes into its form (a bulk `user:pass` line), never stored. */
  accountAddLogin: (fill?: { username: string; password: string }) => invoke<AccountSummary>("account_add_login", { username: fill?.username, password: fill?.password }),
  /** Closes the open sign-in window: its accountAddLogin fails "cancelled". */
  accountLoginCancel: () => invoke<void>("account_login_cancel"),
  accountUpdate: (id: number, patch: AccountPatch) => invoke<AccountSummary>("account_update", { id, patch }),
  accountsReorder: (ids: number[]) => invoke<void>("accounts_reorder", { ids }),
  accountsRemove: (ids: number[]) => invoke<void>("accounts_remove", { ids }),
  accountsValidate: (ids?: number[]) => invoke<AccountSummary[]>("accounts_validate", { ids }),
  accountsBefriend: (ids: number[]) => invoke<FriendReport>("accounts_befriend", { ids }),
  /** Master mode needs the password; DPAPI mode shows Windows Hello. */
  accountRevealCookie: (id: number, password?: string) => invoke<string>("account_reveal_cookie", { id, password }),
  accountCopyCookie: (id: number, password?: string) => invoke<void>("account_copy_cookie", { id, password }),
  /** `username:cookie` lines in a file in RoALT's folder, selected in Explorer; all accounts with no ids. */
  accountsExportCookies: (accountIds?: number[], password?: string) => invoke<CookieExport>("accounts_export_cookies", { accountIds, password }),
  cookieExportDelete: () => invoke<number>("cookie_export_delete"),
  cookieExportReveal: () => invoke<void>("cookie_export_reveal"),

  /** Cached for settings.cacheMinutes unless refresh. Parts Roblox didn't answer are null and named in `unavailable`. */
  accountInfo: (id: number, refresh = false) => invoke<AccountInfo>("account_info", { id, refresh }),
  /** The account's limiteds as last counted (null: never); `refresh` counts them now (slow). */
  accountValue: (id: number, refresh = false) => invoke<AccountValue | null>("account_value", { id, refresh }),
  /** Avatar URLs for all accounts (cached 6 h); accounts without one are missing from the list. */
  accountsHeadshots: (refresh = false) => invoke<Headshot[]>("accounts_headshots", { refresh }),
  /** Presence of these accounts, each asked with its own cookie (accounts without a valid cookie are left out). */
  accountsPresence: (ids: number[]) => invoke<AccountPresence[]>("accounts_presence", { ids }),
  /** Opens roblox.com signed in as the account in its own window. */

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
  /** Browse: www.roblox.com/`path` as the account, at `rect` (the main window's CSS pixels). */
  browseOpen: (accountId: number, path: string, rect: PaneRect) => invoke<void>("browse_open", { accountId, path, rect }),
  browseBounds: (rect: PaneRect, shown: boolean) => invoke<void>("browse_bounds", { rect, shown }),
  browseNav: (action: BrowseNav) => invoke<void>("browse_nav", { action }),
  browseClose: () => invoke<void>("browse_close"),
  /** Roblox's Charts lists with icons (Browse's Popular; cached). */
  gamesPopular: () => invoke<GameSort[]>("games_popular"),
  /** Every account's time in each place, the most played first. */
  playtimeList: () => invoke<PlaytimeRow[]>("playtime_list"),
  /** Closes a playing account's client and launches it again into `target` (the smallest other
   * server when that is the game it is in). `fpsCap` as `launch`'s; none: the old client's. */
  instanceMove: (id: string, target: LaunchTarget, favouriteId?: number, fpsCap?: number | null) =>
    invoke<LaunchInfo>("instance_move", { id, target, favouriteId, fpsCap: fpsCap || null }),
  instancesCloseAll: (force: boolean) => invoke<void>("instances_close_all", { force }),
  instancesClearEnded: () => invoke<void>("instances_clear_ended"),
  /** CPU cap, efficiency mode, trim interval and cores for one running client (they win over adaptive ones). */
  instanceSetLimits: (id: string, limits: Limits) => invoke<void>("instance_set_limits", { id, limits }),
  /** Pages the client's working set out now. */
  /** Trims the client: its Memory column before and after, in MB. */
  instanceTrim: (id: string) => invoke<[number, number]>("instance_trim", { id }),
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
  /** Draws the window layout's spots for launching these accounts (in order) over RoALT's monitor for a few seconds. */
  layoutPreview: (accountIds: number[], running: string) => invoke<void>("layout_preview", { accountIds, running }),
  /** Takes the layout preview off the screen before its time. */
  layoutPreviewStop: () => invoke<void>("layout_preview_stop"),
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
  gameStats: (universeId: number) => invoke<GameStats>("game_stats", { universeId }),
  /** Shows the profile card on Browse's page (null: takes it away). */
  browseCard: (card: PageCard | null) => invoke<void>("browse_card", { card }),
  /** A Roblox player's profile, asked as `accountId` (the Browse account). */
  playerStats: (accountId: number, userId: number) => invoke<PlayerStats>("player_stats", { accountId, userId }),
  /** A player's limiteds: a recent count kept, else counted now (slow for big inventories). */
  playerValue: (accountId: number, userId: number, refresh = false) => invoke<PlayerValue>("player_value", { accountId, userId, refresh }),
  /** The place a target leads to (universes, share links with the account's cookie). */
  targetPlace: (target: LaunchTarget, accountId?: number) => invoke<number>("target_place", { target, accountId }),
  privateServersList: (placeId: number, accountId: number, cursor?: string) =>
    invoke<PrivateServerPage>("private_servers_list", { placeId, accountId, cursor }),
  /** As the owner's account (`accountId`); another account gets "Only the owner's account can change this". */
  privateServerGet: (accountId: number, vipServerId: number) =>
    invoke<PrivateServerSettings>("private_server_get", { accountId, vipServerId }),
  privateServerUpdate: (accountId: number, vipServerId: number, patch: PrivateServerPatch) =>
    invoke<PrivateServerSettings>("private_server_update", { accountId, vipServerId, patch }),
  /** Auto-renew on spends `price` Robux a month: confirm first. `price` comes from privateServerGet. */
  privateServerRenew: (accountId: number, vipServerId: number, on: boolean, price: number) =>
    invoke<PrivateServerSettings>("private_server_renew", { accountId, vipServerId, on, price }),
  /** Spends Robux: confirm first. One idempotencyKey per buy dialog. */
  privateServerBuy: (buy: PrivateServerBuy) => invoke<void>("private_server_buy", { buy }),

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
  /** Watches the clients in a game for about 22 s, then what to turn on for `target` clients like them (default: as many). */
  performanceBenchmark: (target?: number) => invoke<Benchmark>("performance_benchmark", { target }),
  backupExport: (path: string, password: string, currentPassword?: string, accountIds?: number[]) =>
    invoke<number>("backup_export", { path, password, currentPassword, accountIds }),
  backupImport: (path: string, password: string, onConflict: ConflictPolicy) =>
    invoke<ImportReport>("backup_import", { path, password, onConflict }),

  robloxClient: () => invoke<RobloxClient>("roblox_client"),
  /** Downloads Roblox's installer, checks it is signed by Roblox Corporation, runs it; progress on onRobloxInstall. */
  robloxInstall: () => invoke<RobloxClient>("roblox_install"),
  /** RoALT's version and the newer release the last check found (no network). */
  news: () => invoke<News>("news"),
  onlinePing: () => invoke<Online | null>("online_ping"),
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
  browserDataReset: () => invoke<boolean>("browser_data_reset"),
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
