import "./monitor.css";
import { useEffect, useLayoutEffect, useRef, useState, type FocusEvent, type ReactNode, type RefObject } from "react";
import { AnimatePresence, motion } from "motion/react";
import { asError, type AccountSummary, type ClientMetrics, type Instance } from "../../api";
import { requestNavigation } from "../../app/navigation";
import { useInstances, useMetrics } from "../../data";
import { fmtNum, t, type Key } from "../../i18n";
import { Skeleton } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import { Collapse, DUR, EASE_CSS, Num, prefersReducedMotion, RISE, riseIn, SPRING, STAGGER, TWEEN } from "../../ui/motion";
import { StatusPill } from "../../ui/Status";
import { friendly } from "../../ui/Toast";
import { accountName, Avatar } from "../accounts/shared";
import { LimitsDialog } from "../dashboard/LimitsDialog";
import { isRunning, STATE_TONE, STATES, uptime, useNow } from "../launch/Running";
import { mb, NONE, one, pct } from "./format";

/** Samples kept per line: two minutes at one every 2 s, longer when reading them is slow (`metrics`). */
const KEEP = 60;

type Series = { cpu: number[]; mem: number[] };
type Col = "cpu" | "memory" | "commit" | "gpu" | "gpuMem" | "read" | "write" | "up" | "restarts";
type Sort = { col: Col; dir: "desc" | "asc" } | null;

function rate(bps: number | null) {
  if (bps === null) return NONE;
  if (bps >= 1024 * 1024) return t("monitor.mbps", { n: one(bps / 1024 / 1024) });
  if (bps >= 1024) return t("monitor.kbps", { n: fmtNum(Math.round(bps / 1024)) });
  return t("monitor.bps", { n: fmtNum(Math.round(bps)) });
}
const mbOr = (n: number | null) => (n === null ? NONE : mb(n));
const whole = (n: number) => fmtNum(Math.round(n));
const push = (list: number[], v: number) => [...list, v].slice(-KEEP);
const nameOf = (i: Instance, a?: AccountSummary) => (a ? accountName(a) : t("monitor.accountId", { id: i.accountId }));

/** What each column sorts by. Unknown (no sample yet, or Windows doesn't say) goes last either way. */
const SORT_BY: Record<Col, (i: Instance, m?: ClientMetrics) => number | null | undefined> = {
  cpu: (_, m) => m?.cpuPct,
  memory: (_, m) => m?.memoryMb,
  commit: (_, m) => m?.privateMb,
  gpu: (_, m) => m?.gpuPct,
  gpuMem: (_, m) => m?.gpuMemMb,
  read: (_, m) => m?.readBps,
  write: (_, m) => m?.writeBps,
  up: (i) => -i.startedAt,
  restarts: (i) => i.rejoins,
};
/** Kept for the session: coming back to Monitor finds the same order. */
let lastSort: Sort = null;

/** Descending, ascending, then launch order again. */
const nextSort = (s: Sort, col: Col): Sort => (!s || s.col !== col ? { col, dir: "desc" } : s.dir === "desc" ? { col, dir: "asc" } : null);

function sortRows(list: Instance[], by: Sort, metrics: Map<string, ClientMetrics>) {
  if (!by) return list;
  const key = (i: Instance) => SORT_BY[by.col](i, metrics.get(i.id)) ?? null;
  const dir = by.dir === "desc" ? -1 : 1;
  return [...list].sort((a, b) => {
    const [x, y] = [key(a), key(b)];
    return x === null || y === null ? Number(x === null) - Number(y === null) : (x - y) * dir;
  });
}

/** `list` in the order of `ids`; clients that are new since come after them. */
function keepOrder(list: Instance[], ids: string[]) {
  const at = new Map(ids.map((id, n) => [id, n]));
  return [...list].sort((a, b) => (at.get(a.id) ?? ids.length) - (at.get(b.id) ?? ids.length));
}

/** Rows rise in when they arrive and glide to their new place when the order changes. WAAPI on the
 * row, since motion's transforms stay off table rows; offsetTop, unlike useFlip's viewport rects,
 * doesn't move when the page scrolls between samples. */
function useRowMotion(body: RefObject<HTMLTableSectionElement | null>, ids: string[]) {
  const tops = useRef<Map<string, number> | null>(null);
  const key = ids.join(" ");
  useLayoutEffect(() => {
    const rows = [...(body.current?.rows ?? [])];
    const before = tops.current;
    tops.current = new Map(rows.map((r) => [r.dataset.id!, r.offsetTop]));
    if (!before || rows.length > STAGGER.listLimit) return;
    let fresh = 0;
    for (const r of rows) {
      const was = before.get(r.dataset.id!);
      if (was === undefined) riseIn(r, fresh++);
      else if (was !== r.offsetTop && !prefersReducedMotion()) {
        // A newer move replaces one still under way (or still waiting for frames).
        r.getAnimations().forEach((a) => a.cancel());
        r.animate([{ translate: `0 ${was - r.offsetTop}px` }, { translate: "0 0" }], { duration: DUR.move + 80, easing: EASE_CSS.out });
      }
    }
  }, [key, body]);
}

/** One series over its last KEEP samples, filling in from the end (newest) over its baseline; it
 * stretches to its box, its stroke doesn't. `floor`: the smallest span its height stands for, so
 * noise stays flat. `zero`: drawn from 0 (a share); else from its lowest value (memory: the change
 * is the story). */
function Sparkline({ values = [], floor, zero, label, format }: {
  values?: number[];
  floor: number;
  zero?: boolean;
  label: string;
  format: (n: number) => string;
}) {
  const baseline = <line className="mon-spark-base" x1={0} y1={23.5} x2={100} y2={23.5} vectorEffect="non-scaling-stroke" />;
  if (values.length < 2)
    return (
      <svg className="mon-spark" viewBox="0 0 100 24" preserveAspectRatio="none" aria-hidden="true">
        {baseline}
      </svg>
    );
  const lo = Math.min(...values);
  const hi = Math.max(...values);
  const base = zero ? 0 : lo;
  const span = Math.max(floor, hi - base);
  const x = (i: number) => (((i + KEEP - values.length) / (KEEP - 1)) * 100).toFixed(2);
  const y = (v: number) => (22 - ((v - base) / span) * 20).toFixed(2);
  const pts = values.map((v, i) => `${x(i)},${y(v)}`);
  const range = t("monitor.range", { label, min: format(lo), max: format(hi), now: format(values[values.length - 1]) });
  return (
    <svg className="mon-spark" viewBox="0 0 100 24" preserveAspectRatio="none" role="img" aria-label={range} data-tip={range}>
      {baseline}
      <path className="mon-spark-area" d={`M${x(0)},24L${pts.join("L")}L100,24Z`} />
      <polyline className="mon-spark-line" points={pts.join(" ")} vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

function Figure({ label, chart, children }: { label: string; chart?: ReactNode; children: ReactNode }) {
  return (
    <div className="mon-cell">
      <dt className="mon-label">{label}</dt>
      <dd className="mon-value">
        <bdi>{children}</bdi>
      </dd>
      {chart && <dd className="mon-chart">{chart}</dd>}
    </div>
  );
}

/** All clients together; the numbers ease between samples. */
function Summary({ clients, list, totals }: { clients: number; list?: ClientMetrics[]; totals: Series }) {
  // Null until some client reports it (Windows can't always say).
  const total = (f: (m: ClientMetrics) => number | null) => (list?.some((m) => f(m) !== null) ? list.reduce((n, m) => n + (f(m) ?? 0), 0) : null);
  return (
    <div className="panel mon-strip">
      <dl className="mon-grid">
        <Figure label={t("monitor.clients")}>
          <Num value={clients} format={whole} />
        </Figure>
        <Figure label={t("monitor.cpuAll")} chart={<Sparkline values={totals.cpu} floor={10} zero label={t("monitor.cpuAll")} format={pct} />}>
          <Num value={total((m) => m.cpuPct)} format={pct} />
        </Figure>
        <Figure label={t("monitor.memoryAll")} chart={<Sparkline values={totals.mem} floor={64} label={t("monitor.memoryAll")} format={mb} />}>
          <Num value={total((m) => m.memoryMb)} format={mb} />
        </Figure>
        <Figure label={t("monitor.commitAll")}>
          <Num value={total((m) => m.privateMb)} format={mb} />
        </Figure>
        <Figure label={t("monitor.gpuMemory")}>
          <Num value={total((m) => m.gpuMemMb)} format={mb} />
        </Figure>
        <Figure label={t("monitor.io")}>
          <Num value={total((m) => ((m.readBps ?? m.writeBps) === null ? null : (m.readBps ?? 0) + (m.writeBps ?? 0)))} format={rate} />
        </Figure>
      </dl>
    </div>
  );
}

/** "3 running", in the worst state's hue. Its mark beats on each sample, so a stalled feed shows. */
function LivePill({ running, beat }: { running: Instance[]; beat: number }) {
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    const mark = ref.current?.querySelector(".status-mark");
    if (!beat || !mark) return;
    // Opacity only, in either motion mode: a status mark pulses, it doesn't grow.
    const a = mark.animate([{ opacity: 1 }, { opacity: 0.35 }, { opacity: 1 }], { duration: 500, easing: EASE_CSS.out });
    // A hidden window draws no frames: beats must not pile up behind it.
    return () => a.cancel();
  }, [beat]);
  // Worst first: furthest from in game.
  const worst = (["disconnected", "starting", "joining"] as const).find((s) => running.some((i) => i.state === s));
  const tone = STATE_TONE[worst ?? "inGame"];
  return (
    <motion.span
      ref={ref}
      initial={{ opacity: 0, scale: 0.96 }}
      animate={{ opacity: 1, scale: 1, transition: SPRING.snappy }}
      exit={{ opacity: 0, transition: TWEEN.exit }}
    >
      <StatusPill tone={tone} label={t("app.running", { count: running.length })} />
    </motion.span>
  );
}

function Problem({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  return (
    <div className="notice" data-tone="attention" role="alert">
      <Icon name="alert" className="text-attention" />
      <span className="flex-1 min-w-0">{friendly(asError(error))}</span>
      <Button size="md" icon="refresh" onClick={onRetry}>{t("common.tryAgain")}</Button>
    </div>
  );
}

/** Every running client, one row each, in launch order or sorted by a column. */
function ClientTable({ running, byInst, hist, accounts, onLimits }: {
  running: Instance[];
  byInst: Map<string, ClientMetrics>;
  hist: ReadonlyMap<string, Series>;
  accounts: Map<number, AccountSummary>;
  onLimits: (id: string) => void;
}) {
  const now = useNow(30_000);
  const [sort, setSort] = useState(lastSort);
  // While the pointer or the focus is on the rows, they hold their order: none moves from under it.
  const [frozen, setFrozen] = useState<string[] | null>(null);
  const inside = useRef({ pointer: false, focus: false });
  const shown = useRef<string[]>([]);
  const body = useRef<HTMLTableSectionElement>(null);
  const box = useRef<HTMLDivElement>(null);

  const sorted = sortRows(running, sort, byInst);
  const rows = frozen ? keepOrder(sorted, frozen) : sorted;
  const ids = rows.map((i) => i.id);
  useLayoutEffect(() => {
    shown.current = ids;
  });
  useRowMotion(body, ids);

  // A header keyboard focus lands on under a sticky column scrolls clear of it, ring (4 px) included:
  // the browser's own scroll doesn't know what is sticky, and counts a header 32 px in view as seen.
  const clearSticky = (e: FocusEvent<HTMLTableSectionElement>) => {
    const el = e.target as HTMLElement;
    if (!el.matches(":focus-visible")) return;
    const r = el.getBoundingClientRect();
    const [lo, hi] = [...e.currentTarget.querySelectorAll(".mon-acc, .mon-act")].map((c) => c.getBoundingClientRect()).sort((a, b) => a.left - b.left);
    const dx = r.left < lo.right + 4 ? r.left - lo.right - 4 : r.right > hi.left - 4 ? r.right - hi.left + 4 : 0;
    if (dx) box.current!.scrollBy({ left: dx, behavior: "instant" });
  };

  const hold = (where: "pointer" | "focus", on: boolean) => {
    inside.current[where] = on;
    const held = inside.current.pointer || inside.current.focus;
    setFrozen((f) => (held ? (f ?? shown.current) : null));
  };
  const sortBy = (col: Col) => {
    const next = nextSort(sort, col);
    lastSort = next;
    setSort(next);
    // Asked for: the new order shows even while the rows hold still, and then holds.
    setFrozen((f) => f && sortRows(running, next, byInst).map((i) => i.id));
  };
  const head = (col: Col, label: Key, hint?: Key) => {
    const dir = sort?.col === col ? sort.dir : undefined;
    const tip = hint ? t(hint) : undefined;
    return (
      <th scope="col" className="mon-num" aria-sort={dir && (dir === "desc" ? "descending" : "ascending")}>
        <button type="button" className="mon-sort" data-dir={dir} data-tip={tip} aria-description={tip} onClick={() => sortBy(col)}>
          <Icon name="chevron" size={12} className="mon-sort-mark" />
          {t(label)}
        </button>
      </th>
    );
  };
  // Undefined: the client's first sample isn't in yet. <bdi>: "796 MB" stays in that order right to left.
  const cell = (text: string | undefined) => (text === undefined ? <Skeleton width={36} height={10} className="ms-auto" /> : <bdi>{text}</bdi>);

  return (
    <div ref={box} className="scroll mon-scroll">
      <table className="mon-table">
        <thead onFocus={clearSticky}>
          <tr>
            <th scope="col" className="mon-acc">{t("monitor.account")}</th>
            <th scope="col">{t("monitor.state")}</th>
            {head("cpu", "monitor.cpu")}
            <th scope="col" className="mon-trend"><span className="sr-only">{t("monitor.cpuTrend")}</span></th>
            {head("memory", "monitor.memory", "monitor.memoryHint")}
            <th scope="col" className="mon-trend"><span className="sr-only">{t("monitor.memoryTrend")}</span></th>
            {head("commit", "monitor.commitCol", "monitor.commitHint")}
            {head("gpu", "monitor.gpu", "monitor.gpuHint")}
            {head("gpuMem", "monitor.gpuMem", "monitor.gpuMemoryHint")}
            {head("read", "monitor.read")}
            {head("write", "monitor.write")}
            {head("up", "monitor.up")}
            {head("restarts", "monitor.restarts", "monitor.restartsHint")}
            <th scope="col" className="mon-act"><span className="sr-only">{t("monitor.limits")}</span></th>
          </tr>
        </thead>
        <tbody
          ref={body}
          onPointerEnter={() => hold("pointer", true)}
          onPointerLeave={() => hold("pointer", false)}
          onFocus={() => hold("focus", true)}
          onBlur={(e) => !e.currentTarget.contains(e.relatedTarget) && hold("focus", false)}
        >
          {rows.map((i) => {
            const a = accounts.get(i.accountId);
            const m = byInst.get(i.id);
            const h = hist.get(i.id);
            const name = nameOf(i, a);
            const pid = t("launch.running.pid", { pid: i.pid });
            return (
              <tr key={i.id} data-id={i.id}>
                <td className="mon-acc">
                  <span className="mon-who">
                    {a ? <Avatar account={a} size={20} /> : <span className="w-5 h-5 flex-none" />}
                    <span className="truncate" data-tip={`${name} · ${pid}`}>
                      {name}
                      <span className="sr-only"> {pid}</span>
                    </span>
                  </span>
                </td>
                <td>
                  <span className="mon-state" data-tip={i.note ?? undefined}>
                    <StatusPill tone={STATE_TONE[i.state]}label={STATES[i.state][0]} detail={i.note ?? undefined} />
                  </span>
                </td>
                <td className="mon-num">{cell(m && pct(m.cpuPct))}</td>
                <td className="mon-trend"><Sparkline values={h?.cpu} floor={10} zero label={t("monitor.cpu")} format={pct} /></td>
                <td className="mon-num" data-tip={m && t("monitor.commit", { mb: mb(m.privateMb) })}>{cell(m && mb(m.memoryMb))}</td>
                <td className="mon-trend"><Sparkline values={h?.mem} floor={64} label={t("monitor.memory")} format={mb} /></td>
                <td className="mon-num">{cell(m && mb(m.privateMb))}</td>
                <td className="mon-num">{cell(m && pct(m.gpuPct))}</td>
                <td className="mon-num">{cell(m && mbOr(m.gpuMemMb))}</td>
                <td className="mon-num">{cell(m && rate(m.readBps))}</td>
                <td className="mon-num">{cell(m && rate(m.writeBps))}</td>
                <td className="mon-num">{cell(uptime(i.startedAt, now))}</td>
                <td className="mon-num">{cell(fmtNum(i.rejoins))}</td>
                <td className="mon-act">
                  <Button size="sm" variant="ghost" aria-label={t("launch.running.limitsFor", { name })} onClick={() => onLimits(i.id)}>
                    {t("home.limits")}
                  </Button>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

/** Live CPU, memory and IO of every client RoALT runs, sampled every 2 s: which one uses too much,
 * and its limits one click away. */
export function MonitorView({ accounts }: { accounts: AccountSummary[] }) {
  const instances = useInstances();
  const metrics = useMetrics();
  const running = (instances.data ?? []).filter(isRunning);
  const [hist, setHist] = useState<{ per: ReadonlyMap<string, Series>; totals: Series }>({ per: new Map(), totals: { cpu: [], mem: [] } });
  const [limitsFor, setLimitsFor] = useState<string | null>(null);
  const byId = new Map(accounts.map((a) => [a.id, a]));
  const byInst = new Map((metrics.data ?? []).map((m) => [m.instanceId, m]));

  // One point per new sample; clients that ended drop out. Set during render, not in an effect,
  // so a sample costs one commit, not two. `seen` starts empty so cached data adds its point too.
  const { data } = metrics;
  const [seen, setSeen] = useState<ClientMetrics[]>();
  if (data && data !== seen) {
    setSeen(data);
    setHist((old) => {
      const per = new Map<string, Series>();
      for (const m of data) {
        const s = old.per.get(m.instanceId) ?? { cpu: [], mem: [] };
        per.set(m.instanceId, { cpu: m.cpuPct === null ? s.cpu : push(s.cpu, m.cpuPct), mem: push(s.mem, m.memoryMb) });
      }
      const cpu = data.reduce((n, m) => n + (m.cpuPct ?? 0), 0);
      const mem = data.reduce((n, m) => n + m.memoryMb, 0);
      if (!data.length) return { per, totals: { cpu: [], mem: [] } };
      return { per, totals: { cpu: push(old.totals.cpu, cpu), mem: push(old.totals.mem, mem) } };
    });
  }

  const view = instances.error ? "error" : !instances.data ? "loading" : running.length ? "live" : "empty";
  const limited = running.find((i) => i.id === limitsFor);

  return (
    <main className="mon scroll flex-1 min-w-0 min-h-0 page pb-6 flex flex-col gap-6">
      <header className="flex flex-wrap items-center gap-x-4 gap-y-2">
        <h1 className="page-title">{t("monitor.title")}</h1>
        <AnimatePresence initial={false}>
          {running.length > 0 && <LivePill key="live" running={running} beat={metrics.dataUpdatedAt} />}
        </AnimatePresence>
      </header>

      <AnimatePresence mode="wait" initial={false}>
        <motion.div
          key={view}
          className={view === "live" ? "flex-1 min-h-0 flex flex-col" : undefined}
          initial={{ opacity: 0, y: RISE.md }}
          animate={{ opacity: 1, y: 0, transition: SPRING.smooth }}
          exit={{ opacity: 0, transition: TWEEN.exit }}
        >
          {view === "error" ? (
            <Problem error={instances.error} onRetry={() => void instances.refetch()} />
          ) : view === "loading" ? (
            <p className="mon-wait">{t("common.loading")}</p>
          ) : view === "empty" ? (
            <div className="empty">
              <p>{t("monitor.empty")}</p>
              <Button variant="primary" icon="play" onClick={() => requestNavigation("launch")}>{t("common.launch")}</Button>
            </div>
          ) : (
            <>
              <Summary clients={running.length} list={metrics.data} totals={hist.totals} />
              <Collapse open={!!metrics.error} className="pt-6">
                <Problem error={metrics.error} onRetry={() => void metrics.refetch()} />
              </Collapse>
              <ClientTable running={running} byInst={byInst} hist={hist.per} accounts={byId} onLimits={setLimitsFor} />
            </>
          )}
        </motion.div>
      </AnimatePresence>

      <LimitsDialog instanceId={limited?.id ?? null} name={limited ? nameOf(limited, byId.get(limited.accountId)) : ""} onClose={() => setLimitsFor(null)} />
    </main>
  );
}
