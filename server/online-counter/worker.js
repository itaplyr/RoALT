// RoALT's online counter. Each running RoALT (with "Show how many people use RoALT" on) POSTs
// {"id": "<uuid made once per launch>", "playing": <its running Roblox clients>} to /ping every
// 2 minutes and gets {"online": N, "playing": M} back: how many distinct ids pinged in the last
// 5 minutes, and their clients added up. Nothing is stored on disk.
import { DurableObject } from "cloudflare:workers";

/** An id not seen for this long stops counting. */
const TTL_MS = 5 * 60_000;
/** Most ids held at once, so junk pings can't grow the map without end. */
const MAX_IDS = 50_000;
/** Most clients one id counts for, so one made-up ping can't add thousands. */
const MAX_PLAYING = 100;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (request.method !== "POST" || url.pathname !== "/ping") return new Response("Not found", { status: 404 });
    let id, playing;
    try {
      ({ id, playing = 0 } = await request.json());
    } catch {
      return new Response("Bad request", { status: 400 });
    }
    if (typeof id !== "string" || !UUID.test(id)) return new Response("Bad request", { status: 400 });
    // Missing (RoALT 1.0.1 and older) counts as 0.
    if (!Number.isInteger(playing) || playing < 0) return new Response("Bad request", { status: 400 });
    // One instance for everyone, so all pings land in the same map. A deploy doesn't restart the
    // running instance while pings keep it busy: bump the name when Counter's code changes.
    const counter = env.COUNTER.get(env.COUNTER.idFromName("global-v2"));
    return Response.json(await counter.ping(id, Math.min(playing, MAX_PLAYING)));
  },
};

// shortcut: the ids live in memory only, so an eviction resets the count (it rebuilds within one
// ping interval), and anyone can inflate it with made-up ids (each adds up to MAX_PLAYING to
// `playing`). Add a per-IP cap if that gets abused.
export class Counter extends DurableObject {
  /** id -> {at, playing}, oldest ping first. */
  seen = new Map();
  /** Sum of `playing` over `seen`, kept as ids come and go. */
  playing = 0;

  ping(id, playing) {
    const now = Date.now();
    this.drop(id);
    // Re-insert so the map stays ordered oldest first; then expired ids are all at the front.
    this.seen.set(id, { at: now, playing });
    this.playing += playing;
    for (const [old, { at }] of this.seen) {
      if (now - at <= TTL_MS && this.seen.size <= MAX_IDS) break;
      this.drop(old);
    }
    return { online: this.seen.size, playing: this.playing };
  }

  drop(id) {
    this.playing -= this.seen.get(id)?.playing ?? 0;
    this.seen.delete(id);
  }
}
