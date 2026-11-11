import { useEffect, useId, useLayoutEffect, useReducer, useRef, useState } from "react";
import { api, type Profile, type Settings } from "../../api";
import type { Bench } from "../../bindings/Bench";
import type { Benchmark } from "../../bindings/Benchmark";
import type { Suggestion } from "../../bindings/Suggestion";
import type { SuggestionId } from "../../bindings/SuggestionId";
import type { Tier } from "../../bindings/Tier";
import { useInstances } from "../../data";
import { fmtNum, isRtl, t, type Key } from "../../i18n";
import { announce } from "../../ui/a11y";
import { Button } from "../../ui/Button";
import { NumberField } from "../../ui/controls";
import { Collapse, Morph, riseIn, Swap } from "../../ui/motion";
import { StatusPill, type Tone } from "../../ui/Status";
import { useJumpTarget } from "../settings/jump";
import { errorText } from "../settings/kit/rowError";
import { isApplied, KEYS, LABELS } from "./presets";

/** About how long `performance_benchmark` takes: a warm-up, then 8 rounds 2.5 s apart. */
const BENCH_MS = 25_000;
const TIERS: Record<Tier, [Key, Tone]> = {
  light: ["performance.bench.tier.light", "ok"],
  tight: ["performance.bench.tier.tight", "warn"],
  over: ["performance.bench.tier.over", "err"],
};
const gb = (mb: number) => fmtNum(mb / 1024, { maximumFractionDigits: 1 });
const num = (n: number) => fmtNum(n, { maximumFractionDigits: 1 });

/** A suggestion's title and reason, in the UI's language. */
function worded({ id, params: p }: Suggestion): [string, string] {
  const n = (k: string) => num(p[k] ?? 0);
  const title = t(id === "memRestart" ? LABELS.memRestartMb : id === "previewFps" ? "dashboard.previewFps" : LABELS[id]);
  const why = (() => {
    switch (id) {
      case "fpsCap": return p.gpu === undefined ? t("performance.bench.why.fpsCap", { fps: n("fps") }) : t("performance.bench.why.fpsCapGpu", { fps: n("fps"), gpu: fmtNum(Math.round(p.gpu)) });
      case "lowGraphics": return p.gpu === undefined ? t("performance.bench.why.lowGraphics") : t("performance.bench.why.lowGraphicsGpu", { gpu: fmtNum(Math.round(p.gpu)) });
      case "lighterRendering": return t("performance.bench.why.lighterRendering");
      case "minimizeOnLaunch": return t("performance.bench.why.minimizeOnLaunch", { n: n("n") });
      case "tinyOnLaunch": return t("performance.bench.why.tinyOnLaunch", { width: n("width"), height: n("height") });
      case "muteOnLaunch": return t("performance.bench.why.muteOnLaunch", { n: n("n") });
      case "autoSpread": return t("performance.bench.why.autoSpread", { n: n("n"), cores: n("cores") });
      case "fairCap": return t("performance.bench.why.fairCap", { cap: n("cap") });
      case "adaptive": return p.cap === undefined ? t("performance.bench.why.adaptiveEco") : t("performance.bench.why.adaptive", { cap: n("cap") });
      case "trimEveryMin": return t("performance.bench.why.trimEveryMin", { min: n("min") });
      case "memThrottle": return t("performance.bench.why.memThrottle", { mb: n("mb"), peak: fmtNum(Math.round(p.peak ?? 0)) });
      case "memRestart": return p.crashes === undefined ? t("performance.bench.why.memRestart", { mb: n("mb") }) : t("performance.bench.why.memRestartCrash", { mb: n("mb"), crashes: n("crashes") });
      case "previewFps": return t("performance.bench.why.previewFps", { fps: n("fps") });
    }
  })();
  return [title, why];
}

/** What the review adds about how sure the measurement is. */
function benchFacts(b: Bench): string[] {
  return [
    b.caps === "refused" ? t("performance.bench.capsRefused") : b.caps === "unknown" ? t("performance.bench.capsUnknown") : null,
    b.underLimits ? t("performance.bench.underLimits") : null,
    b.gpuPeak !== null ? t("performance.bench.gpuRough") : null,
    b.memRestarts ? t("performance.bench.restarts", { n: fmtNum(b.memRestarts) }) : null,
    b.longCrashes ? t("performance.bench.crashes", { n: fmtNum(b.longCrashes) }) : null,
  ].filter((f): f is string => f !== null);
}

/** The settings a benchmark's suggestions were built on (`performance::suggest` reads these). */
const suggestedOn = (s: Settings) => JSON.stringify([...KEYS, "windowLayout"].map((k) => s[k as keyof Settings]));

// Module-level so leaving the page mid-run neither loses the result nor hides the run still going.
let run: { started: number; done: Promise<void> } | null = null;
let last: { result: Benchmark; on: string } | null = null;
let failure: string | null = null;

/** Applying the measurement's own suggestions doesn't make it stale. */
export function benchApplied(next: Settings) {
  if (last) last = { ...last, on: suggestedOn(next) };
}

/** The estimate while measuring: 25 s, linear, held at 95 % until the answer comes. It runs under
 * reduced motion too, as the only sign of progress; no React render per frame. A clip, not a scale,
 * so the leading end stays round. */
function Estimate({ started }: { started: number }) {
  const fill = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const span = BENCH_MS * 0.95;
    const hide = (pct: number) => (isRtl() ? `inset(0 0 0 ${pct}% round 999px)` : `inset(0 ${pct}% 0 0 round 999px)`);
    const a = fill.current?.animate([{ clipPath: hide(100) }, { clipPath: hide(5) }], { duration: span, easing: "linear", fill: "forwards" });
    if (a) a.currentTime = Math.min(Date.now() - started, span);
    return () => a?.cancel();
  }, [started]);
  return (
    <div className="perf-estimate">
      <p className="text-[13px] text-fg-2">{t("performance.bench.measuring")}</p>
      <span aria-hidden="true" className="meter perf-estimate-bar">
        <span ref={fill} />
      </span>
    </div>
  );
}

/** Measures the clients in a game, then offers what to turn on for them, each with its reason. */
export function ForThisPc({ settings, previewFps, onReview }: { settings: Settings; previewFps: number; onReview: (p: Profile, facts: string[]) => void }) {
  const card = useRef<HTMLDivElement>(null);
  useJumpTarget("bench", card);
  const inGame = (useInstances().data ?? []).filter((i) => i.state === "inGame").length;
  const [plan, setPlan] = useState<number | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const n = plan ?? Math.max(inGame, 1);
  const [off, setOff] = useState<ReadonlySet<SuggestionId>>(new Set());
  const [, rerender] = useReducer((x: number) => x + 1, 0);
  const needId = useId();
  const planId = useId();
  const tickId = useId();
  const running = run;
  useEffect(() => {
    if (!running) return;
    let mounted = true;
    void running.done.then(() => mounted && rerender());
    return () => void (mounted = false);
  }, [running]);

  function measure() {
    if (run || !inGame) return;
    const on = suggestedOn(settings);
    failure = null;
    run = {
      started: Date.now(),
      done: api
        .performanceBenchmark(n)
        .then((result) => void (last = { result, on }), (e) => void (failure = errorText(e)))
        .finally(() => void (run = null)),
    };
    setOff(new Set());
    rerender();
  }

  const result = last?.result;
  // Built on settings changed since: they could loosen what was changed, so measure again.
  const stale = last !== null && last.on !== suggestedOn(settings);
  // What is still lighter than now: the backend can't see Home's preview rate.
  const open = result?.advice.suggestions.filter((s) => (s.id === "previewFps" ? (s.profile.previewFps ?? 0) < previewFps : !isApplied(s.profile, settings))) ?? [];
  const ticked = open.filter((s) => !off.has(s.id));
  const toggle = (id: SuggestionId) => setOff((o) => {
    const next = new Set(o);
    if (!next.delete(id)) next.add(id);
    return next;
  });
  const b = result?.bench;
  const a = result?.advice;
  const offers = !!result && !stale && open.length > 0;

  // A new answer rises in, line by line, and is told (the focus stays on Measure); one already there
  // when the page opens just shows.
  const body = useRef<HTMLDivElement>(null);
  const seen = useRef(result);
  useLayoutEffect(() => {
    if (result === seen.current) return;
    seen.current = result;
    if (result) announce(t(TIERS[result.advice.tier][0]));
    body.current?.querySelectorAll("[data-rise]").forEach((el, i) => riseIn(el, i));
  }, [result]);

  return (
    <div ref={card} className="panel perf-bench">
      <div className="perf-bench-head">
        <div className="min-w-0 flex-[1_1_320px]">
          <h2 className="section-title">{t("performance.bench.title")}</h2>
          <p id={needId} className="set-hint">{t(inGame ? "performance.bench.intro" : "performance.bench.needGame")}</p>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <NumberField
            label={t("performance.bench.planFor")}
            unit={t("performance.bench.clients")}
            short={t("performance.bench.clients")}
            min={1}
            max={200}
            value={n}
            onCommit={setPlan}
            onInvalid={setPlanError}
            describedBy={planError ? planId : undefined}
          />
          {/* aria-disabled: it keeps the focus while measuring, and stays findable with its reason. */}
          <Morph>
            <Button
              size="md"
              variant={offers ? "secondary" : "primary"}
              busy={!!running}
              aria-disabled={!!running || !inGame || undefined}
              aria-describedby={inGame ? undefined : needId}
              onClick={measure}
            >
              <Swap k={result ? "again" : "measure"}>{t(result ? "performance.bench.again" : "performance.bench.measure")}</Swap>
            </Button>
          </Morph>
        </div>
      </div>
      <Collapse open={!!planError}>
        <p id={planId} role="alert" className="alert perf-bench-line">{planError}</p>
      </Collapse>
      <div role="status">
        <Collapse open={!!running}>{running && <Estimate started={running.started} />}</Collapse>
      </div>
      <Collapse open={!!failure && !running}>
        <p role="alert" className="alert perf-bench-line">{failure}</p>
      </Collapse>
      <Collapse open={!!b && !running}>
        {b && a && (
          <div ref={body} className="perf-bench-result">
            <div className="perf-bench-facts" data-rise>
              <StatusPill tone={TIERS[a.tier][1]} label={t(TIERS[a.tier][0])} />
              <span>{t("performance.bench.forN", { n: fmtNum(a.n) })}</span>
              <span>{t("performance.bench.pc", { cores: fmtNum(b.cores), gb: gb(b.ramMb) })}</span>
              <span>{t("performance.bench.each", { gb: gb(b.perMb), cores: num(b.perCores) })}</span>
              {a.room !== null && <span>{t("performance.bench.room", { n: fmtNum(a.room) })}</span>}
            </div>
            {b.underLimits && <p className="text-[13px] text-fg-3" data-rise>{t("performance.bench.underLimits")}</p>}
            {!offers ? (
              <p className="text-fg-2" data-rise>{t(stale ? "performance.bench.stale" : "performance.bench.none")}</p>
            ) : (
              <>
                <div role="group" aria-label={t("performance.bench.suggestions")} className="perf-suggest">
                  {open.map((s) => {
                    const [title, why] = worded(s);
                    return (
                      <label key={s.id} className="perf-suggest-row" data-rise>
                        <input type="checkbox" className="cb" checked={!off.has(s.id)} onChange={() => toggle(s.id)} />
                        <span className="flex flex-col min-w-0">
                          <span className="font-medium">{title}</span>
                          <span className="text-[13px] text-fg-2">{why}</span>
                        </span>
                      </label>
                    );
                  })}
                </div>
                <div className="flex flex-col items-end gap-1" data-rise>
                  <Button
                    size="md"
                    variant="primary"
                    aria-haspopup="dialog"
                    aria-disabled={!ticked.length || undefined}
                    aria-describedby={ticked.length ? undefined : tickId}
                    onClick={() => ticked.length && onReview(Object.assign({}, ...ticked.map((s) => s.profile)) as Profile, benchFacts(b))}
                  >
                    {t("performance.bench.reviewApply")}
                  </Button>
                  {!ticked.length && <p id={tickId} className="text-[13px] text-fg-3">{t("performance.bench.tickFirst")}</p>}
                </div>
              </>
            )}
          </div>
        )}
      </Collapse>
    </div>
  );
}
