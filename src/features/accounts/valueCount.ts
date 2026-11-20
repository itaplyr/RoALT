// Count all: each signed-in account's limiteds, one account after another (Home's total). It runs
// only when asked and goes on while other views are up; a rate limit, no network or a locked vault
// stops it (the next account would fail the same way), any other failure skips that account.
import { useSyncExternalStore } from "react";
import { api, asError, type AccountSummary } from "../../api";
import { qk, queryClient } from "../../data";
import { signal } from "../../ui/signal";
import { friendly } from "../../ui/Toast";

type State = {
  running: boolean;
  /** The account being counted. */
  current: number | null;
  done: number;
  total: number;
  failed: number;
  /** Why the last run stopped early. */
  stopped: string | null;
};

let state: State = { running: false, current: null, done: 0, total: 0, failed: 0, stopped: null };
let cancelled = false;
const { subscribe, notify } = signal();
function set(patch: Partial<State>) {
  state = { ...state, ...patch };
  notify();
}
export const useValueCount = () => useSyncExternalStore(subscribe, () => state);

/** What stops the whole run rather than one account. */
const STOPS = new Set(["rateLimited", "network", "locked"]);

/** Counts `id` now and shows the answer. False when Roblox cut the count short and the last full
 * count stayed (older than this count). */
export async function countOne(id: number) {
  const at = Date.now();
  const v = await api.accountValue(id, true);
  queryClient.setQueryData(qk.value(id), v);
  return !v || v.fetchedAt >= at;
}

/** Reads the other accounts' last counts again (local, nothing is sent): a count may have just
 * downloaded Rolimons' list, which their values use too. */
export function rereadOthers(counted: Set<number>) {
  void queryClient.invalidateQueries({ queryKey: qk.values, predicate: (q) => !counted.has(q.queryKey[1] as number) });
}

export async function countAll(accounts: AccountSummary[]) {
  if (state.running) return;
  const list = accounts.filter((a) => a.status === "valid");
  const counted = new Set<number>();
  cancelled = false;
  set({ running: true, current: null, done: 0, total: list.length, failed: 0, stopped: null });
  for (const a of list) {
    if (cancelled) break;
    set({ current: a.id });
    try {
      if (await countOne(a.id)) counted.add(a.id);
      else set({ failed: state.failed + 1 });
    } catch (e) {
      const err = asError(e);
      if (STOPS.has(err.kind)) {
        set({ stopped: friendly(err) });
        break;
      }
      set({ failed: state.failed + 1 });
    }
    set({ done: state.done + 1 });
  }
  if (counted.size) rereadOthers(counted);
  set({ running: false, current: null });
}

/** Stops after the account being counted. */
export function stopCount() {
  cancelled = true;
}
