import { useEffect, useRef, useState } from "react";
import { api, asError, events, type ErrorPayload } from "../../api";
import { readStored, writeStored } from "../../ui/stored";

/** Lines kept in view; older ones stay in roalt.log. */
const KEEP = 5000;
/** History read from roalt.log when the tab opens. */
const HISTORY = 2000;

export const LEVELS = ["error", "warn", "info", "debug"] as const;
export type Level = (typeof LEVELS)[number];
export type Line = { id: number; level: Level; text: string };

/** tauri-plugin-log's numeric levels (Trace 1 … Error 5). */
const BY_NUMBER: Record<number, Level> = { 1: "debug", 2: "debug", 3: "info", 4: "warn", 5: "error" };

/** The `[LEVEL]` tag of a roalt.log line; lines without one (a panic's next lines) follow `prev`. */
function levelOf(text: string, prev: Level): Level {
  const m = /\[(TRACE|DEBUG|INFO|WARN|ERROR)\]/.exec(text);
  if (!m) return text.startsWith("[fatal]") ? "error" : prev;
  return m[1] === "TRACE" ? "debug" : (m[1].toLowerCase() as Level);
}

let nextId = 0;

/** The newest line Clear hid. roalt.log keeps everything (the diagnostics report reads it), so what
 * the history shows starts after this line, across tab switches and restarts. */
const CLEARED_KEY = "roalt.debug.clearedAfter";

/** The lines after the last `marker`; all of them when it has aged out of `lines`, as they are then
 * all newer than it. */
function afterMarker(lines: string[], marker: string | null): string[] {
  const at = marker === null ? -1 : lines.lastIndexOf(marker);
  return at < 0 ? lines : lines.slice(at + 1);
}

const keep = (lines: Line[]) => (lines.length > KEEP ? lines.slice(-KEEP) : lines);

/** roalt.log's tail, then each line as RoALT writes it. New lines land once per frame, so a burst is
 * one render; while `paused` they wait, counted in `held`. */
export function useLogStream(paused: boolean) {
  const [lines, setLines] = useState<Line[]>([]);
  const [held, setHeld] = useState(0);
  const [history, setHistory] = useState<"loading" | "ready" | ErrorPayload>("loading");
  const [attempt, setAttempt] = useState(0);
  // Arrived but not shown yet: until the history is in, while paused, or until the next frame.
  const queue = useRef<Line[]>([]);
  const gate = useRef({ loaded: false, paused, frame: 0 });

  const flush = useRef(() => {
    const g = gate.current;
    g.frame = 0;
    if (!g.loaded) return;
    if (g.paused) {
      setHeld(queue.current.length);
      return;
    }
    const more = queue.current;
    queue.current = [];
    setHeld(0);
    if (more.length) setLines((l) => keep([...l, ...more]));
  }).current;

  useEffect(() => {
    gate.current.paused = paused;
    if (!paused) flush();
  }, [paused, flush]);

  useEffect(() => {
    let live = true;
    const g = gate.current;
    g.loaded = false;
    queue.current = [];
    // Live lines first, so none are lost while the history loads.
    const un = events.onLog((r) => {
      const q = queue.current;
      q.push({ id: nextId++, level: BY_NUMBER[r.level] ?? levelOf(r.message, "info"), text: r.message });
      if (q.length > KEEP) q.splice(0, q.length - KEEP);
      if (!g.frame) g.frame = requestAnimationFrame(flush);
    });
    api
      .debugTail(HISTORY)
      .then(
        (text) => {
          if (!live) return;
          let prev: Level = "info";
          const all = text.map((s) => {
            prev = levelOf(s, prev);
            return { id: 0, level: prev, text: s };
          });
          const shown = all.slice(all.length - afterMarker(text, readStored(CLEARED_KEY)).length);
          // Ids in reading order: the history, then what came in while it loaded.
          for (const l of [...shown, ...queue.current]) l.id = nextId++;
          setLines(keep(shown));
          setHistory("ready");
        },
        (e) => live && setHistory(asError(e)),
      )
      .finally(() => {
        if (!live) return;
        g.loaded = true;
        flush();
      });
    return () => {
      live = false;
      cancelAnimationFrame(g.frame);
      g.frame = 0;
      void un.then((f) => f());
    };
  }, [attempt, flush]);

  return {
    lines,
    held,
    loading: history === "loading",
    error: typeof history === "object" ? history : null,
    retry: () => {
      setLines([]);
      setHeld(0);
      setHistory("loading");
      setAttempt((n) => n + 1);
    },
    clear: () => {
      const newest = queue.current.at(-1) ?? lines.at(-1);
      // A live line can span several of the file's.
      if (newest) writeStored(CLEARED_KEY, newest.text.split("\n").at(-1) ?? newest.text);
      queue.current = [];
      setHeld(0);
      setLines([]);
    },
  };
}
