import { useId, useRef, useState, type FormEvent } from "react";
import { motion } from "motion/react";
import { useQueryClient } from "@tanstack/react-query";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { api, type Settings } from "../../../api";
import type { Look } from "../../../bindings/Look";
import { ADVANCED, BUILTINS, builtinOf, lookTokens, PARTS, partLabel, readShared, type Builtin, type Part, type Shared } from "../../../app/look";
import { qk, useBackgroundUrl } from "../../../data";
import { t, type Key } from "../../../i18n";
import { rovingKeys } from "../../../ui/a11y";
import { Disclosure } from "../../../ui/Bits";
import { Button, IconButton } from "../../../ui/Button";
import { Segmented, Switch } from "../../../ui/controls";
import { ConfirmDialog, Dialog } from "../../../ui/Dialog";
import { Field } from "../../../ui/Field";
import { Icon } from "../../../ui/Icon";
import { Collapse, lid, Morph, setMotionPreference, SPRING, Swap, useMotionPreference } from "../../../ui/motion";
import { fromCode, toCode } from "../../../ui/shareCode";
import { playSound, previewSound, setSoundPreferences, useSoundPreferences } from "../../../ui/sounds";
import { useRow } from "../kit/rowError";
import { SettingRow } from "../kit/SettingRow";

/** The presets after Monochrome (""): the fill and its name. Any other colour comes from the picker. */
export const ACCENTS: [string, Key][] = [
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

/** Dark, Light, System or the user's own colours. Custom with none yet starts on what shows. */
function ThemePicker() {
  const { ctx, write } = useRow();
  const s = ctx.s;
  const pick = (theme: Settings["theme"]) => {
    if (theme === s.theme) return;
    const fresh = theme === "custom" && !Object.keys(s.look.colors).length;
    const base = document.documentElement.dataset.theme === "light" ? "light" : "dark";
    void write(fresh ? { theme, look: { base, colors: {} } } : { theme });
  };
  return (
    <Segmented
      label={t("settings.theme")}
      value={s.theme}
      onChange={pick}
      options={[
        { value: "dark" as const, label: t("settings.themeDark") },
        { value: "light" as const, label: t("settings.themeLight") },
        { value: "system" as const, label: t("settings.themeSystem") },
        { value: "custom" as const, label: t("settings.themeCustom") },
      ]}
    />
  );
}

export function ThemeRow() {
  return (
    <SettingRow id="theme">
      <ThemePicker />
    </SettingRow>
  );
}

/** Colours the user made (not one of the built-in themes as it came): worth asking before they go. */
const ownLook = (s: Settings) => s.theme === "custom" && Object.keys(s.look.colors).length > 0 && !builtinOf(s.look);

/** Replacing the user's own colours asks first; anything else just applies. Render `dialog`. */
function useReplace() {
  const { ctx } = useRow();
  const [pending, setPending] = useState<(() => void) | null>(null);
  const replace = (apply: () => void) => (ownLook(ctx.s) ? setPending(() => apply) : apply());
  const dialog = (
    <ConfirmDialog
      open={!!pending}
      onClose={() => setPending(null)}
      onConfirm={() => {
        pending?.();
        setPending(null);
      }}
      title={t("look.replaceTitle")}
      text={t("look.replaceText")}
      confirmLabel={t("look.replace")}
      safe
    />
  );
  return { replace, dialog };
}

/** A theme's colours, accent and corners, switched to Custom. */
const patchOf = (x: { look: Look; accent: string; corners: number }) => ({ theme: "custom" as const, look: x.look, accent: x.accent, corners: x.corners });

function ThemeChips() {
  const { ctx, write } = useRow();
  const { replace, dialog } = useReplace();
  const current = ctx.s.theme === "custom" ? builtinOf(ctx.s.look) : null;
  const use = (b: Builtin) => replace(() => void write(patchOf(b)));
  return (
    <div role="group" aria-label={t("look.themes")} className="flex flex-wrap justify-end gap-1.5">
      {BUILTINS.map((b) => (
        <button key={b.id} type="button" className="chip" aria-pressed={current?.id === b.id} onClick={() => use(b)}>
          <span aria-hidden="true" className="look-dots">
            {[b.look.colors.ground, b.look.colors.fg, b.accent].map((c, i) => (
              <i key={i} style={{ background: c }} />
            ))}
          </span>
          {t(b.label)}
        </button>
      ))}
      {dialog}
    </div>
  );
}

export function ThemesRow() {
  return (
    <SettingRow id="themes">
      <ThemeChips />
    </SettingRow>
  );
}

/** One part's colour: its swatch (the system picker), its name, and a reset once it was picked. */
function PartSwatch({ id, value, picked, onPick, onReset }: {
  id: Part;
  value: string;
  picked: boolean;
  onPick: (id: Part, colour: string) => void;
  onReset: (id: Part) => void;
}) {
  const name = t(partLabel(id));
  return (
    <div className="look-part">
      <label className="swatch set-custom look-swatch" style={{ background: value }}>
        <span className="sr-only">{name}</span>
        <input type="color" value={value} onChange={(e) => onPick(id, e.target.value)} />
      </label>
      <span className="look-name">{name}</span>
      {picked && <IconButton icon="refresh" size="sm" label={t("look.resetPart", { part: name })} onClick={() => onReset(id)} />}
    </div>
  );
}

/** How long the picker must rest before its colour is saved. */
const SAVE_AFTER_MS = 300;

/** The parts in plain words, the rest behind Advanced, what the look is built on, and Reset all. */
function LookEditor() {
  const { ctx, write } = useRow();
  const qc = useQueryClient();
  const look = ctx.s.look;
  const shown = lookTokens(look).colors;
  // A pick still waiting for the picker to rest. Not cleared on unmount: leaving Settings mid-drag still saves it.
  const pending = useRef(0);
  const save = (next: Look) => {
    clearTimeout(pending.current);
    void write({ look: next });
  };
  // The screen follows the picker at once (the cached settings, as a save would show them); the save
  // waits for it to rest. A refused save puts the cache back (useSetSettings).
  const pick = (id: Part, c: string) => {
    const colour = c.toLowerCase();
    const without = { ...look.colors };
    delete without[id];
    // The colour it shows unpicked is no pick: a cancelled picker puts that one back.
    const colors = lookTokens({ ...look, colors: without }).colors[id] === colour ? without : { ...without, [id]: colour };
    const next = { ...look, colors };
    qc.setQueryData<Settings>(qk.settings, (s) => (s ? { ...s, look: next } : s));
    clearTimeout(pending.current);
    pending.current = window.setTimeout(() => void write({ look: next }), SAVE_AFTER_MS);
  };
  const onReset = (id: Part) => {
    const colors = { ...look.colors };
    delete colors[id];
    save({ ...look, colors });
  };
  const swatch = (id: Part) => <PartSwatch key={id} id={id} value={look.colors[id] ?? shown[id]} picked={id in look.colors} onPick={pick} onReset={onReset} />;
  return (
    <div className="look-editor">
      <div className="look-parts">{PARTS.map(swatch)}</div>
      <Disclosure summary={t("look.advanced")} className="look-advanced">
        <div className="look-parts">{ADVANCED.map(swatch)}</div>
      </Disclosure>
      <div className="flex flex-wrap items-center gap-2">
        <Segmented
          label={t("look.base")}
          value={look.base}
          onChange={(base) => save({ ...look, base })}
          options={[
            { value: "dark" as const, label: t("settings.themeDark") },
            { value: "light" as const, label: t("settings.themeLight") },
          ]}
        />
        <Button size="md" variant="ghost" icon="refresh" disabled={!Object.keys(look.colors).length} onClick={() => save({ ...look, colors: {} })}>
          {t("look.resetAll")}
        </Button>
      </div>
    </div>
  );
}

export function LookRow() {
  return <SettingRow id="look" below={<LookEditor />} />;
}

/** The theme as a code to copy, and someone else's to paste. */
const THEME_CODE = "RoALT-THEME:";
function LookShare() {
  const { ctx, write, setResult, fail } = useRow();
  const { replace, dialog } = useReplace();
  const [pasting, setPasting] = useState(false);
  const s = ctx.s;
  const copy = () => {
    const shared: Shared = { v: 1, base: s.look.base, colors: s.look.colors, accent: s.accent, corners: s.corners };
    writeText(toCode(THEME_CODE, JSON.stringify(shared))).then(() => setResult(t("look.codeCopied")), fail);
  };
  return (
    <span className="flex flex-wrap justify-end gap-2">
      <Button size="md" onClick={copy}>{t("look.copyCode")}</Button>
      <Button size="md" aria-haspopup="dialog" onClick={() => setPasting(true)}>{t("look.pasteCode")}</Button>
      <PasteTheme
        open={pasting}
        onClose={() => setPasting(false)}
        onTheme={(x) => {
          setPasting(false);
          replace(() => void write(patchOf({ look: { base: x.base, colors: x.colors }, accent: x.accent, corners: x.corners })));
        }}
      />
      {dialog}
    </span>
  );
}

export function LookShareRow() {
  return (
    <SettingRow id="lookShare">
      <LookShare />
    </SettingRow>
  );
}

function PasteTheme({ open, onClose, onTheme }: { open: boolean; onClose: () => void; onTheme: (x: Shared) => void }) {
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [wasOpen, setWasOpen] = useState(open);
  // Closed, it forgets what was pasted.
  if (wasOpen !== open) {
    setWasOpen(open);
    if (!open) {
      setCode("");
      setError(null);
    }
  }
  const submit = (e: FormEvent) => {
    e.preventDefault();
    const json = fromCode(THEME_CODE, code);
    const theme = json === null ? null : readShared(json);
    if (!theme) return setError(t("look.badCode"));
    onTheme(theme);
  };
  return (
    <Dialog open={open} onClose={onClose} title={t("look.pasteTitle")}>
      <form onSubmit={submit}>
        <div className="dlg-body">
          <Field label={t("look.pasteLabel")} error={error}>
            {({ id, describedBy, invalid }) => (
              <textarea
                id={id}
                className="field mono text-xs"
                rows={4}
                value={code}
                onChange={(e) => {
                  setCode(e.target.value);
                  setError(null);
                }}
                autoComplete="off"
                autoCorrect="off"
                autoCapitalize="off"
                spellCheck={false}
                placeholder={`${THEME_CODE}…`}
                aria-invalid={invalid || undefined}
                aria-describedby={describedBy}
              />
            )}
          </Field>
        </div>
        <div className="dlg-foot">
          <Button size="md" onClick={onClose}>{t("common.cancel")}</Button>
          <Button size="md" type="submit" variant="primary" disabled={!code.trim()}>{t("look.apply")}</Button>
        </div>
      </form>
    </Dialog>
  );
}

/** Choose a picture, or with one set: its thumbnail, Change… and Remove. How much the theme covers it
 * is the sub-row under this one (the registry's backgroundDim). */
function BackgroundControls() {
  const row = useRow();
  const qc = useQueryClient();
  // The thumbnail is the theme's own URL: no second copy of the picture.
  const url = useBackgroundUrl();
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
