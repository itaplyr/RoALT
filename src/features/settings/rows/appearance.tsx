import { useEffect, useId, useState } from "react";
import { motion } from "motion/react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { api } from "../../../api";
import { qk } from "../../../data";
import { t, type Key } from "../../../i18n";
import { rovingKeys } from "../../../ui/a11y";
import { Button } from "../../../ui/Button";
import { Segmented, Switch } from "../../../ui/controls";
import { Icon } from "../../../ui/Icon";
import { Collapse, lid, Morph, setMotionPreference, SPRING, Swap, useMotionPreference } from "../../../ui/motion";
import { playSound, previewSound, setSoundPreferences, useSoundPreferences } from "../../../ui/sounds";
import { useRow } from "../kit/rowError";
import { SettingRow } from "../kit/SettingRow";

/** The presets after Monochrome (""): the fill and its name. Any other colour comes from the picker. */
const ACCENTS: [string, Key][] = [
  ["#c6f432", "settings.accent.lime"],
  ["#ff4fd8", "settings.accent.neon"],
  ["#f48fb8", "settings.accent.sakura"],
  ["#e2603f", "settings.accent.brick"],
  ["#ff9a3c", "settings.accent.orange"],
  ["#ffd23f", "settings.accent.gold"],
  ["#2ee6c5", "settings.accent.mint"],
  ["#4f8cff", "settings.accent.blue"],
  ["#a66bff", "settings.accent.violet"],
];
const IMAGES = ["png", "jpg", "jpeg", "webp", "gif"];

/** Monochrome (white on dark, ink on light), the presets, and any colour. The ring that marks the
 * pick travels to the new one. */
function AccentPicker() {
  const { ctx, write } = useRow();
  const ring = useId();
  const value = ctx.s.accent;
  const custom = !!value && !ACCENTS.some(([v]) => v === value);
  const pick = (accent: string) => accent !== value && void write({ accent });
  const mark = (on: boolean) => on && <motion.span layoutId={lid("accent", ring)} className="set-ring" transition={SPRING.snappy} />;
  // One Tab stop, on the pick; the arrows go between them.
  const stop = (on: boolean) => (on ? 0 : -1);
  return (
    <div
      role="group"
      aria-label={t("settings.accentColour")}
      className="set-swatches"
      // rovingKeys leaves inputs to their own arrows; the colour input has none, so it is handed
      // the group as its target and the arrows leave it too.
      onKeyDown={(e) => rovingKeys(e.target instanceof HTMLInputElement ? Object.create(e, { target: { value: e.currentTarget } }) : e, ".swatch:is(button), .set-custom input")}
    >
      <button type="button" className="swatch set-mono" tabIndex={stop(!value)} aria-pressed={!value} aria-label={t("settings.accent.mono")} data-tip={t("settings.accent.mono")} onClick={() => pick("")}>
        <span aria-hidden="true" className="set-mono-half" />
        {mark(!value)}
      </button>
      {ACCENTS.map(([v, name]) => (
        <button key={v} type="button" className="swatch" style={{ background: v }} tabIndex={stop(v === value)} aria-pressed={v === value} aria-label={t(name)} data-tip={t(name)} onClick={() => pick(v)}>
          {mark(v === value)}
        </button>
      ))}
      <label className="swatch set-custom" data-on={custom || undefined} data-tip={t("settings.anyColour")} style={custom ? { background: value } : undefined}>
        {!custom && <Icon name="plus" size={14} />}
        <span className="sr-only">{t(custom ? "settings.customColourOn" : "settings.customColour")}</span>
        <input type="color" tabIndex={stop(custom)} value={value || "#f5f5f5"} onChange={(e) => pick(e.target.value)} />
        {mark(custom)}
      </label>
    </div>
  );
}

export function AccentRow() {
  return (
    <SettingRow id="accent">
      <AccentPicker />
    </SettingRow>
  );
}

/** The picture as a small thumbnail, from the bytes the theme already holds. */
function useImageUrl(bytes: ArrayBuffer | undefined) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!bytes?.byteLength) return setUrl(null);
    const u = URL.createObjectURL(new Blob([bytes]));
    setUrl(u);
    return () => URL.revokeObjectURL(u);
  }, [bytes]);
  return url;
}

/** Choose a picture, or with one set: its thumbnail, Change… and Remove. How much the theme covers it
 * is the sub-row under this one (the registry's backgroundDim). */
function BackgroundControls() {
  const row = useRow();
  const qc = useQueryClient();
  const bytes = useQuery({ queryKey: qk.background, queryFn: api.backgroundGet, staleTime: Infinity }).data;
  const url = useImageUrl(bytes);
  const [busy, setBusy] = useState(false);
  const has = row.ctx.background;
  const run = (p: Promise<unknown>) => {
    setBusy(true);
    row.setError(null);
    p.then(() => qc.invalidateQueries({ queryKey: qk.background }))
      .then(() => row.refocus(), row.fail)
      .finally(() => setBusy(false));
  };
  async function pick() {
    const path = await openFile({ multiple: false, directory: false, filters: [{ name: t("settings.images"), extensions: IMAGES }], title: t("settings.backgroundDialog") }).catch(row.fail);
    if (path) run(api.backgroundSet(path));
  }
  // The main button stays the same element as its words change, so it keeps the focus.
  return (
    <Morph innerClassName="gap-2">
      {has && url && <img src={url} alt="" className="set-thumb enter" />}
      <Button size="md" busy={busy} onClick={() => !busy && void pick()}>
        <Swap k={has ? "change" : "choose"}>{has ? t("settings.change") : t("settings.choose")}</Swap>
      </Button>
      {has && (
        <Button size="md" variant="ghost" className="enter" onClick={() => !busy && run(api.backgroundClear())}>
          {t("common.remove")}
        </Button>
      )}
    </Morph>
  );
}

export function BackgroundRow() {
  return (
    <SettingRow id="background">
      <BackgroundControls />
    </SettingRow>
  );
}

function SoundSwitch({ on }: { on: boolean }) {
  const { describedBy } = useRow();
  return <Switch label={t("settings.soundEffects")} checked={on} onChange={(enabled) => setSoundPreferences({ enabled })} describedBy={describedBy} />;
}

/** This PC's own choice (not a Settings field, kept across restarts): liquid motion (the default),
 * reduced to fades, or none. It applies at once; the hint follows it. */
export function MotionRow() {
  const pref = useMotionPreference();
  return (
    <SettingRow id="motion">
      <Segmented
        label={t("settings.motion")}
        value={pref}
        onChange={setMotionPreference}
        options={[
          { value: "on", label: t("settings.motionOn") },
          { value: "reduced", label: t("settings.motionReduced") },
          { value: "off", label: t("settings.motionOff") },
        ]}
      />
    </SettingRow>
  );
}

/** This PC's own choice (not a Settings field): on or off, and while on, the volume with a sample. */
export function SoundRow() {
  const sound = useSoundPreferences();
  const id = useId();
  const pct = Math.round(sound.volume * 100);
  return (
    <SettingRow
      id="sounds"
      below={
        <Collapse open={sound.enabled}>
          <div className="set-subs-pad">
            <div className="set-subs">
              <div className="set-row set-line">
                <label htmlFor={id} className="set-text set-label">{t("settings.soundVolume")}</label>
                <div className="set-control">
                  <input
                    id={id}
                    className="range set-volume"
                    type="range"
                    min={0}
                    max={100}
                    step={5}
                    value={pct}
                    onChange={(e) => setSoundPreferences({ volume: Number(e.target.value) / 100 })}
                    onPointerUp={() => playSound("click")}
                    onKeyUp={(e) => ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(e.key) && playSound("click")}
                  />
                  <span className="mono text-fg-2 w-10 text-end">{pct}%</span>
                  <Button size="md" data-sound="off" disabled={!pct} onClick={previewSound}>{t("settings.soundPreview")}</Button>
                </div>
              </div>
            </div>
          </div>
        </Collapse>
      }
    >
      <SoundSwitch on={sound.enabled} />
    </SettingRow>
  );
}
