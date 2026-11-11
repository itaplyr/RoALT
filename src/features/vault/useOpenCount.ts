import { useState } from "react";

/** Counts the times `open` turned true. As a key it gives every opening fresh state, even one that
 * comes while the last closing is still fading out. (Its own module: a hook exported beside
 * components would cost those files Fast Refresh.) */
export function useOpenCount(open: boolean) {
  const [seen, setSeen] = useState({ open, count: 0 });
  if (seen.open !== open) setSeen({ open, count: seen.count + (open ? 1 : 0) });
  return seen.count;
}
