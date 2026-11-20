/** Change notices for a store kept outside React: `subscribe` goes to useSyncExternalStore, and the
 * store calls `notify` after each change. */
export function signal() {
  const subs = new Set<() => void>();
  return {
    subscribe: (f: () => void) => (subs.add(f), () => void subs.delete(f)),
    notify: () => subs.forEach((f) => f()),
  };
}
