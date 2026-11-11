// The physics of a liquid drop's two edges (ui/motion/liquid), pure for scripts/test-liquid.mjs.

export type Edge = { x: number; v: number };

/** Stiffness (per s²) and damping ratio of the edge that leads and the one that trails. Damping .62
 * overshoots about 6% of the trip: long trips (a far tab) don't fling past their target. */
export const LEAD = { k: 560, zeta: 0.62 };
export const TRAIL = { k: 230, zeta: 0.72 };
/** Close enough to stop: px and px/s. */
export const REST = { dx: 0.25, v: 6 };
/** Substep, and the most time one frame catches up on. */
const STEP = 0.008;
const CATCH_UP = 0.25;

/** Moves both edges toward their goals for `elapsed` seconds of real time, in small steps: slow or
 * skipped frames (a busy or background window) catch up instead of leaving the drop crawling, and a
 * gap over a quarter second just lands it. `dir` > 0: moving right, so the right edge leads. Returns
 * true once both rest on their goals (then exactly on them, at rest). */
export function stepEdges(l: Edge, r: Edge, goalL: number, goalR: number, dir: number, elapsed: number): boolean {
  let left = Math.min(CATCH_UP, Math.max(0.001, elapsed));
  while (left > 0) {
    const dt = Math.min(STEP, left);
    left -= dt;
    for (const [edge, goal, leads] of [[l, goalL, dir < 0], [r, goalR, dir > 0]] as const) {
      const { k, zeta } = leads ? LEAD : TRAIL;
      const a = k * (goal - edge.x) - 2 * zeta * Math.sqrt(k) * edge.v;
      edge.v += a * dt;
      edge.x += edge.v * dt;
    }
  }
  const resting = elapsed > CATCH_UP || [[l, goalL], [r, goalR]].every(([e, g]) => Math.abs((g as number) - (e as Edge).x) < REST.dx && Math.abs((e as Edge).v) < REST.v);
  if (resting) {
    l.x = goalL;
    r.x = goalR;
    l.v = r.v = 0;
  }
  return resting;
}
