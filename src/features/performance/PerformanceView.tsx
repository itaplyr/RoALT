import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { open as openFile, save as saveFile } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api, asError, type AccountSummary, type Profile, type Settings } from "../../api";
import type { RenderFlags } from "../../bindings/RenderFlags";
import { useAccounts, useSetSettings, useUpdateAccount } from "../../data";
import { NumberField, Segmented, Switch } from "../../ui/controls";
import { Dialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { fmtList, fmtNum, t, type Key } from "../../i18n";
import { friendly, toast, toastError } from "../../ui/Toast";
import { accountName } from "../accounts/shared";
import { BandwidthRow, Row } from "../settings/SettingsView";
import { PREVIEW_FPS, setPreviewFps, storedFps, usePreviewFps, type PreviewFps } from "../dashboard/previewFps";

const profileFilter = () => [{ name: t("performance.profileFilter"), extensions: ["json"] }];
/** A share code: this prefix, then the profile's JSON as base64url. */
const CODE = "RoALT-PERF:";

function toCode(json: string) {
  const bytes = new TextEncoder().encode(json);
  return CODE + btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** The profile's JSON in a share code; null when it isn't one. */
function fromCode(code: string): string | null {
  const body = code.trim();
  if (!body.startsWith(CODE)) return null;
  try {
    const b64 = body.slice(CODE.length).replace(/\s+/g, "").replace(/-/g, "+").replace(/_/g, "/");
    return new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)));
  } catch {
    return null;
  }
}

/** The accounts with an FPS cap of their own, in words; empty with none. */
const ownCaps = (accounts: AccountSummary[]) => fmtList(accounts.filter((a) => a.fpsCap).map((a) => `${accountName(a)} (${a.fpsCap})`));

/** The FPS cap: a slider for the feel of it and a field for the exact number. Saved on release. */
function FpsRow({ settings }: { settings: Settings }) {
  const set = useSetSettings();
  const [fps, setFps] = useState(settings.fpsCap);
  useEffect(() => setFps(settings.fpsCap), [settings.fpsCap]);
  const commit = () => fps !== settings.fpsCap && set.mutate({ fpsCap: fps });
  // An account's own cap wins over this one.
  const own = ownCaps(useAccounts().data ?? []);
  return (
    <div className="setting" title={t("settings.fpsCapHint")}>
      <div className="setting-text">
        <label className="font-medium" htmlFor="perf-fps">{t("settings.fpsCap")}</label>
        <p className="text-[13px] text-dim">{t("performance.hintFps")}</p>
        {own && <p className="text-[13px] text-orange line-clamp-2" title={own}>{t("performance.fpsOverridden", { names: own })}</p>}
      </div>
      <div className="flex-[1_1_280px] flex items-center gap-3.5 min-h-11">
        <input
          id="perf-fps"
          className="range"
          type="range"
          min={0}
          max={360}
          step={5}
          value={fps}
          onChange={(e) => setFps(Number(e.target.value))}
          onPointerUp={commit}
          onKeyUp={commit}
          onBlur={commit}
        />
        <NumberField unit={t("settings.unit.fpsOff")} short="FPS" min={0} max={360} value={fps} onCommit={(fpsCap) => set.mutate({ fpsCap })} />
      </div>
    </div>
  );
}

function SwitchRow({ title, hint, more, checked, onChange }: { title: string; hint: string; more: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <div className="setting" title={more}>
      <div className="setting-text">
        <p className="font-medium">{title}</p>
        <p className="text-[13px] text-dim">{hint}</p>
      </div>
      <Switch label={title} checked={checked} onChange={onChange} />
    </div>
  );
}

/** A Fine-tune line. `.setting` rules sit on top, so these stack with SwitchRow; Settings' Row rules sit below. */
function TuneRow({ title, hint, children }: { title: string; hint: string; children: ReactNode }) {
  return (
    <div className="setting">
      <div className="setting-text">
        <p className="font-medium">{title}</p>
        <p className="text-[13px] text-dim">{hint}</p>
      </div>
      {children}
    </div>
  );
}

const MODE_NAMES = { d3d11: "Direct3D 11", vulkan: "Vulkan", opengl: "OpenGL" } as const;

function Choice<V extends string>({ label, hint, value, options, onChange }: { label: string; hint: string; value: V; options: readonly (readonly [V, string])[]; onChange: (v: V) => void }) {
  return (
    <select className="field w-auto" aria-label={label} aria-description={hint} value={value} onChange={(e) => onChange(e.target.value as V)}>
      {options.map(([v, name]) => <option key={v} value={v}>{name}</option>)}
    </select>
  );
}

/** What "Lighter rendering" writes, one choice each. */
function RenderFlagRows({ settings }: { settings: Settings }) {
  const set = useSetSettings();
  const r = settings.renderFlags;
  const put = (renderFlags: Partial<RenderFlags>) => set.mutate({ renderFlags: { ...r, ...renderFlags } });
  const auto = t("performance.robloxDefault");
  return (
    <div className="enter well mt-3 mb-1 px-5 py-1 [&>*:last-child]:border-b-0">
      <h3 className="font-semibold pt-4">{t("performance.graphicsTitle")}</h3>
      <p className="text-[13px] text-dim">{t("performance.graphicsHint")}</p>
      <Row title={t("performance.msaa")} hint={t("performance.hintMsaa")}>
        <Choice label={t("performance.msaa")} hint={t("performance.hintMsaa")} value={r.msaa} onChange={(msaa) => put({ msaa })} options={[["default", auto], ["x1", "x1"], ["x2", "x2"], ["x4", "x4"]]} />
      </Row>
      <Row title={t("performance.texture")} hint={t("performance.hintTexture")}>
        <Choice
          label={t("performance.texture")}
          hint={t("performance.hintTexture")}
          value={r.texture}
          onChange={(texture) => put({ texture })}
          options={[["default", auto], ["lowest", t("performance.texLowest")], ["low", t("performance.texLow")], ["medium", t("performance.texMedium")], ["high", t("performance.texHigh")]]}
        />
      </Row>
      <Row title={t("performance.renderMode")} hint={t("performance.hintRenderMode")}>
        <Choice label={t("performance.renderMode")} hint={t("performance.hintRenderMode")} value={r.mode} onChange={(mode) => put({ mode })} options={[["default", auto], ["d3d11", MODE_NAMES.d3d11], ["vulkan", MODE_NAMES.vulkan], ["opengl", MODE_NAMES.opengl]]} />
      </Row>
      <h3 className="font-semibold pt-4">{t("performance.stripTitle")}</h3>
      <SwitchRow title={t("performance.graySky")} hint={t("performance.hintGraySky")} more={t("performance.hintGraySky")} checked={r.graySky} onChange={(graySky) => put({ graySky })} />
      <SwitchRow title={t("performance.noGrass")} hint={t("performance.hintNoGrass")} more={t("performance.hintNoGrass")} checked={r.noGrass} onChange={(noGrass) => put({ noGrass })} />
      <SwitchRow title={t("performance.lowestFrm")} hint={t("performance.hintLowestFrm")} more={t("performance.hintLowestFrm")} checked={r.lowestFrm} onChange={(lowestFrm) => put({ lowestFrm })} />
      <SwitchRow title={t("performance.pauseVoxelizer")} hint={t("performance.hintPauseVoxelizer")} more={t("performance.hintPauseVoxelizer")} checked={r.pauseVoxelizer} onChange={(pauseVoxelizer) => put({ pauseVoxelizer })} />
    </div>
  );
}

/** Everything that changes how much the clients and RoALT itself use, and profiles to share it. */
export function PerformanceView({ settings }: { settings: Settings }) {
  const set = useSetSettings();
  const [previewFps, setFps] = usePreviewFps();
  const [incoming, setIncoming] = useState<Profile | null>(null);
  // Enter held on Apply lands on the preset that opened it.
  const closedAt = useRef(0);
  // An account's own cap beats a preset's, so it isn't what those clients get.
  const active = ownCaps(useAccounts().data ?? []) ? undefined : PRESETS.find((p) => isApplied(p.profile, settings));

  return (
    <main className="scroll flex-1 min-w-0 px-8 pt-5 pb-12">
      <div className="enter max-w-[1000px] flex flex-col gap-6">
        <header>
          <h1 className="page-title">{t("performance.title")}</h1>
          <p className="page-sub">{t("performance.intro")}</p>
        </header>

        <section aria-label={t("performance.presets")} className="grid gap-6" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(min(250px, 100%), 1fr))" }}>
          {PRESETS.map((p) => (
            <button key={p.label} className="btn preset" aria-pressed={p === active} aria-haspopup="dialog" onClick={() => Date.now() - closedAt.current > 400 && setIncoming(p.profile)}>
              <span className="flex items-center justify-between gap-2.5 w-full">
                <span className="section-title">{t(p.label)}</span>
                <span aria-hidden="true" className="tick tick-round"><Icon name="check" size={14} /></span>
              </span>
              <span className="font-normal text-dim">{t(p.tag)}</span>
              <span className="flex flex-col gap-1.5 text-[13px] font-normal">
                {p.facts().map((f) => <span key={f}>{f}</span>)}
              </span>
            </button>
          ))}
        </section>

        <section aria-labelledby="perf-tune" className="panel p-5">
          <div className="flex flex-wrap items-center justify-between gap-2 pb-3.5">
            <h2 id="perf-tune" className="section-title">{t("performance.fineTune")}</h2>
            <span className="text-[13px] text-dim">{active ? t("performance.using", { name: t(active.label) }) : t("performance.custom")}</span>
          </div>
          <FpsRow settings={settings} />
          <SwitchRow title={t("settings.lowGraphics")} hint={t("performance.hintLow")} more={t("settings.lowGraphicsHint")} checked={settings.lowGraphics} onChange={(lowGraphics) => set.mutate({ lowGraphics })} />
          <SwitchRow title={t("settings.lighterRendering")} hint={t("performance.hintLight")} more={t("performance.hintLight")} checked={settings.lighterRendering} onChange={(lighterRendering) => set.mutate({ lighterRendering })} />
          {settings.lighterRendering && <RenderFlagRows settings={settings} />}
          <h3 id="perf-cpu" className="font-semibold pt-5 pb-2">{t("performance.section.cpuMemory")}</h3>
          <TuneRow title={t("settings.adaptive")} hint={t("settings.adaptiveHint")}>
            <Switch label={t("settings.adaptive")} checked={settings.adaptive.enabled} onChange={(enabled) => set.mutate({ adaptive: { ...settings.adaptive, enabled } })} />
          </TuneRow>
          {settings.adaptive.enabled && (
            <TuneRow title={t("settings.inBackground")} hint={t("settings.adaptiveBgHint")}>
              <div className="flex flex-wrap items-center gap-3">
                <Segmented
                  mono
                  label={t("settings.adaptiveBgCpu")}
                  value={settings.adaptive.background.cpuCap ?? 0}
                  options={[0, 50, 25, 10, 5].map((c) => ({ value: c, label: c ? t("settings.percent", { n: c }) : t("common.off") }))}
                  onChange={(c) => set.mutate({ adaptive: { ...settings.adaptive, background: { ...settings.adaptive.background, cpuCap: c || null } } })}
                />
                <label className="flex items-center gap-2">
                  <span className="text-xs text-dim">{t("settings.efficiency")}</span>
                  <Switch
                    label={t("settings.adaptiveBgEco")}
                    checked={settings.adaptive.background.eco}
                    onChange={(eco) => set.mutate({ adaptive: { ...settings.adaptive, background: { ...settings.adaptive.background, eco } } })}
                  />
                </label>
                <Segmented
                  mono
                  label={t("settings.adaptiveBgTrim")}
                  value={settings.adaptive.background.trimEveryMin ?? -1}
                  options={[-1, 0, 5, 15, 30, 60].map((m) => ({ value: m, label: m < 0 ? t("common.default") : m ? t("settings.minutesShort", { n: m }) : t("common.off") }))}
                  onChange={(m) => set.mutate({ adaptive: { ...settings.adaptive, background: { ...settings.adaptive.background, trimEveryMin: m < 0 ? null : m } } })}
                />
              </div>
            </TuneRow>
          )}
          <TuneRow title={t("settings.fairCap")} hint={t("settings.fairCapHint")}>
            <Switch label={t("settings.fairCap")} checked={settings.fairCap} onChange={(fairCap) => set.mutate({ fairCap })} />
          </TuneRow>
          <TuneRow title={t("settings.trim")} hint={t("settings.trimHint")}>
            <NumberField unit={t("settings.unit.minutesOff")} short={t("settings.unit.min")} min={0} max={1440} value={settings.trimEveryMin} onCommit={(trimEveryMin) => set.mutate({ trimEveryMin })} />
          </TuneRow>
          <TuneRow title={t("settings.spread")} hint={t("settings.spreadHint")}>
            <Switch label={t("settings.spread")} checked={settings.autoSpread} onChange={(autoSpread) => set.mutate({ autoSpread })} />
          </TuneRow>
          <SwitchRow title={t("settings.minimize")} hint={t("performance.hintMinimize")} more={t("settings.minimizeHint")} checked={settings.minimizeOnLaunch} onChange={(minimizeOnLaunch) => set.mutate({ minimizeOnLaunch })} />
          <SwitchRow title={t("performance.tinyOnLaunch")} hint={t("performance.hintTinyOnLaunch")} more={t("performance.hintTinySize")} checked={settings.tinyOnLaunch} onChange={(tinyOnLaunch) => set.mutate({ tinyOnLaunch })} />
          <TuneRow title={t("performance.tinySize")} hint={t("performance.hintTinySize")}>
            <div className="flex flex-wrap items-center gap-2">
              <NumberField unit={t("performance.tinyWidth")} short="×" min={16} max={7680} value={settings.microWidth} onCommit={(microWidth) => set.mutate({ microWidth })} />
              <NumberField unit={t("performance.tinyHeight")} short="px" min={16} max={4320} value={settings.microHeight} onCommit={(microHeight) => set.mutate({ microHeight })} />
            </div>
          </TuneRow>
          <TuneRow title={t("settings.memRestart")} hint={t("settings.memRestartHint")}>
            <NumberField unit={t("settings.unit.mbOff")} short="MB" min={0} max={65536} value={settings.memRestartMb} onCommit={(memRestartMb) => set.mutate({ memRestartMb })} />
          </TuneRow>
          <TuneRow title={t("settings.memThrottle")} hint={t("settings.memThrottleHint")}>
            <div className="flex flex-wrap items-center gap-3">
              <NumberField
                unit={t("settings.unit.mbOff")}
                short="MB"
                min={0}
                max={65536}
                value={settings.memThrottle.mb}
                onCommit={(mb) => set.mutate({ memThrottle: { ...settings.memThrottle, mb } })}
              />
              <Segmented
                mono
                label={t("settings.memThrottleCpu")}
                value={settings.memThrottle.cpuCap}
                options={[25, 10, 5, 2].map((c) => ({ value: c, label: t("settings.percent", { n: c }) }))}
                onChange={(cpuCap) => set.mutate({ memThrottle: { ...settings.memThrottle, cpuCap } })}
              />
            </div>
          </TuneRow>
          <h3 id="perf-roalt" className="font-semibold pt-5 pb-2">{t("performance.section.roalt")}</h3>
          <TuneRow title={t("dashboard.previewFps")} hint={t("dashboard.previewFpsHint")}>
            <Segmented mono label={t("dashboard.previewFps")} value={previewFps} options={PREVIEW_FPS.map((f) => ({ value: f, label: t("dashboard.fps", { fps: f }) }))} onChange={setFps} />
          </TuneRow>
          <h3 id="perf-network" className="font-semibold pt-5 pb-2 border-b border-line">{t("performance.section.network")}</h3>
          <div className="[&>*]:border-b-0 [&>*]:pb-0">
            <BandwidthRow />
          </div>
        </section>

        <ShareRow onProfile={setIncoming} />
      </div>
      <ReviewDialog profile={incoming} settings={settings} onClose={() => { closedAt.current = Date.now(); setIncoming(null); }} />
    </main>
  );
}

const noThrottle = { mb: 0, cpuCap: 10 };
const lightest: RenderFlags = { msaa: "x1", texture: "lowest", mode: "default", graySky: false, noGrass: true, lowestFrm: true, pauseVoxelizer: false };
/** What leaving Max alts must undo: a profile only changes what it names. */
const normalWindows = { tinyOnLaunch: false, microWidth: 320, microHeight: 240, fairCap: false };
/** One-click setups: a profile each, reviewed like any other. */
const PRESETS: readonly { label: Key; tag: Key; facts: () => string[]; profile: Profile }[] = [
  {
    label: "performance.preset.maxAlts",
    tag: "performance.tagMaxAlts",
    facts: () => [
      t("dashboard.fps", { fps: 10 }), t("settings.lowGraphics"), t("performance.tinyOnLaunch"), t("settings.minimize"), t("performance.factFair", { n: 25 }),
      t("performance.factMainAccount"), t("performance.factBootstrapper"), t("performance.factAfk"), t("performance.factNextLaunch"),
    ],
    profile: {
      fpsCap: 10, lowGraphics: true, lighterRendering: true, renderFlags: { ...lightest, graySky: true, pauseVoxelizer: true }, minimizeOnLaunch: true, tinyOnLaunch: true, microWidth: 240, microHeight: 180, memRestartMb: 0, memThrottle: noThrottle, trimEveryMin: 15, autoSpread: true, fairCap: true,
      adaptive: { enabled: true, background: { cpuCap: 25, eco: true, trimEveryMin: null, affinity: null } }, previewFps: 5,
    },
  },
  {
    label: "performance.preset.balanced",
    tag: "performance.tagBalanced",
    facts: () => [t("dashboard.fps", { fps: 30 }), t("settings.lowGraphics"), t("performance.factOnScreen"), t("performance.factBg", { n: 50 })],
    profile: {
      fpsCap: 30, lowGraphics: true, lighterRendering: true, renderFlags: lightest, minimizeOnLaunch: false, ...normalWindows, memRestartMb: 0, memThrottle: noThrottle, trimEveryMin: 30, autoSpread: true,
      adaptive: { enabled: true, background: { cpuCap: 50, eco: true, trimEveryMin: null, affinity: null } }, previewFps: 15,
    },
  },
  {
    label: "performance.preset.defaults",
    tag: "performance.tagDefaults",
    facts: () => [t("performance.factNoFps"), t("performance.factOwnGraphics"), t("performance.factOnScreen"), t("performance.factNoCpu")],
    profile: {
      fpsCap: 0, lowGraphics: false, lighterRendering: false, minimizeOnLaunch: false, ...normalWindows, memRestartMb: 0, memThrottle: noThrottle, trimEveryMin: 0, autoSpread: false,
      adaptive: { enabled: false, background: { cpuCap: 25, eco: true, trimEveryMin: null, affinity: null } }, previewFps: 15,
    },
  },
];

/** Every setting the profile names is already so. */
function isApplied(profile: Profile, settings: Settings) {
  return (Object.keys(LABELS) as Field[]).every((k) => profile[k] === undefined || profile[k] === null || JSON.stringify(profile[k]) === JSON.stringify(settings[k]));
}

/** Export, import and share the settings above. */
function ShareRow({ onProfile }: { onProfile: (p: Profile) => void }) {
  const [pasteOpen, setPasteOpen] = useState(false);

  async function exportFile() {
    try {
      const path = await saveFile({ defaultPath: `RoALT-performance-${new Date().toISOString().slice(0, 10)}.json`, filters: profileFilter() });
      if (!path) return;
      await api.performanceExport(path, storedFps());
      toast(t("performance.exported"));
    } catch (e) {
      toastError(e);
    }
  }

  async function importFile() {
    try {
      const path = await openFile({ multiple: false, directory: false, filters: profileFilter() });
      if (path) onProfile(await api.performanceRead(path));
    } catch (e) {
      toastError(e);
    }
  }

  async function copyCode() {
    try {
      await writeText(toCode(await api.performanceProfile(storedFps())));
      toast(t("performance.codeCopied"));
    } catch (e) {
      toastError(e);
    }
  }

  return (
    <section aria-labelledby="perf-share" className="panel flex flex-wrap items-center gap-x-5 gap-y-3.5 p-5">
      <div className="flex-[1_1_320px] min-w-0">
        <h2 id="perf-share" className="section-title">{t("performance.shareTitle")}</h2>
        <p className="text-[13px] text-dim mt-1">{t("performance.shareHint")}</p>
      </div>
      <div className="flex flex-wrap gap-3">
        <button className="btn" onClick={copyCode}>{t("performance.copyCode")}</button>
        <button className="btn" onClick={() => setPasteOpen(true)}>{t("performance.pasteCode")}</button>
        <button className="btn btn-ghost" onClick={exportFile}>{t("performance.export")}</button>
        <button className="btn btn-ghost" onClick={importFile}>{t("performance.import")}</button>
      </div>
      <PasteDialog open={pasteOpen} onClose={() => setPasteOpen(false)} onProfile={(p) => { setPasteOpen(false); onProfile(p); }} />
    </section>
  );
}

function PasteDialog({ open, onClose, onProfile }: { open: boolean; onClose: () => void; onProfile: (p: Profile) => void }) {
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (open) return;
    setCode("");
    setError(null);
  }, [open]);

  async function submit(e: FormEvent) {
    e.preventDefault();
    const json = fromCode(code);
    if (json === null) return setError(t("performance.badCode"));
    setBusy(true);
    try {
      onProfile(await api.performanceParse(json));
    } catch (err) {
      setError(friendly(asError(err)));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onClose={onClose} title={t("performance.pasteTitle")}>
      <form onSubmit={submit} className="flex flex-col gap-4 p-5">
        <label className="flex flex-col gap-1.5">
          <span className="label">{t("performance.pasteLabel")}</span>
          <textarea
            className="field mono text-xs"
            rows={4}
            value={code}
            onChange={(e) => setCode(e.target.value)}
            autoComplete="off"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck={false}
            placeholder={`${CODE}…`}
            aria-invalid={!!error}
          />
        </label>
        {error && <div role="alert" className="alert">{error}</div>}
        <div className="flex justify-end gap-2 pt-1">
          <button type="button" className="btn" onClick={onClose}>{t("common.cancel")}</button>
          <button type="submit" className="btn btn-primary" disabled={busy || !code.trim()}>{t("performance.review")}</button>
        </div>
      </form>
    </Dialog>
  );
}

type Field = Exclude<keyof Profile, "previewFps">;
const LABELS: Record<Field, Key> = {
  fpsCap: "settings.fpsCap",
  lowGraphics: "settings.lowGraphics",
  lighterRendering: "settings.lighterRendering",
  renderFlags: "performance.graphicsTitle",
  minimizeOnLaunch: "settings.minimize",
  microWidth: "performance.tinyWidth",
  microHeight: "performance.tinyHeight",
  tinyOnLaunch: "performance.tinyOnLaunch",
  memRestartMb: "settings.memRestart",
  memThrottle: "settings.memThrottle",
  trimEveryMin: "settings.trim",
  autoSpread: "settings.spread",
  fairCap: "settings.fairCap",
  adaptive: "settings.adaptive",
};

const capitalize = (s: string) => s[0].toUpperCase() + s.slice(1);
const pct = (n: number | null) => (n ? t("settings.percent", { n }) : t("common.off"));

/** A setting's value as the review shows it. */
function shown(k: Field, v: Profile[Field] | Settings[Field]): string {
  if (v === undefined || v === null) return "";
  if (typeof v === "boolean") return v ? t("common.on") : t("common.off");
  if (typeof v === "number") return k === "memRestartMb" && v ? `${fmtNum(v)} MB` : fmtNum(v);
  if ("msaa" in v) {
    const on = [[v.graySky, "performance.graySky"], [v.noGrass, "performance.noGrass"], [v.lowestFrm, "performance.lowestFrm"], [v.pauseVoxelizer, "performance.pauseVoxelizer"]] as const;
    return [v.msaa === "default" ? null : `MSAA ${v.msaa}`, v.texture === "default" ? null : `${t("performance.texture")}: ${t(`performance.tex${capitalize(v.texture)}` as Key)}`, v.mode === "default" ? null : MODE_NAMES[v.mode], ...on.map(([b, k]) => (b ? t(k) : null))].filter(Boolean).join(" · ") || t("performance.robloxDefault");
  }
  if ("mb" in v) return `${v.mb ? `${fmtNum(v.mb)} MB` : t("common.off")} · ${pct(v.cpuCap)}`;
  const b = v.background;
  const trim = b.trimEveryMin === null ? t("common.default") : b.trimEveryMin ? t("settings.minutesShort", { n: b.trimEveryMin }) : t("common.off");
  return `${v.enabled ? t("common.on") : t("common.off")} · ${pct(b.cpuCap)} · ${t("settings.efficiency")} ${b.eco ? t("common.on") : t("common.off")} · ${trim}`;
}

function ReviewDialog({ profile, settings, onClose }: { profile: Profile | null; settings: Settings; onClose: () => void }) {
  const set = useSetSettings();
  const update = useUpdateAccount();
  const accounts = useAccounts().data ?? [];
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (profile) setError(null);
  }, [profile]);
  const changes = profile
    ? (Object.keys(LABELS) as Field[])
        .filter((k) => profile[k] !== undefined && profile[k] !== null && JSON.stringify(profile[k]) !== JSON.stringify(settings[k]))
        .map((k) => ({ label: t(LABELS[k]), from: shown(k, settings[k]), to: shown(k, profile[k]) }))
    : [];
  const fps = profile?.previewFps as PreviewFps | undefined;
  if (fps !== undefined && fps !== null && fps !== storedFps()) {
    changes.push({ label: t("dashboard.previewFps"), from: t("dashboard.fps", { fps: storedFps() }), to: t("dashboard.fps", { fps }) });
  }
  // A profile's FPS cap is what its clients get: an account's own would win over it at launch.
  const clearing = typeof profile?.fpsCap === "number" ? accounts.filter((a) => a.fpsCap) : [];
  if (clearing.length) changes.push({ label: t("performance.accountCaps"), from: ownCaps(clearing), to: t("performance.capsCleared") });

  async function apply() {
    if (!profile) return;
    setBusy(true);
    setError(null);
    try {
      const { previewFps, ...patch } = profile;
      await set.mutateAsync(patch);
      for (const a of clearing) await update.mutateAsync({ id: a.id, patch: { fpsCap: 0 } });
      if (previewFps) setPreviewFps(previewFps as PreviewFps);
      toast(t("performance.applied"));
      onClose();
    } catch (e) {
      setError(friendly(asError(e)));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={profile !== null} onClose={onClose} title={t("performance.reviewTitle")} width={480}>
      <div className="flex flex-col gap-4 px-5 pb-5">
        {changes.length === 0 ? (
          <p className="text-dim">{t("performance.profileSame")}</p>
        ) : (
          <>
            <p className="text-dim">{t("performance.reviewIntro")}</p>
            <ul className="well flex flex-col px-1">
              {changes.map((c) => (
                <li key={c.label} className="flex items-baseline justify-between gap-4 px-3 py-2.5 border-b border-line last:border-b-0">
                  <span>{c.label}</span>
                  <span className="mono text-xs text-end">
                    <span className="text-dim">{c.from}</span> → {c.to}
                  </span>
                </li>
              ))}
            </ul>
          </>
        )}
        {error && <div role="alert" className="alert">{error}</div>}
        <div className="flex justify-end gap-2 pt-1">
          <button className="btn" onClick={onClose}>{changes.length ? t("common.cancel") : t("common.close")}</button>
          {changes.length > 0 && (
            <button className="btn btn-primary" onClick={apply} disabled={busy} data-autofocus>
              {t("performance.apply")}
            </button>
          )}
        </div>
      </div>
    </Dialog>
  );
}
