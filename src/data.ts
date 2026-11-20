// TanStack Query wiring: one cache, keys, and the hooks the views share.
import { useCallback, useMemo } from "react";
import { MutationCache, QueryClient, queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, type AccountPatch, type AccountPresence, type AccountSummary, type Group, type Headshot, type Settings, type SettingsPatch } from "./api";
import type { Online } from "./bindings/Online";
import { toastError } from "./ui/Toast";
import { t } from "./i18n";

export const qk = {
  vault: ["vault"],
  settings: ["settings"],
  accounts: ["accounts"],
  groups: ["groups"],
  headshots: ["headshots"],
  presence: ["presence"],
  info: (id: number) => ["info", id],
  favourites: ["favourites"],
  join: ["join"],
  instances: ["instances"],
  multi: ["multi"],
  crashes: ["crashes"],
  serverHistory: (id: number) => ["serverHistory", id] as const,
  metrics: ["metrics"],
  system: ["system"],
  bandwidth: ["bandwidth"],
  discord: ["discord"],
  bulk: ["bulk"],
  game: (placeId: number | null) => ["game", placeId],
  gameStats: (universeId: number) => ["gameStats", universeId],
  player: (userId: number) => ["player", userId],
  // By viewer too: who may see an inventory depends on who asks (a friend may).
  playerValue: (accountId: number, userId: number) => ["playerValue", accountId, userId],
  values: ["value"],
  /** Browse › Limiteds (Rolimons' list; none while Rolimons values are off). */
  market: ["market"],
  mods: ["mods"],
  robux: (accountId: number, kind: string) => ["robux", accountId, kind],
  robuxTotals: (accountId: number) => ["robuxTotals", accountId],
  value: (id: number) => ["value", id],
  servers: ["servers"],
  privateServers: ["privateServers"],
  client: ["client"],
  launchers: ["launchers"],
  appUpdate: ["appUpdate"],
  dataDir: ["dataDir"],
  shutdown: ["shutdown"],
  background: ["background"],
  network: ["network"],
  news: ["news"],
  extras: ["extras"],
  online: ["online"],
} as const;

export const queryClient = new QueryClient({
  // Data only changes through our own mutations and backend events, which invalidate.
  defaultOptions: { queries: { staleTime: Infinity, retry: false, refetchOnWindowFocus: false } },
  // Every failed mutation toasts unless it handles its own errors (meta.quiet).
  mutationCache: new MutationCache({
    onError: (e, _vars, _res, m) => {
      if (!m.meta?.quiet) toastError(e);
    },
  }),
});

const bySortOrder = <T extends { sortOrder: number }>(list: T[]) => [...list].sort((a, b) => a.sortOrder - b.sortOrder);

export const useVault = () => useQuery({ queryKey: qk.vault, queryFn: api.vaultStatus });
export const useSettings = () => useQuery({ queryKey: qk.settings, queryFn: api.settingsGet });
/** The MAIN account first, then the user's order (as the backend lists them). */
const mainFirst = (list: AccountSummary[]) => bySortOrder(list).sort((a, b) => Number(b.main) - Number(a.main));
export const useAccounts = () => useQuery({ queryKey: qk.accounts, queryFn: api.accountsList, select: mainFirst });
export const useGroups = () => useQuery({ queryKey: qk.groups, queryFn: api.groupsList, select: bySortOrder });

/** `a` as its clients are run: what it doesn't set itself comes from the first of its groups, in the
 * list's order, that sets it (the backend's `with_group_defaults`). An account's own switch only
 * counts when it is on, and limits it keeps of its own win over a group's. */
export function withGroupDefaults(a: AccountSummary, groups: Group[]): AccountSummary {
  const mine = groups.filter((g) => a.groupIds.includes(g.id)).sort((x, y) => x.sortOrder - y.sortOrder || x.id - y.id);
  const first = <T,>(of: (d: Group["defaults"]) => T | null): T | null => {
    for (const g of mine) {
      const v = of(g.defaults);
      if (v !== null) return v;
    }
    return null;
  };
  return {
    ...a,
    fullQuality: a.fullQuality || first((x) => x.fullQuality) === true,
    discordMuted: a.discordMuted || first((x) => x.discordMuted) === true,
    allowDisconnect: a.allowDisconnect || first((x) => x.allowDisconnect) === true,
    fpsCap: a.fpsCap ?? first((x) => x.fpsCap),
    launchArgs: a.launchArgs ?? first((x) => x.launchArgs),
    clientLimits: a.clientLimits ?? first((x) => x.client),
  };
}

/** The accounts as their clients are run (`withGroupDefaults`): for what the UI says a launch or a
 * client does. Lists and editors show an account's own values (`useAccounts`). */
export function useRunAccounts() {
  const accounts = useAccounts().data;
  const groups = useGroups().data;
  return useMemo(() => accounts?.map((a) => withGroupDefaults(a, groups ?? [])), [accounts, groups]);
}
export const useFavourites = () => useQuery({ queryKey: qk.favourites, queryFn: api.favouritesList, select: bySortOrder });

/** Refreshed by instances://changed, and every 5 s in case an event was missed. */
export const useInstances = () => useQuery({ queryKey: qk.instances, queryFn: api.instancesList, refetchInterval: 5_000 });
/** Refreshed by instances://changed (an ending client may add one). */
export const useCrashes = () => useQuery({ queryKey: qk.crashes, queryFn: api.crashesList });
/** Polled every 2 s while a caller is mounted and the window is visible. */
export const useMetrics = () =>
  useQuery({ queryKey: qk.metrics, queryFn: api.instancesMetrics, staleTime: 0, refetchInterval: 2000, refetchIntervalInBackground: false });
/** How busy the whole PC is; polled like the metrics. */
export const useSystemUsage = (enabled: boolean) =>
  useQuery({ queryKey: qk.system, queryFn: api.systemUsage, enabled, staleTime: 0, refetchInterval: 2000, refetchIntervalInBackground: false });
/** While the lock is wanted but not held, asks again every 5 s (Roblox may have been closed);
 * so too after a failed read, which would otherwise stay unknown. */
export const useMultiStatus = () =>
  useQuery({
    queryKey: qk.multi,
    queryFn: api.multiInstanceStatus,
    refetchInterval: (q) => (q.state.status === "error" || (q.state.data?.enabled && !q.state.data.active) ? 5000 : false),
    refetchIntervalInBackground: false,
  });

/** Whether a Public network is connected; app://network replaces it. */
export const useNetwork = () => useQuery({ queryKey: qk.network, queryFn: api.networkStatus });

/** Why launching is refused on this network (`network::blocked`), or null. */
export function publicBlock(network: { public: boolean } | undefined, settings: { allowPublicNetworks: boolean } | undefined) {
  if (!network?.public || settings?.allowPublicNetworks !== false) return null;
  return t("app.publicBlocked");
}

/** A place's game (name, icon), when `placeId` is known; the backend keeps its answers. */
export const useGame = (placeId: number | null) =>
  useQuery({ queryKey: qk.game(placeId ?? 0), queryFn: () => api.gameInfo(placeId!), enabled: placeId !== null, staleTime: 10 * 60_000, retry: false });

/** Home's News: fetched at start, then every 30 minutes while RoALT runs; only while `enabled` (Settings' news). */
export const useNews = (enabled: boolean) => useQuery({ queryKey: qk.news, queryFn: api.news, enabled, staleTime: 30 * 60_000, refetchInterval: 30 * 60_000 });

/** RoALT's sponsor and supporters (roalt.dev), asked only while `enabled`: News with Settings'
 * showSponsor on, or Settings › About open. They change rarely: kept 6 hours. */
export const useExtras = (enabled: boolean) => useQuery({ queryKey: qk.extras, queryFn: api.extras, enabled, staleTime: 6 * 3_600_000 });

/** Ms until the next ping: as the counter asks (1 to 60 minutes). Without an answer every 5 minutes, the counter's own pace: a counter in trouble must not be pinged more often. */
const pingGap = (q: { state: { data: Online | null | undefined } }) => Math.min(3600, Math.max(60, q.state.data?.every ?? 300)) * 1000;

/** How many people use RoALT now, and their accounts playing: pinged as often as the counter asks while `enabled`
 * (Settings' onlineCount), also hidden or in the tray, since the ping is what counts this run. */
export const useOnline = (enabled: boolean) =>
  useQuery({ queryKey: qk.online, queryFn: api.onlinePing, enabled, staleTime: pingGap, refetchInterval: pingGap, refetchIntervalInBackground: true });

/** The Roblox client and its versions; roblox://client replaces it. */
export const useRobloxClient = () => useQuery({ queryKey: qk.client, queryFn: api.robloxClient });

/** The running or last batch; bulk://progress replaces it, null once dismissed. */
// A dismissed batch stays hidden when the query is fetched again (bulk_current keeps the last).
let dismissedBulk: string | null = null;
export function dismissBulk(qc: QueryClient, batchId: string) {
  dismissedBulk = batchId;
  qc.setQueryData(qk.bulk, null);
}
export const useBulk = () =>
  useQuery({ queryKey: qk.bulk, queryFn: () => api.bulkCurrent().then((p) => (p && p.batchId === dismissedBulk ? null : p)) });

const headshots = queryOptions({ queryKey: qk.headshots, queryFn: () => api.accountsHeadshots(), staleTime: 6 * 3_600_000 });
/** One shared query; each avatar picks its own URL out of it. */
// A stable select: react-query reuses its result on re-render, so the find runs only when the list changes.
export const useHeadshot = (accountId: number) =>
  useQuery({ ...headshots, select: useCallback((list: Headshot[]) => list.find((h) => h.accountId === accountId)?.url, [accountId]) }).data;

// The one object URL for the picture (window and Settings thumbnail), and its still frame when it may
// move; a new picture revokes the old ones.
let bgUrl: string | null = null;
let bgStill: string | null = null;
/** A GIF or WebP, by its first bytes: it may be animated. */
const mayMove = (b: Uint8Array) =>
  (b[0] === 0x47 && b[1] === 0x49 && b[2] === 0x46) || (b[0] === 0x52 && b[1] === 0x49 && b[2] === 0x46 && b[3] === 0x46 && b[8] === 0x57 && b[9] === 0x45 && b[10] === 0x42 && b[11] === 0x50);
/** Its first frame, as a still PNG; null when it can't be drawn (the moving one then stays). */
async function stillOf(blob: Blob) {
  try {
    const frame = await createImageBitmap(blob);
    const canvas = new OffscreenCanvas(frame.width, frame.height);
    canvas.getContext("2d")!.drawImage(frame, 0, 0);
    frame.close();
    return URL.createObjectURL(await canvas.convertToBlob({ type: "image/png" }));
  } catch {
    return null;
  }
}
const background = queryOptions({
  queryKey: qk.background,
  queryFn: async () => {
    const bytes = await api.backgroundGet();
    for (const u of [bgUrl, bgStill]) if (u) URL.revokeObjectURL(u);
    const blob = bytes.byteLength ? new Blob([bytes]) : null;
    bgUrl = blob && URL.createObjectURL(blob);
    bgStill = blob && mayMove(new Uint8Array(bytes, 0, Math.min(12, bytes.byteLength))) ? await stillOf(blob) : null;
    return { url: bgUrl, still: bgStill };
  },
});
/** The background picture as an object URL, null without one; invalidating qk.background reloads it. */
export const useBackgroundUrl = (enabled = true) => useQuery({ ...background, enabled }).data?.url ?? null;
/** Its first frame when it may move (a GIF or WebP), else null. */
export const useBackgroundStill = (enabled = true) => useQuery({ ...background, enabled }).data?.still ?? null;

const byAccount = (list: AccountPresence[]) => new Map(list.map((p) => [p.accountId, p.presence]));
/** Presence of the given (on-screen, valid) accounts, polled once a minute while a caller is
 * mounted and the window is visible. Each account asks about itself, so keep the list short. */
export const usePresence = (ids: number[]) =>
  useQuery({
    queryKey: [...qk.presence, ids],
    queryFn: () => api.accountsPresence(ids),
    select: byAccount,
    enabled: ids.length > 0,
    staleTime: 60_000,
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
    placeholderData: (prev) => prev,
  });

/** The backend caches info for settings.cacheMinutes (presence inside it refreshes at most once
 * a minute), so asking again on open and once a minute while shown is cheap. */
export const useAccountInfo = (id: number) =>
  useQuery({
    queryKey: qk.info(id),
    queryFn: () => api.accountInfo(id),
    staleTime: 0,
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
  });

/** An account's limiteds as last counted (null: never). Reading never asks Roblox; a count is a
 * mutation (`api.accountValue(id, true)`) that puts its answer here. */
export const accountValueQuery = (id: number) => queryOptions({ queryKey: qk.value(id), queryFn: () => api.accountValue(id) });
export const useAccountValue = (id: number) => useQuery(accountValueQuery(id));

export function setAccounts(qc: ReturnType<typeof useQueryClient>, f: (list: AccountSummary[]) => AccountSummary[]) {
  qc.setQueryData<AccountSummary[]>(qk.accounts, (old) => (old ? f(old) : old));
}

/** Optimistic account edit; the server's copy replaces it on success. */
/** `quiet`: the caller shows a failure itself, so it doesn't toast. */
export function useUpdateAccount(opts: { quiet?: boolean } = {}) {
  const qc = useQueryClient();
  return useMutation({
    meta: opts.quiet ? { quiet: true } : undefined,
    mutationFn: ({ id, patch }: { id: number; patch: AccountPatch }) => api.accountUpdate(id, patch),
    onMutate: ({ id, patch }) =>
      setAccounts(qc, (list) =>
        list.map((a) =>
          a.id === id
            ? { ...a, ...patch, color: patch.color === undefined ? a.color : patch.color || null }
            : patch.main && a.main // one MAIN: the old one loses it
              ? { ...a, main: false }
              : a,
        ),
      ),
    onSuccess: (acc) => setAccounts(qc, (list) => list.map((a) => (a.id === acc.id ? acc : a))),
    onError: () => qc.invalidateQueries({ queryKey: qk.accounts }),
  });
}

/** Optimistic settings change (theme and density apply at once). `quiet`: the caller shows a failure
 * itself (beside the control), so it doesn't toast. */
export function useSetSettings(opts: { quiet?: boolean } = {}) {
  const qc = useQueryClient();
  return useMutation({
    mutationKey: qk.settings,
    meta: opts.quiet ? { quiet: true } : undefined,
    mutationFn: (patch: SettingsPatch) => api.settingsSet(patch),
    onMutate: (patch) => qc.setQueryData<Settings>(qk.settings, (s) => (s ? { ...s, ...patch } : s)),
    onSuccess: (s) => {
      // An earlier save's answer would undo the optimistic value of a later one still on its way.
      if (qc.isMutating({ mutationKey: qk.settings }) === 1) qc.setQueryData(qk.settings, s);
      qc.invalidateQueries({ queryKey: qk.vault });
    },
    onError: () => qc.invalidateQueries({ queryKey: qk.settings }),
  });
}

export function useSaveGroup() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: api.groupSave,
    onSuccess: (g: Group) =>
      qc.setQueryData<Group[]>(qk.groups, (list) => (list ? [...list.filter((x) => x.id !== g.id), g] : list)),
  });
}

export const groupColor = (g: Group) => g.color ?? "var(--fg-3)";
