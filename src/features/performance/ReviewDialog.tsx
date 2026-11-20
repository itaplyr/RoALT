import { useLayoutEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api, asError, type AccountSummary, type Group, type Profile, type Settings } from "../../api";
import { qk, setAccounts, useAccounts, useGroups, useInstances, useRunAccounts, useSetSettings } from "../../data";
import { fmtList, fmtNum, t, type Key } from "../../i18n";
import { Button } from "../../ui/Button";
import { Segmented } from "../../ui/controls";
import { Dialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { Collapse, riseIn } from "../../ui/motion";
import { friendly, toast } from "../../ui/Toast";
import { fpsLabel, setPreviewFps, storedFps, type PreviewFps } from "../dashboard/previewFps";
import { AccountPicker } from "../launch/AccountPicker";
import { isLive } from "../launch/Running";
import { FACTS_SHOWN, KEYS, LABELS, named, ownCaps, perClient, PRESETS, same, SHARED, type ProfileKey } from "./presets";

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
    return k === "fpsCap" ? t("dashboard.fps", { fps: v }) : k === "trimEveryMin" ? t("settings.minutes", { n: v }) : t("monitor.mb", { n: fmtNum(v) });
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
  if ("mb" in v) return `${v.mb ? t("monitor.mb", { n: fmtNum(v.mb) }) : t("common.off")} · ${pct(v.cpuCap)}`;
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
  const groups = useGroups().data ?? [];
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Compared with what was there when it opened: Apply writes the cache before the save answers
  // (data.ts), and the list must not empty under the busy Apply.
  const [base, setBase] = useState(() => ({ settings, accounts, groups, fps: storedFps() }));
  const [shownFor, setShownFor] = useState(profile);
  // Every account (Settings), or only the picked ones (each keeps the profile as its own).
  const [scope, setScope] = useState<"all" | "chosen">("all");
  const [picked, setPicked] = useState<number[]>([]);
  const list = useRef<HTMLDivElement>(null);
  if (shownFor !== profile) {
    setShownFor(profile);
    setBase({ settings, accounts, groups, fps: storedFps() });
    setError(null);
    setScope("all");
    setPicked([]);
  }
  const s = base.settings;
  const chosen = scope === "chosen";
  const own = profile ? perClient(profile) : {};
  const targets = base.accounts.filter((a) => picked.includes(a.id));
  // The rows arrive one after another as the dialog opens.
  useLayoutEffect(() => {
    if (profile) list.current?.querySelectorAll("[data-rise]").forEach((el, i) => riseIn(el, i));
  }, [profile, scope]);

  // For chosen accounts: everything it gives them, whatever each has now.
  const changes = !profile
    ? []
    : chosen
      ? KEYS.filter((k) => own[k] != null).map((k) => ({ label: t(LABELS[k]), from: "", to: valueText(k, own[k]) }))
      : KEYS.filter((k) => profile[k] != null && !same(profile[k], s[k])).map((k) => ({ label: t(LABELS[k]), from: valueText(k, s[k]), to: valueText(k, profile[k]) }));
  const fps = profile?.previewFps as PreviewFps | undefined;
  if (!chosen && fps != null && fps !== base.fps) changes.push({ label: t("dashboard.previewFps"), from: fpsLabel(base.fps), to: fpsLabel(fps) });
  // A profile's FPS cap is what its clients get: an account's own looser one would win over it at launch.
  const cap = profile?.fpsCap;
  const looser = (c: number | null) => typeof cap === "number" && !!c && (cap === 0 || c > cap);
  const clearing = (chosen ? targets : base.accounts).filter((a) => looser(a.fpsCap));
  if (clearing.length) changes.push({ label: t("performance.accountCaps"), from: ownCaps(clearing), to: t("performance.capsCleared") });
  // A group's cap wins over Settings' too, but not over a chosen account's own profile.
  const clearingGroups = chosen ? [] : base.groups.filter((g) => looser(g.defaults.fpsCap));
  if (clearingGroups.length) changes.push({ label: t("performance.groupCaps"), from: fmtList(clearingGroups.map((g) => `${g.name} (${g.defaults.fpsCap})`)), to: t("performance.capsCleared") });
  // Windows change now; what a client reads at its start waits for its next launch.
  const running = (useInstances().data ?? []).filter((i) => isLive(i) && (!chosen || picked.includes(i.accountId)));
  // One still loading gets these when it reaches a game.
  const asRun = useRunAccounts() ?? [];
  const reshape = running.filter((i) => i.state === "inGame" && !asRun.find((a) => a.id === i.accountId)?.fullQuality);
  // Only when this turns them on: a client since brought back stays so when nothing changes. An
  // account's own profile decides for its clients, so applying to all leaves those alone.
  const turnsOn = (accountId: number, k: "tinyOnLaunch" | "minimizeOnLaunch") => {
    const had = base.accounts.find((a) => a.id === accountId)?.profile?.[k];
    return !!profile?.[k] && (chosen ? !(had ?? s[k]) : !s[k] && had == null);
  };
  const makeTiny = reshape.filter((i) => !i.micro && turnsOn(i.accountId, "tinyOnLaunch"));
  const hide = reshape.filter((i) => turnsOn(i.accountId, "minimizeOnLaunch"));
  if (makeTiny.length || hide.length) changes.push({ label: t("performance.runningClients"), from: "", to: t("performance.runningNow") });
  // What its card had no room for.
  const notes = [...(PRESETS.find((p) => p.profile === profile)?.facts().slice(FACTS_SHOWN) ?? []), ...(facts ?? [])];
  // What chosen accounts can't have of their own.
  const shared = chosen && profile ? SHARED.filter((k) => profile[k] != null) : [];
  if (shared.length) notes.push(t("performance.sharedStay", { list: fmtList(shared.map((k) => t(LABELS[k]))) }));
  const nextLaunch = (["fpsCap", "lowGraphics", "lighterRendering", "renderFlags"] as const).some((k) => profile?.[k] !== undefined && (chosen || !same(profile[k], s[k])));
  const ready = changes.length > 0 && (!chosen || targets.length > 0);

  async function apply() {
    if (!profile || busy || !ready) return;
    setBusy(true);
    setError(null);
    const cleared = new Map<number, AccountSummary>();
    const clearedGroups = new Map<number, Group>();
    try {
      if (chosen) {
        for (const a of targets) {
          const saved = await api.accountUpdate(a.id, { profile: own, ...(clearing.includes(a) ? { fpsCap: 0 } : {}) });
          cleared.set(saved.id, saved);
        }
      } else {
        const { previewFps, ...patch } = profile;
        const next = await set.mutateAsync(patch);
        onApplied?.(next);
        for (const a of clearing) {
          const saved = await api.accountUpdate(a.id, { fpsCap: 0 });
          cleared.set(saved.id, saved);
        }
        for (const g of clearingGroups) {
          const saved = await api.groupSave({ id: g.id, name: g.name, color: g.color ?? undefined, defaults: { ...g.defaults, fpsCap: null } });
          clearedGroups.set(saved.id, saved);
        }
        // 0 is Off.
        if (previewFps != null) setPreviewFps(previewFps as PreviewFps);
      }
      await Promise.allSettled([...makeTiny.map((i) => api.instanceMicro(i.id, true)), ...hide.map((i) => api.instanceShow(i.id, "minimize"))]);
      toast(t(running.length && nextLaunch ? "performance.appliedRelaunch" : "performance.applied"));
      onClose();
    } catch (e) {
      setError(friendly(asError(e)));
    } finally {
      // One cache write for all of them, the ones saved before a failure too: each redraws every account list.
      if (cleared.size) setAccounts(qc, (all) => all.map((x) => cleared.get(x.id) ?? x));
      if (clearedGroups.size) qc.setQueryData<Group[]>(qk.groups, (all) => all?.map((x) => clearedGroups.get(x.id) ?? x));
      setBusy(false);
    }
  }

  return (
    <Dialog open={profile !== null} onClose={onClose} title={t("performance.reviewTitle")} width={500}>
      <div className="dlg-body perf-review-body">
        <div ref={list} className="flex flex-col gap-4">
          {base.accounts.length > 0 && (
            <Segmented
              label={t("performance.applyTo")}
              value={scope}
              options={[{ value: "all", label: t("performance.scopeAll") }, { value: "chosen", label: t("performance.scopeChosen"), disabled: !named(own) }]}
              onChange={setScope}
              className="self-center"
            />
          )}
          {chosen && <AccountPicker variant="list" accounts={base.accounts} picked={picked} onPicked={setPicked} label={t("performance.scopeChosen")} />}
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
      </div>
      <div className="dlg-foot">
        <Button size="md" onClick={onClose} data-autofocus={!changes.length || undefined}>
          {t(changes.length ? "common.cancel" : "common.close")}
        </Button>
        {changes.length > 0 && (
          // aria-disabled: disabled would drop the focus, and leave it nowhere after an error.
          <Button size="md" variant="primary" busy={busy} aria-disabled={busy || !ready || undefined} onClick={() => void apply()} data-autofocus>
            {t("performance.apply")}
          </Button>
        )}
      </div>
    </Dialog>
  );
}
