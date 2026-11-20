// Settings' Performance area: its rows that draw their own controls, and its three panels (the
// preset cards, For this PC, the share row) with the review they all open.
import "./performance.css";
import { createContext, useContext, useRef, useState, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { motion } from "motion/react";
import { api, type Profile } from "../../api";
import type { Limits } from "../../bindings/Limits";
import { useAccounts, useSetSettings, useSettings } from "../../data";
import { fmtNum, t, type Key } from "../../i18n";
import { Button } from "../../ui/Button";
import { NumberField, Segmented, Switch } from "../../ui/controls";
import { Icon } from "../../ui/Icon";
import { Collapse, Morph, SPRING, Swap, TWEEN } from "../../ui/motion";
import { StatusPill } from "../../ui/Status";
import { fpsLabel, PREVIEW_FPS, usePreviewFps } from "../dashboard/previewFps";
import { useJumpTarget } from "../settings/jump";
import { useRow } from "../settings/kit/rowError";
import { SettingRow, type RowRenderers } from "../settings/kit/SettingRow";
import { benchApplied, ForThisPc } from "./ForThisPc";
import { FpsRow } from "./FpsControl";
import { FlagsDialog } from "./FlagsDialog";
import { FACTS_SHOWN, isApplied, named, ownCaps, PRESETS, recommend, type Preset } from "./presets";
import { ReviewDialog } from "./ReviewDialog";
import { ShareRow } from "./ShareRow";

/** "[320] × [240] px": the size clients go tiny at. */
function TinyFields() {
  const { ctx, write, setError, describedBy } = useRow();
  return (
    <span className="flex items-center gap-2">
      <NumberField unit={t("performance.tinyWidth")} short="×" min={16} max={7680} value={ctx.s.microWidth} onCommit={(microWidth) => void write({ microWidth })} onInvalid={setError} describedBy={describedBy} />
      <NumberField unit={t("performance.tinyHeight")} short="px" min={16} max={4320} value={ctx.s.microHeight} onCommit={(microHeight) => void write({ microHeight })} onInvalid={setError} describedBy={describedBy} />
    </span>
  );
}

/** The tiny size, under "Start clients tiny" while that is on. */
function TinyBelow() {
  const { ctx } = useRow();
  return (
    <Collapse open={ctx.s.tinyOnLaunch}>
      <div className="pt-2.5">
        <TinyFields />
      </div>
    </Collapse>
  );
}

/** What clients not in front get: a CPU limit, efficiency mode, a trim schedule; each named on screen. */
function BackgroundLimits() {
  const { ctx, write } = useRow();
  const a = ctx.s.adaptive;
  const put = (patch: Partial<Limits>) => void write({ adaptive: { ...a, background: { ...a.background, ...patch } } });
  return (
    <div className="perf-bg">
      <div className="perf-bg-item">
        <span className="perf-bg-label">{t("launch.limits.cpu")}</span>
        <Segmented
          mono
          label={t("settings.adaptiveBgCpu")}
          value={a.background.cpuCap ?? 0}
          options={[0, 50, 25, 10, 5].map((c) => ({ value: c, label: c ? t("settings.percent", { n: c }) : t("common.off") }))}
          onChange={(c) => put({ cpuCap: c || null })}
        />
      </div>
      <label className="perf-bg-item">
        <span className="perf-bg-label">{t("launch.limits.eco")}</span>
        <Switch label={t("settings.adaptiveBgEco")} checked={a.background.eco} onChange={(eco) => put({ eco })} />
      </label>
      <div className="perf-bg-item">
        <span className="perf-bg-label">{t("launch.limits.trimEvery")}</span>
        <Segmented
          mono
          label={t("settings.adaptiveBgTrim")}
          value={a.background.trimEveryMin ?? -1}
          options={[-1, 0, 5, 15, 30, 60].map((m) => ({ value: m, label: m < 0 ? t("common.default") : m ? t("settings.minutesShort", { n: m }) : t("common.off") }))}
          onChange={(m) => put({ trimEveryMin: m < 0 ? null : m })}
        />
      </div>
    </div>
  );
}

/** Custom FFlags: for every account or each its own; the global ones are edited here. */
function CustomFlagsRow() {
  return (
    <SettingRow id="customFlags">
      <CustomFlagsControl />
    </SettingRow>
  );
}

function CustomFlagsControl() {
  const { ctx, write, describedBy } = useRow();
  const setSettings = useSetSettings({ quiet: true });
  const [editing, setEditing] = useState(false);
  const global = ctx.s.customFlagsScope === "global";
  const count = Object.keys(ctx.s.customFlags).length;
  return (
    <span className="flex flex-wrap items-center justify-end gap-2">
      <Segmented
        label={t("flags.scope")}
        value={ctx.s.customFlagsScope}
        options={[{ value: "global" as const, label: t("flags.global") }, { value: "account" as const, label: t("flags.perAccount") }]}
        onChange={(customFlagsScope) => void write({ customFlagsScope })}
      />
      {global && (
        <Button size="md" icon="pencil" onClick={() => setEditing(true)} aria-describedby={describedBy}>
          {count ? t("flags.count", { count }) : t("flags.edit")}
        </Button>
      )}
      <FlagsDialog
        open={editing}
        onClose={() => setEditing(false)}
        title={t("flags.title")}
        flags={ctx.s.customFlags}
        // Through the API, not `write`: a refusal belongs in the dialog, not the row.
        onSave={(customFlags) => setSettings.mutateAsync({ customFlags })}
      />
    </span>
  );
}

function PreviewFpsRow() {
  const [fps, setFps] = usePreviewFps();
  return (
    <SettingRow id="previewFps">
      <Segmented mono label={t("dashboard.previewFps")} value={fps} options={PREVIEW_FPS.map((f) => ({ value: f, label: fpsLabel(f) }))} onChange={setFps} />
    </SettingRow>
  );
}

/** Performance's rows that draw their own controls: Settings adds them to its own. The
 * strip-visuals flags get a heading inside Lighter rendering's panel. */
export const PERFORMANCE_RENDERERS: RowRenderers = {
  fpsCap: FpsRow,
  // The switch, and the size it makes clients while it's on (the manual Tiny keeps using it too).
  tinyOnLaunch: () => <SettingRow id="tinyOnLaunch" below={<TinyBelow />} />,
  graySky: () => (
    <>
      <h3 className="perf-subhead">{t("performance.stripTitle")}</h3>
      <SettingRow id="graySky" />
    </>
  ),
  adaptiveBg: () => <SettingRow id="adaptiveBg" below={<BackgroundLimits />} />,
  previewFps: PreviewFpsRow,
  // Custom FFlags are backend-agnostic; the bandwidth row stays Windows-only and unrendered here.
  customFlags: CustomFlagsRow,
};

/** The review a profile goes through before it applies. `pick`: For this PC's own, which is "in
 * use" once applied. */
type Review = { open: (profile: Profile, facts?: string[]) => void; pick: (profile: Profile, facts: string[]) => void; picked: Profile | null };
const ReviewContext = createContext<Review | null>(null);
const useReview = () => useContext(ReviewContext)!;

/** Holds the one review dialog the three panels open: they sit apart on Settings' page. */
export function ReviewProvider({ children }: { children: ReactNode }) {
  // From the query, not a prop: a prop can lag a render behind the accounts the panels read.
  const settings = useSettings().data!;
  const [review, setReview] = useState<{ profile: Profile; facts?: string[] } | null>(null);
  // What "For this PC" last sent to the review: it is "in use" once that is applied.
  const [picked, setPicked] = useState<Profile | null>(null);
  // Enter held on Apply lands on what opened the review: it must not open it again.
  const closedAt = useRef(0);
  const open = (profile: Profile, facts?: string[]) => void (Date.now() - closedAt.current > 400 && setReview({ profile, facts }));
  const pick = (profile: Profile, facts: string[]) => {
    setPicked(profile);
    open(profile, facts);
  };
  return (
    <ReviewContext value={{ open, pick, picked }}>
      {children}
      <ReviewDialog
        profile={review?.profile ?? null}
        settings={settings}
        facts={review?.facts}
        onApplied={review?.facts ? benchApplied : undefined}
        onClose={() => {
          closedAt.current = Date.now();
          setReview(null);
        }}
      />
    </ReviewContext>
  );
}

/** `fit`: about how many clients this preset fits by specs, in words; `recommended`: the specs' pick. */
function PresetCard({ preset: p, active, fit, recommended, onPick }: { preset: Preset; active: boolean; fit?: string; recommended?: boolean; onPick: () => void }) {
  const ref = useRef<HTMLButtonElement>(null);
  useJumpTarget(p.id, ref);
  const facts = p.facts();
  const rest = facts.length - FACTS_SHOWN;
  return (
    <button ref={ref} type="button" className="btn preset" data-active={active || undefined} aria-haspopup="dialog" onClick={onPick}>
      <span className="perf-preset-head">
        <span className="section-title">
          {t(p.label)}
          {active && <span className="sr-only"> {t("performance.inUse")}</span>}
          {recommended && <> <span className="tag ms-1 align-middle">{t("performance.specs.recommended")}</span></>}
        </span>
        <span aria-hidden="true" className="tick tick-round">
          {/* Applied, the check pops into its tile; it leaves faster. */}
          <motion.span className="grid" initial={false} animate={active ? { scale: 1, opacity: 1 } : { scale: 0.4, opacity: 0 }} transition={active ? SPRING.snappy : TWEEN.exit}>
            <Icon name="check" size={14} />
          </motion.span>
        </span>
      </span>
      <span className="text-fg-2">{t(p.tag)}</span>
      {fit && <span className="font-medium">{fit}</span>}
      <span className="perf-preset-facts">
        {facts.slice(0, FACTS_SHOWN).map((f) => (
          <span key={f}>{f}</span>
        ))}
        {rest > 0 && (
          <>
            {/* The rest are in the review. */}
            <span aria-hidden="true" className="text-fg-3">+{fmtNum(rest)}</span>
            <span className="sr-only">{facts.slice(FACTS_SHOWN).join(". ")}</span>
          </>
        )}
      </span>
    </button>
  );
}

/** How hard RoALT works this PC: what is in use now, and a preset to pick, reviewed before it
 * applies. `block`: its name on Settings' page (as the other panels'). */
export function PresetsPanel({ block }: { block?: string }) {
  const settings = useSettings().data!;
  const { open, picked } = useReview();
  const presets = useRef<HTMLDivElement>(null);
  useJumpTarget("presets", presets);

  // An account's own cap beats a preset's, so it isn't what those clients get.
  // A pick naming no setting would match any settings (isApplied skips what it leaves out).
  const candidates = [...PRESETS, ...(picked && named(picked) ? [{ label: "performance.bench.title" as Key, profile: picked }] : [])];
  const accounts = useAccounts().data ?? [];
  const active = ownCaps(accounts) ? undefined : candidates.find((p) => isApplied(p.profile, settings));
  const using = active ? t("performance.using", { name: t(active.label) }) : t("performance.custom");
  // By specs alone: rough, but there before any client runs. Until it answers, the cards are as they were.
  const specs = useQuery({ queryKey: ["specs"], queryFn: api.performanceSpecs, staleTime: 5 * 60_000 }).data;
  const want = Math.max(1, accounts.filter((a) => a.status === "valid").length);
  const pick = specs && recommend(specs.rooms, want, specs.integrated);
  const fitOf = (id: string) => {
    if (!specs || !(id in specs.rooms)) return undefined;
    const n = specs.rooms[id as keyof typeof specs.rooms];
    return id === pick && n < want ? t("performance.specs.notAll", { n: fmtNum(n), m: fmtNum(want) }) : t("performance.bench.room", { n: fmtNum(n) });
  };
  const gb = (mb: number) => fmtNum(mb / 1024, { maximumFractionDigits: 1 });

  return (
    <section className="set-section perf-top" data-block={block} aria-label={t("performance.presets")}>
      <div className="perf-title">
        <Morph className="relative">
          <Swap k={using}>
            <StatusPill tone="neutral" shape={active ? "dot" : "ring"} label={using} />
          </Swap>
        </Morph>
        <p className="page-sub">{t("performance.intro")}</p>
      </div>
      <div ref={presets} className="perf-presets">
        {PRESETS.map((p) => (
          <PresetCard key={p.id} preset={p} active={p === active} fit={fitOf(p.id)} recommended={p.id === pick} onPick={() => open(p.profile)} />
        ))}
      </div>
      {specs && (
        <p className="meta">
          {[t("performance.bench.pc", { cores: fmtNum(specs.threads), gb: gb(specs.ramMb) }), specs.cpu, specs.gpu && t("performance.specs.gpu", { gpu: specs.gpu, gb: gb(specs.vramMb) })].filter(Boolean).join(" · ")}
          <br />
          {t("performance.specs.hint")}
        </p>
      )}
    </section>
  );
}

/** For this PC: measures the clients in a game and offers what to turn on, through the review. */
export function BenchPanel({ block }: { block?: string }) {
  const settings = useSettings().data!;
  const [previewFps] = usePreviewFps();
  const { pick } = useReview();
  return (
    <section className="set-section" data-block={block}>
      <ForThisPc settings={settings} previewFps={previewFps} onReview={pick} />
    </section>
  );
}

/** Share your setup: a share code or a file, each read back through the review. */
export function SharePanel({ block }: { block?: string }) {
  return <ShareRow block={block} onProfile={useReview().open} />;
}
