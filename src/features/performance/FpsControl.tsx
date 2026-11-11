import { useState } from "react";
import { useAccounts } from "../../data";
import { t } from "../../i18n";
import { OffOrNumberField } from "../../ui/controls";
import { Morph } from "../../ui/motion";
import { useRow } from "../settings/kit/rowError";
import { SettingRow } from "../settings/kit/SettingRow";
import { say, type Control } from "../settings/registry";
import { ownCaps } from "./presets";

/** Off, or a cap the field and the slider share: the slider for the feel of it, saved on release.
 * Switched on, both slide out from the switch. */
function FpsControl() {
  const { entry, ctx, write, describedBy } = useRow();
  const range = entry.control as Extract<Control, { kind: "offOrValue" }>;
  const saved = ctx.s.fpsCap;
  const [fps, setFps] = useState(saved);
  const [was, setWas] = useState(saved);
  if (was !== saved) {
    setWas(saved);
    setFps(saved);
  }
  const commit = (n: number) => {
    setFps(n);
    if (n !== saved) void write({ fpsCap: n });
  };
  const label = t(entry.label);
  return (
    // The slider's own Morph beside the field's, not around it: nested, the outer spring chases the inner.
    <span className="flex items-center justify-end">
      <Morph>
        {fps !== 0 && (
          <input
            type="range"
            className="range perf-fps-range me-3"
            min={range.min}
            max={range.max}
            step={5}
            value={fps}
            aria-label={label}
            aria-describedby={describedBy}
            onChange={(e) => setFps(Number(e.target.value))}
            // The draft, not the slider's value: the slider snaps 144 to its grid (145) just by being there.
            onPointerUp={() => commit(fps)}
            onKeyUp={() => commit(fps)}
            onBlur={() => commit(fps)}
          />
        )}
      </Morph>
      <OffOrNumberField label={label} value={fps} range={range} unit={say(range.unit)} short={say(range.short)} onCommit={commit} describedBy={describedBy} />
    </span>
  );
}

/** The FPS cap; the accounts whose own cap wins over it are named under it. */
export function FpsRow() {
  const own = ownCaps(useAccounts().data ?? []);
  return (
    <SettingRow
      id="fpsCap"
      detail={
        own && (
          <p className="alert perf-own">
            <span className="line-clamp-2" data-tip={own}>{t("performance.fpsOverridden", { names: own })}</span>
          </p>
        )
      }
    >
      <FpsControl />
    </SettingRow>
  );
}
