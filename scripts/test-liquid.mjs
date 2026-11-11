import assert from "node:assert/strict";
import { stepEdges } from "../src/ui/motion/spring.ts";

// A drop's trip at 60 fps, as ui/motion/liquid drives it: how long until it rests, and how far each
// edge flies past its goal on the way.
function trip(from, to, frame = 1 / 60) {
  const l = { x: from[0], v: 0 };
  const r = { x: from[1], v: 0 };
  const dir = Math.sign((to[0] + to[1]) / 2 - (from[0] + from[1]) / 2);
  const past = { l: 0, r: 0 };
  let t = 0;
  for (; t < 5; t += frame) {
    const resting = stepEdges(l, r, to[0], to[1], dir, frame);
    past.l = Math.max(past.l, dir > 0 ? l.x - to[0] : to[0] - l.x);
    past.r = Math.max(past.r, dir > 0 ? r.x - to[1] : to[1] - r.x);
    if (resting) break;
  }
  return { t, l: l.x, r: r.x, lead: dir > 0 ? past.r : past.l, trail: dir > 0 ? past.l : past.r };
}

for (const [from, to, way] of [
  [[0, 80], [200, 300], "right"],
  [[200, 300], [0, 80], "left"],
  [[4, 68], [600, 690], "a far tab, right"],
]) {
  const distance = Math.abs(to[0] - from[0]);
  const run = trip(from, to);
  assert.ok(run.t < 1, `${way}: rests within 1 s (took ${run.t.toFixed(2)} s)`);
  assert.deepEqual([run.l, run.r], to, `${way}: rests exactly on its goal`);
  assert.ok(run.lead < 0.08 * distance, `${way}: the leading edge overshoots ${run.lead.toFixed(1)} px, under 8% of ${distance}`);
  assert.ok(run.trail <= run.lead, `${way}: the trailing edge overshoots less (${run.trail.toFixed(1)} px)`);
}

// A frame after a long stall (a busy or background window) lands it, at rest.
{
  const l = { x: 0, v: 0 };
  const r = { x: 80, v: 0 };
  assert.equal(stepEdges(l, r, 200, 300, 1, 1), true);
  assert.deepEqual([l.x, r.x, l.v, r.v], [200, 300, 0, 0]);
}

// Slow frames still get there in about the same time (catch-up, not one small step per frame).
assert.ok(trip([0, 80], [200, 300], 0.2).t < 1.2, "slow frames still rest within about a second");

console.log("Liquid checks passed: settling, overshoot, both directions, stalls, slow frames.");
