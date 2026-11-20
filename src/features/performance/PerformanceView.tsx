import "./performance.css";
import { useRef, useState } from "react";
import { motion } from "motion/react";
import type { Profile } from "../../api";
import type { Limits } from "../../bindings/Limits";
import { useAccounts, useSetSettings, useSettings } from "../../data";
import { fmtNum, t, type Key } from "../../i18n";
import { Button } from "../../ui/Button";
import { NumberField, Segmented, Switch } from "../../ui/controls";
import { Icon } from "../../ui/Icon";
import { Collapse, Morph, SPRING, Swap, TWEEN } from "../../ui/motion";
import { StatusPill } from "../../ui/Status";
import { PREVIEW_FPS, usePreviewFps } from "../dashboard/previewFps";
import { useJumpTarget } from "../settings/jump";
import { useRow } from "../settings/kit/rowError";
import { Renderers, SettingRow, type RowRenderers } from "../settings/kit/SettingRow";
import { SettingSection } from "../settings/kit/SettingSection";
import { HeadlessResolutionRow } from "../settings/LinuxRows";
import { MocktailNotice } from "../settings/MocktailNotice";
import { PERFORMANCE_ENTRIES } from "./entries";
import { benchApplied, ForThisPc } from "./ForThisPc";
import { FpsRow } from "./FpsControl";
import { FlagsDialog } from "./FlagsDialog";
import { FACTS_SHOWN, isApplied, named, ownCaps, PRESETS, type Preset } from "./presets";
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
      <Segmented mono label={t("dashboard.previewFps")} value={fps} options={PREVIEW_FPS.map((f) => ({ value: f, label: t("dashboard.fps", { fps: f }) }))} onChange={setFps} />
    </SettingRow>
  );
}

/** Performance's rows that draw their own controls. The strip-visuals flags get a heading inside
 * Lighter rendering's panel. */
const RENDERERS: RowRenderers = {
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

/** Rows whose backend is Windows-only: there is no QoS policy here. Session muting is not one of
 * them: the Linux fork mutes a client's sink-input through `pactl` (`instances/audio_linux.rs`). */
const WINDOWS_ONLY = new Set(["bandwidth"]);

/** The groups of rows in screen order; each lists its entries' top rows (entries.ts order). */
const GROUPS = (["performance.fineTune", "performance.section.cpuMemory", "performance.section.roalt", "performance.section.network"] as const)
  .map((group: Key) => ({
    group,
    ids: PERFORMANCE_ENTRIES.filter((e) => "group" in e.place && e.place.group === group && !e.parent && !WINDOWS_ONLY.has(e.id)).map((e) => e.id),
  }))
  .filter((g) => g.ids.length > 0);

function PresetCard({ preset: p, active, onPick }: { preset: Preset; active: boolean; onPick: () => void }) {
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
        </span>
        <span aria-hidden="true" className="tick tick-round">
          {/* Applied, the check pops into its tile; it leaves faster. */}
          <motion.span className="grid" initial={false} animate={active ? { scale: 1, opacity: 1 } : { scale: 0.4, opacity: 0 }} transition={active ? SPRING.snappy : TWEEN.exit}>
            <Icon name="check" size={14} />
          </motion.span>
        </span>
      </span>
      <span className="text-fg-2">{t(p.tag)}</span>
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

/** How hard RoALT works this PC: a preset or this PC's own suggestions, each reviewed before it
 * applies, then every setting behind them, then a profile to share. */
export function PerformanceView() {
  // From the query, not Shell's prop: the prop can lag a render behind the accounts below, and the pill then named a mix of both.
  const settings = useSettings().data!;
  const [previewFps] = usePreviewFps();
  const [review, setReview] = useState<{ profile: Profile; facts?: string[] } | null>(null);
  // What "For this PC" last sent to the review: it is "in use" once that is applied.
  const [picked, setPicked] = useState<Profile | null>(null);
  // Enter held on Apply lands on what opened the review: it must not open it again.
  const closedAt = useRef(0);
  const presets = useRef<HTMLElement>(null);
  useJumpTarget("presets", presets);
  const open = (profile: Profile, facts?: string[]) => Date.now() - closedAt.current > 400 && setReview({ profile, facts });

  // An account's own cap beats a preset's, so it isn't what those clients get.
  // A pick naming no setting would match any settings (isApplied skips what it leaves out).
  const candidates = [...PRESETS, ...(picked && named(picked) ? [{ label: "performance.bench.title" as Key, profile: picked }] : [])];
  const active = ownCaps(useAccounts().data ?? []) ? undefined : candidates.find((p) => isApplied(p.profile, settings));
  const using = active ? t("performance.using", { name: t(active.label) }) : t("performance.custom");

  return (
    <main className="scroll flex-1 min-w-0 page">
      <div className="perf-page">
        <header className="perf-head">
          <div className="perf-title">
            <h1 className="page-title">{t("performance.title")}</h1>
            <Morph className="relative">
              <Swap k={using}>
                <StatusPill tone="neutral" shape={active ? "dot" : "ring"} label={using} />
              </Swap>
            </Morph>
          </div>
          <p className="page-sub">{t("performance.intro")}</p>
        </header>

        <div className="perf-top">
          <section ref={presets} aria-label={t("performance.presets")} className="perf-presets">
            {PRESETS.map((p) => (
              <PresetCard key={p.id} preset={p} active={p === active} onPick={() => open(p.profile)} />
            ))}
          </section>
          <ForThisPc
            settings={settings}
            previewFps={previewFps}
            onReview={(p, facts) => {
              setPicked(p);
              open(p, facts);
            }}
          />
        </div>

        <Renderers value={RENDERERS}>
          {GROUPS.map((g) => (
            <SettingSection key={g.group} label={g.group} ids={g.ids}>
              {g.group === "performance.section.roalt" && (
                <>
                  <div className="set-row">
                    <MocktailNotice />
                  </div>
                  <HeadlessResolutionRow settings={settings} />
                </>
              )}
            </SettingSection>
          ))}
        </Renderers>
        <ShareRow onProfile={(p) => open(p)} />
      </div>
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
    </main>
  );
}
