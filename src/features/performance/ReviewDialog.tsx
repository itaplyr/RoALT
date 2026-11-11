import { useLayoutEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api, asError, type AccountSummary, type Profile, type Settings } from "../../api";
import { setAccounts, useAccounts, useInstances, useSetSettings } from "../../data";
import { fmtNum, t, type Key } from "../../i18n";
import { Button } from "../../ui/Button";
import { Dialog, DialogBody, DialogFooter } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { Collapse, riseIn } from "../../ui/motion";
import { friendly, toast } from "../../ui/Toast";
import { setPreviewFps, storedFps, type PreviewFps } from "../dashboard/previewFps";
import { isLive } from "../launch/Running";
import { FACTS_SHOWN, KEYS, LABELS, ownCaps, PRESETS, same, type ProfileKey } from "./presets";

const MODE_NAMES = { d3d11: "Direct3D 11", vulkan: "Vulkan", opengl: "OpenGL" } as const;
const onOff = (on: boolean) => t(on ? "common.on" : "common.off");
const pct = (n: number | null) => (n ? t("settings.percent", { n }) : t("common.off"));
const capitalize = (s: string) => s[0].toUpperCase() + s.slice(1);

/** A setting's value as the review shows it; 0 is Off where 0 turns the setting off. */
function valueText(k: ProfileKey, v: Profile[ProfileKey] | Settings[ProfileKey]): string {
  if (v == null) return "";
  if (typeof v === "boolean") return onOff(v);
  if (typeof v === "number") {
    if (k === "microWidth" || k === "microHeight") return fmtNum(v);
    if (!v) return t("common.off");
    return k === "fpsCap" ? t("dashboard.fps", { fps: v }) : k === "trimEveryMin" ? t("settings.minutes", { n: v }) : `${fmtNum(v)} MB`;
  }
  if ("msaa" in v) {
    const strip = [[v.graySky, "performance.graySky"], [v.noGrass, "performance.noGrass"], [v.lowestFrm, "performance.lowestFrm"], [v.pauseVoxelizer, "performance.pauseVoxelizer"]] as const;
    const parts = [
      v.msaa === "default" ? null : `MSAA ${v.msaa}`,
      v.texture === "default" ? null : `${t("performance.texture")}: ${t(`performance.tex${capitalize(v.texture)}` as Key)}`,
      v.mode === "default" ? null : MODE_NAMES[v.mode],
      ...strip.map(([on, key]) => (on ? t(key) : null)),
    ];
    return parts.filter(Boolean).join(" · ") || t("performance.robloxDefault");
  }
  if ("mb" in v) return `${v.mb ? `${fmtNum(v.mb)} MB` : t("common.off")} · ${pct(v.cpuCap)}`;
  const b = v.background;
  const trim = b.trimEveryMin === null ? t("common.default") : b.trimEveryMin ? t("settings.minutesShort", { n: b.trimEveryMin }) : t("common.off");
  return `${onOff(v.enabled)} · ${pct(b.cpuCap)} · ${t("settings.efficiency")} ${onOff(b.eco)} · ${trim}`;
}

/** What a profile changes, before anything does: each setting from → to, account caps it clears,
 * running clients it reshapes now. Apply sets them all; nothing changes on Cancel. */
export function ReviewDialog({ profile, settings, facts, onApplied, onClose }: {
  profile: Profile | null;
  settings: Settings;
  /** What the measurement adds (For this PC only). */
  facts?: string[];
  onApplied?: (next: Settings) => void;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const set = useSetSettings({ quiet: true });
  const accounts = useAccounts().data ?? [];
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Compared with what was there when it opened: Apply writes the cache before the save answers
  // (data.ts), and the list must not empty under the busy Apply.
  const [base, setBase] = useState(() => ({ settings, accounts, fps: storedFps() }));
  const [shownFor, setShownFor] = useState(profile);
  const list = useRef<HTMLDivElement>(null);
  if (shownFor !== profile) {
    setShownFor(profile);
    setBase({ settings, accounts, fps: storedFps() });
    setError(null);
  }
  const s = base.settings;
  // The rows arrive one after another as the dialog opens.
  useLayoutEffect(() => {
    if (profile) list.current?.querySelectorAll("[data-rise]").forEach((el, i) => riseIn(el, i));
  }, [profile]);

  const changes = profile
    ? KEYS.filter((k) => profile[k] != null && !same(profile[k], s[k])).map((k) => ({ label: t(LABELS[k]), from: valueText(k, s[k]), to: valueText(k, profile[k]) }))
    : [];
  const fps = profile?.previewFps as PreviewFps | undefined;
  if (fps != null && fps !== base.fps) changes.push({ label: t("dashboard.previewFps"), from: t("dashboard.fps", { fps: base.fps }), to: t("dashboard.fps", { fps }) });
  // A profile's FPS cap is what its clients get: an account's own looser one would win over it at launch.
  const cap = profile?.fpsCap;
  const clearing = typeof cap === "number" ? base.accounts.filter((a) => a.fpsCap && (cap === 0 || a.fpsCap > cap)) : [];
  if (clearing.length) changes.push({ label: t("performance.accountCaps"), from: ownCaps(clearing), to: t("performance.capsCleared") });
  // Windows change now; what a client reads at its start waits for its next launch.
  const running = (useInstances().data ?? []).filter(isLive);
  // One still loading gets these when it reaches a game.
  const reshape = running.filter((i) => i.state === "inGame" && !accounts.find((a) => a.id === i.accountId)?.fullQuality);
  // Only when this turns them on: a client since brought back stays so when nothing changes.
  const makeTiny = !!profile?.tinyOnLaunch && !s.tinyOnLaunch;
  const hide = !!profile?.minimizeOnLaunch && !s.minimizeOnLaunch;
  if (reshape.length && (makeTiny || hide)) changes.push({ label: t("performance.runningClients"), from: "", to: t("performance.runningNow") });
  // What its card had no room for.
  const notes = [...(PRESETS.find((p) => p.profile === profile)?.facts().slice(FACTS_SHOWN) ?? []), ...(facts ?? [])];
  const nextLaunch = (["fpsCap", "lowGraphics", "lighterRendering", "renderFlags"] as const).some((k) => profile?.[k] !== undefined && !same(profile[k], s[k]));

  async function apply() {
    if (!profile || busy) return;
    setBusy(true);
    setError(null);
    const cleared = new Map<number, AccountSummary>();
    try {
      const { previewFps, ...patch } = profile;
      const next = await set.mutateAsync(patch);
      onApplied?.(next);
      for (const a of clearing) {
        const saved = await api.accountUpdate(a.id, { fpsCap: 0 });
        cleared.set(saved.id, saved);
      }
      if (previewFps) setPreviewFps(previewFps as PreviewFps);
      await Promise.allSettled(reshape.flatMap((i) => [...(makeTiny && !i.micro ? [api.instanceMicro(i.id, true)] : []), ...(hide ? [api.instanceShow(i.id, "minimize")] : [])]));
      toast(t(running.length && nextLaunch ? "performance.appliedRelaunch" : "performance.applied"));
      onClose();
    } catch (e) {
      setError(friendly(asError(e)));
    } finally {
      // One cache write for all of them, the ones saved before a failure too: each redraws every account list.
      if (cleared.size) setAccounts(qc, (all) => all.map((x) => cleared.get(x.id) ?? x));
      setBusy(false);
    }
  }

  return (
    <Dialog open={profile !== null} onClose={onClose} title={t("performance.reviewTitle")} width={500}>
      <DialogBody className="perf-review-body">
        <div ref={list} className="flex flex-col gap-4">
          <p className="text-fg-2" data-rise>{t(changes.length ? "performance.reviewIntro" : "performance.profileSame")}</p>
          {changes.length > 0 && (
            <ul className="perf-diff">
              {changes.map((c) => (
                <li key={c.label} data-rise>
                  <span className="perf-diff-label">{c.label}</span>
                  <span className="perf-diff-value mono">
                    {c.from && <span className="text-fg-3">{c.from}</span>}
                    <span className="perf-diff-to">
                      {c.from && <Icon name="back" size={12} className="perf-diff-arrow" />}
                      {c.to}
                    </span>
                  </span>
                </li>
              ))}
            </ul>
          )}
          {notes.length > 0 && (
            <ul className="flex flex-col gap-1 text-[13px] text-fg-2">
              {notes.map((f) => (
                <li key={f} data-rise>{f}</li>
              ))}
            </ul>
          )}
          <Collapse open={!!error}>
            <p role="alert" className="alert text-[13px]">{error}</p>
          </Collapse>
        </div>
      </DialogBody>
      <DialogFooter>
        <Button size="md" onClick={onClose} data-autofocus={!changes.length || undefined}>
          {t(changes.length ? "common.cancel" : "common.close")}
        </Button>
        {changes.length > 0 && (
          // aria-disabled: disabled would drop the focus, and leave it nowhere after an error.
          <Button size="md" variant="primary" busy={busy} aria-disabled={busy || undefined} onClick={() => void apply()} data-autofocus>
            {t("performance.apply")}
          </Button>
        )}
      </DialogFooter>
    </Dialog>
  );
}
