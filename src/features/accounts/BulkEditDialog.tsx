import { useState } from "react";
import { asError, type AccountPatch, type AccountSummary, type Group } from "../../api";
import { useUpdateAccount } from "../../data";
import { t } from "../../i18n";
import { Button } from "../../ui/Button";
import { NumberField } from "../../ui/controls";
import { Dialog } from "../../ui/Dialog";
import { Input, Select } from "../../ui/Field";
import { Collapse } from "../../ui/motion";
import { RANGES } from "../../ui/ranges";
import { friendly, toast } from "../../ui/Toast";
import { useOpenCount } from "../vault/useOpenCount";

/** "": leave each account as it is. */
type Tri = "" | "on" | "off";
type Own = "" | "default" | "set";

/** The overrides of several accounts at once: each field starts at "Leave as is", so only what was
 * picked is changed. One account after another; the first refusal stops it and says which. */
export function BulkEditDialog({ accounts, groups, onClose }: { accounts: AccountSummary[] | null; groups: Group[]; onClose: () => void }) {
  const opened = useOpenCount(!!accounts);
  // While it applies it stays open: a refusal part-way has to be seen.
  const [busy, setBusy] = useState(false);
  return (
    <Dialog open={!!accounts} onClose={() => !busy && onClose()} title={t("accounts.bulk.title")} width={460}>
      {accounts && <Form key={opened} accounts={accounts} groups={groups} busy={busy} setBusy={setBusy} onClose={onClose} />}
    </Dialog>
  );
}

function Form({ accounts, groups, busy, setBusy, onClose }: { accounts: AccountSummary[]; groups: Group[]; busy: boolean; setBusy: (b: boolean) => void; onClose: () => void }) {
  const update = useUpdateAccount({ quiet: true });
  const [fullQuality, setFullQuality] = useState<Tri>("");
  const [bringBack, setBringBack] = useState<Tri>("");
  const [mute, setMute] = useState<Tri>("");
  const [fps, setFps] = useState<Own>("");
  const [fpsCap, setFpsCap] = useState<number>(RANGES.fpsCap.on);
  const [args, setArgs] = useState<Own>("");
  const [launchArgs, setLaunchArgs] = useState("");
  const [leave, setLeave] = useState("");
  const [error, setError] = useState<string | null>(null);

  const gid = groups.some((g) => String(g.id) === leave) ? Number(leave) : null;
  const patchFor = (a: AccountSummary): AccountPatch => ({
    ...(fullQuality && { fullQuality: fullQuality === "on" }),
    ...(bringBack && { allowDisconnect: bringBack === "off" }),
    ...(mute && { discordMuted: mute === "on" }),
    // 0 and "" are the backend's "back to the default".
    ...(fps && { fpsCap: fps === "set" ? fpsCap : 0 }),
    ...(args && { launchArgs: args === "set" ? launchArgs : "" }),
    ...(gid !== null && a.groupIds.includes(gid) && { groupIds: a.groupIds.filter((g) => g !== gid) }),
  });
  const nothing = !fullQuality && !bringBack && !mute && !fps && !args && gid === null;

  async function apply() {
    if (busy || nothing) return;
    setBusy(true);
    setError(null);
    try {
      for (const a of accounts) {
        const patch = patchFor(a);
        if (Object.keys(patch).length) await update.mutateAsync({ id: a.id, patch }).catch((e) => Promise.reject(new Error(`@${a.username}: ${friendly(asError(e))}`)));
      }
      toast(t("accounts.bulk.done"));
      onClose();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  const tri = (label: string, value: Tri, set: (v: Tri) => void) => (
    <label className="flex items-center justify-between gap-3">
      <span>{label}</span>
      <Select
        aria-label={label}
        value={value}
        onChange={(v) => set(v as Tri)}
        options={[{ value: "", label: t("accounts.bulk.leave") }, { value: "on", label: t("common.on") }, { value: "off", label: t("common.off") }]}
      />
    </label>
  );
  const own = (label: string, value: Own, set: (v: Own) => void) => (
    <Select
      aria-label={label}
      value={value}
      onChange={(v) => set(v as Own)}
      options={[{ value: "", label: t("accounts.bulk.leave") }, { value: "default", label: t("common.default") }, { value: "set", label: t("accounts.bulk.set") }]}
    />
  );

  return (
    <>
      <div className="dlg-body flex flex-col gap-3.5">
        {tri(t("accounts.detail.fullQuality"), fullQuality, setFullQuality)}
        {tri(t("accounts.detail.bringBack"), bringBack, setBringBack)}
        {tri(t("accounts.detail.muteDiscord"), mute, setMute)}
        <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
          <span>{t("accounts.detail.fpsCap")}</span>
          <span className="flex items-center gap-2">
            {own(t("accounts.detail.fpsCap"), fps, setFps)}
            {fps === "set" && <NumberField unit={t("accounts.detail.fpsUnit")} short="FPS" min={RANGES.fpsCap.min} max={RANGES.fpsCap.max} value={fpsCap} onCommit={setFpsCap} />}
          </span>
        </div>
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between gap-3">
            <span>{t("accounts.detail.launchArgs")}</span>
            {own(t("accounts.detail.launchArgs"), args, setArgs)}
          </div>
          {args === "set" && <Input aria-label={t("accounts.detail.launchArgs")} autoComplete="off" spellCheck={false} value={launchArgs} onChange={(e) => setLaunchArgs(e.target.value)} />}
        </div>
        {groups.length > 0 && (
          <label className="flex items-center justify-between gap-3">
            <span>{t("accounts.bulk.removeFromGroup")}</span>
            <Select
              aria-label={t("accounts.bulk.removeFromGroup")}
              value={gid === null ? "" : String(gid)}
              onChange={setLeave}
              options={[{ value: "", label: t("accounts.bulk.leave") }, ...groups.map((g) => ({ value: String(g.id), label: g.name }))]}
            />
          </label>
        )}
        <Collapse open={!!error}>
          <p role="alert" className="alert">{error}</p>
        </Collapse>
      </div>
      <div className="dlg-foot">
        <Button onClick={onClose} disabled={busy}>{t("common.cancel")}</Button>
        <Button variant="primary" busy={busy} aria-disabled={nothing || undefined} onClick={() => void apply()}>
          {t("performance.apply")}
        </Button>
      </div>
    </>
  );
}
