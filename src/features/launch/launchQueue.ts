// The launch engine every launch path shares (Play, Home's tabs and cards, Servers, Recently closed,
// an account's recent servers): one launch at a time, the session FPS cap, and one view of the launch
// under way (useLaunchActivity). Owned by Play; the API here is what the other views build on.
import { useEffect, useSyncExternalStore, type Dispatch, type SetStateAction } from "react";
import { api, asError, type AccountSummary, type BulkProgress, type BulkStage, type Instance, type LaunchInfo, type LaunchTarget, type MultiInstanceStatus, type SameServer, type Settings } from "../../api";
import { dismissBulk, qk, queryClient, useAccounts, useBulk, useInstances } from "../../data";
import { fmtNum, t, type Key } from "../../i18n";
import { friendly, toast, toastError } from "../../ui/Toast";
import { accountName } from "../accounts/shared";
import { describeTarget, resolveTarget, type Draft } from "./target";

const LAST_KEY = "roalt.lastLaunchAccount";
const LAUNCH_ERRORS: Record<string, Key> = {
  cookieInvalid: "launch.err.cookieInvalid",
  challenge: "launch.err.challenge",
  notFound: "launch.err.notFound",
};

/** The account launched last, which Play picks until something else is picked. */
export function lastLaunchAccount() {
  try {
    return Number(localStorage.getItem(LAST_KEY)) || null;
  } catch {
    return null;
  }
}
function remember(id: number) {
  try {
    localStorage.setItem(LAST_KEY, String(id));
  } catch {
    // Only a convenience.
  }
}

/** Why a launch can't start now: launches go one at a time (a running batch blocks single
 * launches too), and several accounts need multi-instance. Null when it can. */
export function launchBlocked(several: boolean, multi: MultiInstanceStatus | undefined, batch: BulkProgress | null | undefined) {
  if (batch && !batch.done) return t("launch.batchRunning");
  // Only a known status blocks: before it's read (or when reading it failed) the backend decides.
  if (several && multi && !multi.active) return (multi.enabled && multi.message) || t("launch.needMulti");
  return null;
}

/** Toasts a started client and remembers its account for the next start. */
export function launched(a: AccountSummary, info: LaunchInfo) {
  remember(a.id);
  toast(t(info.viaBootstrapper ? "launch.launchedViaBootstrapper" : "launch.launched", { name: accountName(a) }));
  if (info.warning) toast(info.warning, "error");
}

const failText = (e: unknown) => {
  const err = asError(e);
  return LAUNCH_ERRORS[err.kind] ? t(LAUNCH_ERRORS[err.kind]) : friendly(err);
};

/** Toasts why a launch failed, in words for the common cases; a cancelled one stays quiet. */
export function launchFailed(e: unknown) {
  if (asError(e).kind !== "cancelled") toast(failText(e), "error");
}

/** Where a launch goes: a known target, or a draft (parsed again if it hasn't caught up, its
 * username looked up once, its favourite marked used). */
export type Where = LaunchTarget | { draft: Draft; setDraft: Dispatch<SetStateAction<Draft>> };
const favOf = (w: Where) => ("draft" in w ? w.draft.favId : null);

// ---- State: one launch at a time, and the launch started here (local) on show ----

type Row = { accountId: number; userId: number; stage: BulkStage; at: number | null; instanceId: string | null };
type Local = { id: string; rows: Row[]; target: LaunchTarget | null; running: boolean; stopped: string | null; cancel: boolean };
type State = { step: string | null; fps: number; local: Local | null; cancelling: string | null; arriving: boolean };

let state: State = { step: null, fps: 0, local: null, cancelling: null, arriving: false };
const subs = new Set<() => void>();
function set(patch: Partial<State>) {
  state = { ...state, ...patch };
  subs.forEach((f) => f());
}
const subscribe = (f: () => void) => {
  subs.add(f);
  return () => void subs.delete(f);
};
const useStore = () => useSyncExternalStore(subscribe, () => state);

/** A launch from before the vault locked stops touching anything (resetLaunches bumps it). */
let gen = 0;
let seq = 0;

function patchLocal(id: string, f: (l: Local) => Partial<Local>) {
  const l = state.local;
  if (l?.id === id) set({ local: { ...l, ...f(l) } });
}
const setRow = (id: string, n: number, patch: Partial<Row>) =>
  patchLocal(id, (l) => ({ rows: l.rows.map((r, i) => (i === n ? { ...r, ...patch } : r)) }));
/** The rest from row `from` on won't start. */
const stopRest = (id: string, from: number, stopped: string | null) =>
  patchLocal(id, (l) => ({ stopped, rows: l.rows.map((r, i) => (i >= from && r.stage.kind === "queued" ? { ...r, stage: { kind: "cancelled" } } : r)) }));
const clearLocal = (id: string) => state.local?.id === id && set({ local: null });
const bulk = () => queryClient.getQueryData<BulkProgress | null>(qk.bulk);

type Job = { account: AccountSummary; where: Where; fpsCap?: number | null };

/** Launches one after another, each with its own target. A refusal stops the rest, and so do a
 * cancelled launch, a lock, a batch that started meanwhile (the tray starts them) and Cancel. */
async function runLocal(jobs: Job[]) {
  if (state.step !== null || !jobs.length) return;
  const g = gen;
  const current = () => g === gen;
  const step = (s: string | null) => current() && set({ step: s });
  const id = `local:${++seq}`;
  // A finished batch on show gives way to this launch.
  const p = bulk();
  if (p?.done) dismissBulk(queryClient, p.batchId);
  const only = jobs.length === 1 && !("draft" in jobs[0].where) ? (jobs[0].where as LaunchTarget) : null;
  set({
    step: t("launch.step.ticket"),
    cancelling: null,
    local: { id, target: only, running: true, stopped: null, cancel: false, rows: jobs.map(({ account: a }) => ({ accountId: a.id, userId: a.userId, stage: { kind: "queued" }, at: null, instanceId: null })) },
  });
  let favourite = false;
  try {
    for (const [n, { account: a, where, fpsCap }] of jobs.entries()) {
      if (!current()) return;
      const running = bulk();
      const busy = !!running && !running.done;
      const list = queryClient.getQueryData<AccountSummary[]>(qk.accounts);
      const gone = !!list && !list.some((x) => x.id === a.id && x.userId === a.userId);
      if (state.local?.id !== id || state.local.cancel || busy || gone) {
        if (busy) toast(t("launch.batchRunning"), "error");
        stopRest(id, n, busy ? t("launch.batchRunning") : null);
        return;
      }
      setRow(id, n, { stage: { kind: "ticket" } });
      step(t("launch.step.ticket"));
      let asked = false;
      let timer = 0;
      try {
        const target = "draft" in where ? await resolveTarget(where.draft, where.setDraft, (name) => step(t("launch.step.lookup", { name }))) : where;
        if (!current()) return;
        asked = true;
        if (jobs.length === 1) patchLocal(id, () => ({ target }));
        step(t("launch.step.ticket"));
        timer = window.setTimeout(() => step(t("launch.step.starting")), 800);
        // Its client is the account's first one that starts after this (a bootstrapper changes the pid).
        setRow(id, n, { at: Date.now() });
        const info = await api.launch(a.id, target, favOf(where) ?? undefined, fpsCap);
        if (!current()) return;
        launched(a, info);
        setRow(id, n, { stage: { kind: "starting" } });
        favourite ||= favOf(where) !== null;
      } catch (e) {
        if (!current()) return;
        launchFailed(e);
        const { kind } = asError(e);
        // The backend refusing: mostly Kill all, or an auto-rejoin or another launch under way. They
        // have no kind of their own, and the rest must not go on starting. A dead share link reads
        // the same, so it stops the rest too; its toast says why.
        const refused = asked && kind === "invalid";
        setRow(id, n, { stage: kind === "cancelled" ? { kind: "cancelled" } : { kind: "failed", message: failText(e) } });
        if (refused || kind === "cancelled" || kind === "locked") {
          stopRest(id, n + 1, kind === "cancelled" ? null : failText(e));
          return;
        }
      } finally {
        clearTimeout(timer);
      }
    }
  } finally {
    if (current()) {
      patchLocal(id, () => ({ running: false, cancel: false }));
      step(null);
    }
    if (favourite) void queryClient.invalidateQueries({ queryKey: qk.favourites });
  }
}

/** One account (Play, a Home tab, Play again, Rejoin). `fpsCap`: this launch's own (Play passes
 * sessionFps(), Play again the client's own); none: the account's, else the default. */
export const launchOne = (account: AccountSummary, where: Where, fpsCap?: number | null) => runLocal([{ account, where, fpsCap }]);

/** One after another, each account to its own target (Home's "Play with N", Play all again): a batch
 * has one target. The backend keeps the launch delay between them. `fpsCap`: as launchOne's. */
export const launchEach = (list: Job[]) => runLocal(list);

/** A backend batch: several accounts into one target, in the order given (the Accounts list's).
 * The launch delay is read now, so one committed by this click's blur counts. */
export async function launchSame(accounts: AccountSummary[], where: Where, opts: { sameServer?: SameServer; fpsCap?: number | null } = {}) {
  if (state.step !== null || !accounts.length) return;
  const g = gen;
  const step = (s: string | null) => g === gen && set({ step: s });
  step(t("launch.step.batch"));
  try {
    const target = "draft" in where ? await resolveTarget(where.draft, where.setDraft, (name) => step(t("launch.step.lookup", { name }))) : where;
    if (g !== gen) return;
    step(t("launch.step.batch"));
    const delayMs = queryClient.getQueryData<Settings>(qk.settings)?.launchDelayMs ?? 6000;
    const batchId = await api.bulkLaunch({ accountIds: accounts.map((a) => a.id), target, delayMs, sameServer: opts.sameServer ?? "off", favouriteId: favOf(where), fpsCap: opts.fpsCap || null });
    if (g !== gen) return;
    // On show before this returns, even if its first bulk://progress is still on the way.
    const now = bulk()?.batchId === batchId ? null : await api.bulkCurrent().catch(() => null);
    if (g !== gen) return;
    if (now?.batchId === batchId && bulk()?.batchId !== batchId) queryClient.setQueryData(qk.bulk, now);
    set({ local: null });
    if (favOf(where) !== null) void queryClient.invalidateQueries({ queryKey: qk.favourites });
  } catch (e) {
    if (g === gen) launchFailed(e);
  } finally {
    step(null);
  }
}

/** What the launch under way is doing ("Getting a ticket…"); null when none is. Launches go one at a
 * time: while it is set, launchOne, launchSame and launchEach do nothing. */
export const useLaunchStep = () => useSyncExternalStore(subscribe, () => state.step);
/** The same, read once (LaunchButton: did its press start a launch?). */
export const launchStep = () => state.step;

/** The FPS cap typed in Play for this session's launches (0: each account's own, else the default). */
export const sessionFps = () => state.fps;
const setSessionFps = (fps: number) => set({ fps });
export const useSessionFps = (): [number, (fps: number) => void] => [useSyncExternalStore(subscribe, () => state.fps), setSessionFps];

/** LaunchButton: its pill is on its way to the island; until it lands useLaunchActivity is null. */
export function holdIsland(on: boolean) {
  if (state.arriving !== on) set({ arriving: on });
}

/** Forgets this session's launches when the vault locks (a target can carry a private server's code):
 * the launch on show from here, the session FPS cap, the guard. A launch still on its way stops. */
export function resetLaunches() {
  gen++;
  set({ step: null, fps: 0, local: null, cancelling: null, arriving: false });
}

// ---- The view of the launch on show ----

export type LaunchRow = {
  accountId: number;
  userId: number;
  /** The account the launch was for (account ids are reused once one is removed). */
  account: AccountSummary | undefined;
  stage: BulkStage;
  instanceId: string | null;
  /** The server it is in, once in game. */
  job: string | null;
};
export type LaunchActivity = {
  /** A batch's id, or "local:n"; a new launch has a new one. */
  id: string;
  /** A backend batch (one target), or launches made one by one from here. */
  kind: "batch" | "local";
  /** "Batch", or for one account its target's kind ("Game"). */
  title: string;
  /** "Game · same server · 6 s", or one account's target ("place 1818"). */
  summary: string;
  rows: LaunchRow[];
  /** In game, or done (an account allowed to disconnect that did). */
  ok: number;
  failed: number;
  /** No more launches start; rows still follow their clients. */
  done: boolean;
  /** Why it stopped early; null otherwise. */
  stopped: string | null;
  /** Something to know (no server had room for everyone, ...). */
  note: string | null;
  /** Valid accounts that never started (cancelled, or failed before a client ran): "Pick the N…". */
  rest: number[];
  /** The row under way, for "Alt 02 · joining server". */
  current: LaunchRow | null;
  cancelling: boolean;
  cancel: () => void;
  dismiss: () => void;
};

/** A started client's stage from its tracked instance (as the backend mirrors it), so rows keep
 * moving after the launch itself stopped. `allowDisconnect`: its account's setting. */
function stageOf(i: Instance, allowDisconnect: boolean): BulkStage {
  // As the backend's mirror_item: these notes fail the item whatever the state.
  if (i.signInProblem) return { kind: "failed", message: i.note ?? "" };
  switch (i.state) {
    case "starting":
    case "joining":
    case "inGame":
      return { kind: i.state };
    case "disconnected":
      return allowDisconnect ? { kind: "done" } : { kind: "failed", message: i.note ?? t("launch.batch.disconnected") };
    case "exited":
      return { kind: "failed", message: i.note ?? t("launch.batch.closed") };
  }
}

const isIn = (s: BulkStage) => s.kind === "inGame" || s.kind === "done";
const UNDER_WAY = new Set<BulkStage["kind"]>(["ticket", "starting", "joining"]);
/** A running batch wins; else the launch from here (`here`), which dismissed any finished batch it followed. */
const shown = (p: BulkProgress | null | undefined, here: boolean) => (p && (!p.done || !here) ? p : null);

/** The id of the launch on show, read once (LaunchButton: is a new one on show yet?). */
export const launchActivityId = () => shown(bulk(), !!state.local)?.batchId ?? state.local?.id ?? null;
/** The same, kept current. */
export function useLaunchActivityId() {
  const local = useSyncExternalStore(subscribe, () => state.local?.id ?? null);
  return shown(useBulk().data, local !== null)?.batchId ?? local;
}

/** The launch on show, whichever way it was started: a backend batch (Play, Servers, the tray) or
 * launches made from here (one account, or Home's tabs one after another). Null when none is, and
 * while a LaunchButton's pill is still on its way to the island (so what shows it appears as the pill
 * lands). A launch from here that gets every client in game clears itself after 4 s; one with a
 * failure, and a batch, stay until dismissed. */
export function useLaunchActivity(): LaunchActivity | null {
  const s = useStore();
  const p = useBulk().data;
  const instances = useInstances().data;
  const accounts = useAccounts().data;
  const local = s.local;
  const batch = shown(p, !!local);
  const byId = new Map(accounts?.map((a) => [a.id, a]));
  const own = (r: { accountId: number; userId: number }) => {
    const a = byId.get(r.accountId);
    return a && a.userId === r.userId ? a : undefined;
  };
  const live = new Map(instances?.map((i) => [i.id, i]));
  const closed: BulkStage = { kind: "failed", message: t("launch.batch.closed") };

  const rows: LaunchRow[] = batch
    ? batch.items.map((item) => {
        const inst = item.instanceId ? live.get(item.instanceId) : undefined;
        const started = item.stage.kind === "starting" || item.stage.kind === "joining" || item.stage.kind === "inGame";
        // Only ended clients leave the tracker; while the batch runs the backend already says so,
        // and a progress event may arrive before the instance list catches up.
        const stage = inst ? stageOf(inst, own(item)?.allowDisconnect ?? false) : started && batch.done && instances && item.instanceId ? closed : item.stage;
        return { accountId: item.accountId, userId: item.userId, account: own(item), stage, instanceId: item.instanceId, job: inst?.jobId ?? null };
      })
    : (local?.rows ?? []).map((r) => {
        const inst = r.instanceId
          ? live.get(r.instanceId)
          : r.stage.kind === "starting" && r.at !== null
            ? instances?.filter((i) => i.accountId === r.accountId && i.startedAt >= r.at! - 1000).sort((x, y) => x.startedAt - y.startedAt)[0]
            : undefined;
        const stage = inst ? stageOf(inst, own(r)?.allowDisconnect ?? false) : r.instanceId && instances ? closed : r.stage;
        return { accountId: r.accountId, userId: r.userId, account: own(r), stage, instanceId: inst?.id ?? r.instanceId, job: inst?.jobId ?? null };
      });

  // A local row keeps the client it found, so it shows that one ending (or cleared) rather than waiting.
  const found = !batch && local ? rows.map((r, n) => (r.instanceId && !local.rows[n].instanceId ? n : -1)).filter((n) => n >= 0) : [];
  const foundKey = found.join();
  useEffect(() => {
    // Several views use this hook: the first one to get here stores it.
    if (local) for (const n of found) if (!state.local?.rows[n]?.instanceId) setRow(local.id, n, { instanceId: rows[n].instanceId });
  }, [foundKey]); // eslint-disable-line react-hooks/exhaustive-deps

  // A batch that started while a finished local launch was on show replaces it.
  const stale = p && !p.done && local && !local.running ? local.id : null;
  useEffect(() => {
    if (stale) clearLocal(stale);
  }, [stale]);

  const settled = !batch && !!local && !local.running && rows.length > 0 && rows.every((r) => isIn(r.stage)) ? local.id : null;
  useEffect(() => {
    if (!settled) return;
    const timer = setTimeout(() => clearLocal(settled), 4000);
    return () => clearTimeout(timer);
  }, [settled]);

  if (s.arriving || (!batch && !local)) return null;
  const target = batch ? batch.target : local!.target;
  const look = target && describeTarget(target);
  const several = !!batch || rows.length > 1;
  const summary = batch
    ? [look!.kind, batch.sameServer === "pickFirst" ? t("launch.batch.sameServer") : batch.sameServer === "followFirst" ? t("launch.batch.followingFirst") : batch.sameServer === "off" ? null : t("launch.batch.spread"), t("launch.batch.seconds", { s: fmtNum(batch.delayMs / 1000) })]
        .filter(Boolean)
        .join(" · ")
    : rows.length === 1 && look ? look.detail : "";
  return {
    id: batch ? batch.batchId : local!.id,
    kind: batch ? "batch" : "local",
    title: several ? t("launch.batch.label") : (look?.kind ?? t("common.launch")),
    summary,
    rows,
    ok: rows.filter((r) => isIn(r.stage)).length,
    failed: rows.filter((r) => r.stage.kind === "failed").length,
    done: batch ? batch.done : !local!.running,
    stopped: batch ? batch.stopped : local!.stopped,
    note: batch ? batch.note : null,
    rest: rows.filter((r) => (r.stage.kind === "cancelled" || (r.stage.kind === "failed" && !r.instanceId)) && r.account?.status === "valid").map((r) => r.accountId),
    current: rows.find((r) => UNDER_WAY.has(r.stage.kind)) ?? rows.find((r) => r.stage.kind === "queued") ?? null,
    cancelling: batch ? s.cancelling === batch.batchId : local!.cancel,
    cancel: () => {
      if (!batch) return local && patchLocal(local.id, () => ({ cancel: true }));
      const id = batch.batchId;
      set({ cancelling: id });
      api.bulkCancel(id).catch((e) => {
        if (state.cancelling === id) set({ cancelling: null });
        toastError(e);
      });
    },
    dismiss: () => {
      if (batch) dismissBulk(queryClient, batch.batchId);
      set({ local: null });
    },
  };
}
