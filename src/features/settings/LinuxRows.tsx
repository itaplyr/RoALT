import { useId, useState, type ReactNode } from "react";
import type { Settings, SettingsPatch } from "../../api";
import { useSetSettings } from "../../data";
import { t } from "../../i18n";
import { NumberField, Switch } from "../../ui/controls";
import { errorText } from "./kit/rowError";
import { MocktailNotice } from "./MocktailNotice";

/** Settings this fork keeps that upstream's generated bindings predate. */
export type LinuxSettings = Settings & {
  /** Start Cordial clients under `cage`, with no game window. */
  headless: boolean;
  /** A closed game window leaves the client running until RoALT closes it. */
  keepRunningOnClose: boolean;
  /** Render size a headless client reports to the game. */
  headlessWidth: number;
  headlessHeight: number;
};

type LinuxPatch = Partial<Pick<LinuxSettings, "headless" | "keepRunningOnClose" | "headlessWidth" | "headlessHeight">>;

/** The bindings lane regenerates `Settings`/`SettingsPatch` from the Rust model; until then the
 * Linux-only fields still exist at runtime, so writes go through the same command unchanged. */
const patch = (p: LinuxPatch) => p as unknown as SettingsPatch;

/** One of this fork's own settings, drawn like a registry row so it sits naturally in the panel. */
function LinuxRow({ id, label, hint, hintId, children, error }: { id: string; label: string; hint: string; hintId: string; children: ReactNode; error: string | null }) {
  return (
    <div id={`set-${id}`} role="group" aria-label={label} className="set-row">
      <div className="set-line">
        <div className="set-text">
          <div className="set-title">
            <span className="set-label">{label}</span>
          </div>
          <p id={hintId} className="set-hint">{hint}</p>
        </div>
        <div className="set-control">{children}</div>
      </div>
      {error && (
        <p role="alert" className="alert set-error">
          {error}
        </p>
      )}
    </div>
  );
}

/** One Linux switch, with its own error line. */
function LinuxSwitchRow({ id, label, hint, field, checked }: { id: string; label: string; hint: string; field: "headless" | "keepRunningOnClose"; checked: boolean }) {
  const set = useSetSettings({ quiet: true });
  const [error, setError] = useState<string | null>(null);
  const hintId = useId();
  return (
    <LinuxRow id={id} label={label} hint={hint} hintId={hintId} error={error}>
      <Switch
        label={label}
        describedBy={hintId}
        checked={checked}
        onChange={(v) => {
          setError(null);
          set.mutate(patch(field === "headless" ? { headless: v } : { keepRunningOnClose: v }), { onError: (e) => setError(errorText(e)) });
        }}
      />
    </LinuxRow>
  );
}

/** The launching rows Linux keeps from before the registry: headless clients, and what closing a
 * client's window does. */
export function LinuxLaunchRows({ settings }: { settings: Settings }) {
  const s = settings as LinuxSettings;
  return (
    <>
      <div className="set-row">
        <MocktailNotice />
      </div>
      <LinuxSwitchRow id="headless" label={t("settings.headless")} hint={t("settings.headlessHint")} field="headless" checked={!!s.headless} />
      <LinuxSwitchRow id="keepRunning" label={t("settings.keepRunning")} hint={t("settings.keepRunningHint")} field="keepRunningOnClose" checked={!!s.keepRunningOnClose} />
    </>
  );
}

/** Headless resolution: what a headless client renders at, applied the next time one starts. */
export function HeadlessResolutionRow({ settings }: { settings: Settings }) {
  const s = settings as LinuxSettings;
  const set = useSetSettings({ quiet: true });
  const [error, setError] = useState<string | null>(null);
  const hintId = useId();
  const write = (p: LinuxPatch) => {
    setError(null);
    set.mutate(patch(p), { onError: (e) => setError(errorText(e)) });
  };
  return (
    <LinuxRow id="headlessResolution" label={t("performance.headlessResolution")} hint={t("performance.headlessResolutionHint")} hintId={hintId} error={error}>
      <NumberField
        unit={t("performance.headlessWidth")}
        short="×"
        min={320}
        max={7680}
        value={s.headlessWidth ?? 640}
        onCommit={(headlessWidth) => write({ headlessWidth })}
        onInvalid={setError}
        describedBy={hintId}
      />
      <NumberField
        unit={t("performance.headlessHeight")}
        short="px"
        min={240}
        max={4320}
        value={s.headlessHeight ?? 360}
        onCommit={(headlessHeight) => write({ headlessHeight })}
        onInvalid={setError}
        describedBy={hintId}
      />
    </LinuxRow>
  );
}
