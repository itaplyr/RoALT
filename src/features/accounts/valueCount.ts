// Counting an account's limiteds, shared by its panel's Count button.
import { api } from "../../api";
import { qk, queryClient } from "../../data";

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
