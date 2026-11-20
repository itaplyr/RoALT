import { useState } from "react";
import { api, type AccountSummary, type Group } from "../../api";
import type { ClientPrefs } from "../../bindings/ClientPrefs";
import type { GroupDefaults } from "../../bindings/GroupDefaults";
import { qk, queryClient, useSaveGroup, withGroupDefaults } from "../../data";
import { t } from "../../i18n";
import { Button } from "../../ui/Button";
import { NumberField, Segmented, Switch } from "../../ui/controls";
import { Dialog } from "../../ui/Dialog";
import { Input, Select } from "../../ui/Field";
import { RANGES } from "../../ui/ranges";
import { toast } from "../../ui/Toast";
import { CAPS, TRIMS } from "../dashboard/LimitsDialog";
import { useOpenCount } from "../vault/useOpenCount";

/** "": the group says nothing, so each account's own setting (or the next group's) counts. */
type Tri = "" | "on" | "off";
const tri = (v: boolean | null): Tri => (v === null ? "" : v ? "on" : "off");
const bool = (v: Tri) => (v === "" ? null : v === "on");
const NO_LIMITS: ClientPrefs = { limits: { cpuCap: null, eco: false, trimEveryMin: null, affinity: null }, antiAfk: true };

/** What `a` gets from its groups where it sets nothing itself (`withGroupDefaults`). Null / false:
 * nothing from a group. */
export function fromGroups(a: AccountSummary, groups: Group[]) {
  const run = withGroupDefaults(a, groups);
  return {
    fullQuality: run.fullQuality && !a.fullQuality,
    discordMuted: run.discordMuted && !a.discordMuted,
    allowDisconnect: run.allowDisconnect && !a.allowDisconnect,
    fpsCap: a.fpsCap === null ? run.fpsCap : null,
    launchArgs: a.launchArgs === null ? run.launchArgs : null,
    client: a.clientLimits === null ? run.clientLimits : null,
  };
}

/** A group's defaults: what its accounts use where they set nothing of their own. Every field
 * starts at "Each account's own", which is the group saying nothing. */
export function GroupDefaultsDialog({ group, onClose }: { group: Group | null; onClose: () => void }) {
  const opened = useOpenCount(!!group);
  return (
    <Dialog open={!!group} onClose={onClose} title={group ? t("groups.defaults.title", { name: group.name }) : ""} width={480}>
      {group && <Form key={opened} group={group} onClose={onClose} />}
    </Dialog>
  );
}

function Form({ group, onClose }: { group: Group; onClose: () => void }) {
  // A refusal is toasted (the mutation cache).
  const save = useSaveGroup();
  const d = group.defaults;
  const [fullQuality, setFullQuality] = useState<Tri>(tri(d.fullQuality));
  // "Bring it back" is the other way round from `allowDisconnect`.
  const [bringBack, setBringBack] = useState<Tri>(tri(d.allowDisconnect === null ? null : !d.allowDisconnect));
  const [mute, setMute] = useState<Tri>(tri(d.discordMuted));
  const [fpsOn, setFpsOn] = useState(d.fpsCap !== null);
  const [fpsCap, setFpsCap] = useState<number>(d.fpsCap ?? RANGES.fpsCap.on);
  const [argsOn, setArgsOn] = useState(d.launchArgs !== null);
  const [launchArgs, setLaunchArgs] = useState(d.launchArgs ?? "");
  const [clientOn, setClientOn] = useState(d.client !== null);
  // The cores a group was given elsewhere (a backup) are kept: this dialog doesn't show them.
  const [client, setClient] = useState<ClientPrefs>(d.client ?? NO_LIMITS);
  const [clearOverrides, setClearOverrides] = useState(false);
  const limit = (patch: Partial<ClientPrefs["limits"]>) => setClient((c) => ({ ...c, limits: { ...c.limits, ...patch } }));

  const submit = () => {
    if (save.isPending) return;
    const defaults: GroupDefaults = {
      fullQuality: bool(fullQuality),
      allowDisconnect: bringBack === "" ? null : bringBack === "off",
      discordMuted: bool(mute),
      fpsCap: fpsOn ? fpsCap : null,
      launchArgs: argsOn && launchArgs.trim() ? launchArgs.trim() : null,
      client: clientOn ? client : null,
    };
    save.mutate(
      { id: group.id, name: group.name, color: group.color ?? undefined, defaults },
      {
        onSuccess: async () => {
          if (clearOverrides) {
            try {
              await api.groupClearOverrides(group.id);
              void queryClient.invalidateQueries({ queryKey: qk.accounts });
            } catch {
              // group defaults were saved; keep going
            }
          }
          toast(t("groups.defaults.saved"));
          onClose();
        },
      },
    );
  };

  const own = { value: "", label: t("groups.defaults.own") };
  const switchRow = (label: string, value: Tri, set: (v: Tri) => void) => (
    <label className="flex items-center justify-between gap-3">
      <span>{label}</span>
      <Select aria-label={label} value={value} onChange={(v) => set(v as Tri)} options={[own, { value: "on", label: t("common.on") }, { value: "off", label: t("common.off") }]} />
    </label>
  );
  const setRow = (label: string, on: boolean, set: (on: boolean) => void, none = own.label) => (
    <Select aria-label={label} value={on ? "set" : ""} onChange={(v) => set(v === "set")} options={[{ value: "", label: none }, { value: "set", label: t("accounts.bulk.set") }]} />
  );

  return (
    <>
      <div className="dlg-body flex flex-col gap-3.5">
        <p className="text-[13px] text-fg-2">{t("groups.defaults.lead")}</p>
        {switchRow(t("accounts.detail.fullQuality"), fullQuality, setFullQuality)}
        {switchRow(t("accounts.detail.bringBack"), bringBack, setBringBack)}
        {switchRow(t("accounts.detail.muteDiscord"), mute, setMute)}
        <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
          <span>{t("accounts.detail.fpsCap")}</span>
          <span className="flex items-center gap-2">
            {setRow(t("accounts.detail.fpsCap"), fpsOn, setFpsOn)}
            {fpsOn && <NumberField unit={t("accounts.detail.fpsUnit")} short="FPS" min={RANGES.fpsCap.min} max={RANGES.fpsCap.max} value={fpsCap} onCommit={setFpsCap} />}
          </span>
        </div>
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between gap-3">
            <span>{t("accounts.detail.launchArgs")}</span>
            {setRow(t("accounts.detail.launchArgs"), argsOn, setArgsOn)}
          </div>
          {argsOn && <Input aria-label={t("accounts.detail.launchArgs")} autoComplete="off" spellCheck={false} value={launchArgs} onChange={(e) => setLaunchArgs(e.target.value)} />}
        </div>
        <div className="flex flex-col gap-3">
          <div className="flex items-center justify-between gap-3">
            <span>{t("groups.defaults.client")}</span>
            {setRow(t("groups.defaults.client"), clientOn, setClientOn, t("groups.defaults.clientNone"))}
          </div>
          {clientOn && (
            <div className="well flex flex-col gap-3 px-4 py-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span>{t("launch.limits.cpu")}</span>
                <Segmented
                  mono
                  label={t("launch.limits.cpu")}
                  value={client.limits.cpuCap ?? 0}
                  options={CAPS.map((c) => ({ value: c, label: c ? t("settings.percent", { n: c }) : t("common.off") }))}
                  onChange={(c) => limit({ cpuCap: c || null })}
                />
              </div>
              <div className="flex items-center justify-between gap-2">
                <span>{t("launch.limits.eco")}</span>
                <Switch label={t("launch.limits.eco")} checked={client.limits.eco} onChange={(eco) => limit({ eco })} />
              </div>
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span>{t("launch.limits.trim")}</span>
                <Segmented
                  mono
                  label={t("launch.limits.trimEvery")}
                  value={client.limits.trimEveryMin ?? -1}
                  options={TRIMS.map((m) => ({ value: m, label: m < 0 ? t("common.default") : m ? t("settings.minutesShort", { n: m }) : t("common.off") }))}
                  onChange={(m) => limit({ trimEveryMin: m < 0 ? null : m })}
                />
              </div>
              <div className="flex items-center justify-between gap-2">
                <span>{t("launch.limits.antiAfk")}</span>
                <Switch label={t("launch.limits.antiAfk")} checked={client.antiAfk} onChange={(antiAfk) => setClient((c) => ({ ...c, antiAfk }))} />
              </div>
            </div>
          )}
        </div>
        <label className="flex items-start gap-2.5 pt-1 text-[13px] text-fg-2">
          <input type="checkbox" className="cb mt-0.5" checked={clearOverrides} onChange={(e) => setClearOverrides(e.target.checked)} />
          <span>{t("groups.defaults.clearOverrides")}</span>
        </label>
      </div>
      <div className="dlg-foot">
        <Button onClick={onClose} disabled={save.isPending}>{t("common.cancel")}</Button>
        <Button variant="primary" busy={save.isPending} onClick={submit}>
          {t("common.save")}
        </Button>
      </div>
    </>
  );
}
