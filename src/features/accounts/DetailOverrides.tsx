import { useState } from "react";
import type { AccountPatch, AccountSummary } from "../../api";
import { requestNavigation } from "../../app/navigation";
import type { ClientPrefs } from "../../bindings/ClientPrefs";
import { useGroups, useSettings, useUpdateAccount } from "../../data";
import { fmtList, t } from "../../i18n";
import { Disclosure } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { OffOrNumberField, Switch, TextField } from "../../ui/controls";
import { RANGES } from "../../ui/ranges";
import { FlagsDialog } from "../performance/FlagsDialog";
import { presetOf } from "../performance/presets";
import { Row, Section } from "./DetailOverview";
import { fromGroups } from "./GroupDefaultsDialog";

/** What the account's next client starts with, in words: "CPU limit 25 %, Efficiency mode, Anti-AFK off". */
function keptWords({ limits: l, antiAfk }: ClientPrefs) {
  return fmtList(
    [
      l.cpuCap !== null && `${t("launch.limits.cpu")} ${t("settings.percent", { n: l.cpuCap })}`,
      l.eco && t("launch.limits.eco"),
      !!l.trimEveryMin && `${t("launch.limits.trimEvery")} ${t("settings.minutesShort", { n: l.trimEveryMin })}`,
      l.affinity !== null && t("launch.limits.cores"),
      !antiAfk && t("accounts.detail.antiAfkOff"),
    ].filter((p): p is string => !!p),
  );
}

/** What this account's clients do differently from Settings and Performance. A refused change is
 * toasted and put back (useUpdateAccount). */
export function DetailOverrides({ account: a }: { account: AccountSummary }) {
  const update = useUpdateAccount();
  // The flags dialog shows a refusal itself.
  const quietUpdate = useUpdateAccount({ quiet: true });
  const settings = useSettings().data;
  const save = (patch: AccountPatch) => update.mutate({ id: a.id, patch });
  // Nothing brings a client back while both are off, whatever this switch says.
  const nothingBrings = !!settings && !settings.autoRejoin && !settings.autoRestartOnCrash;
  const [editing, setEditing] = useState(false);
  const count = Object.keys(a.customFlags).length;
  // What its groups give it where nothing is set here, in words.
  const g = fromGroups(a, useGroups().data ?? []);
  const given = [
    g.fullQuality && t("accounts.detail.fullQuality"),
    g.allowDisconnect && `${t("accounts.detail.bringBack")}: ${t("common.off")}`,
    g.fpsCap !== null && `${t("accounts.detail.fpsCap")} ${g.fpsCap}`,
    g.launchArgs !== null && `${t("accounts.detail.launchArgs")} ${g.launchArgs}`,
    g.discordMuted && t("accounts.detail.muteDiscord"),
    g.client !== null && keptWords(g.client),
  ].filter((p): p is string => !!p);

  return (
    <>
      {given.length > 0 && (
        <Section>
          <p className="text-[13px] text-fg-2">{t("accounts.detail.fromGroups", { list: fmtList(given) })}</p>
        </Section>
      )}
      <Section>
        <Row label={t("accounts.detail.fullQuality")}>
          <Switch label={t("accounts.detail.fullQuality")} checked={a.fullQuality} onChange={(fullQuality) => save({ fullQuality })} />
        </Row>
        <Disclosure summary={t("settings.moreAbout", { label: t("accounts.detail.fullQuality") })}>{t("accounts.detail.fullQualityHint")}</Disclosure>
      </Section>
      <Section>
        <Row label={t("accounts.detail.bringBack")} hint={!nothingBrings && t("accounts.detail.bringBackHint")}>
          <Switch label={t("accounts.detail.bringBack")} checked={!a.allowDisconnect} onChange={(back) => save({ allowDisconnect: !back })} />
        </Row>
        {nothingBrings && (
          <div className="flex flex-col items-start gap-2">
            <p className="alert text-[13px]">{t("accounts.detail.bringBackOff")}</p>
            <Button size="sm" variant="ghost" icon="settings" onClick={() => requestNavigation("settings", "launching")}>
              {t("app.nav.settings")}
            </Button>
          </div>
        )}
      </Section>
      <Section>
        <Row label={t("accounts.detail.fpsCap")} hint={t("accounts.detail.fpsCapShort")}>
          <OffOrNumberField
            label={t("accounts.detail.fpsCap")}
            value={a.fpsCap ?? 0}
            range={RANGES.fpsCap}
            unit={t("accounts.detail.fpsUnit")}
            short="FPS"
            onCommit={(fpsCap) => save({ fpsCap })}
          />
        </Row>
        <Disclosure summary={t("settings.moreAbout", { label: t("accounts.detail.fpsCap") })}>{t("accounts.detail.fpsCapHint")}</Disclosure>
      </Section>
      <Section>
        <span aria-hidden="true" className="font-medium -mb-1.5">{t("accounts.detail.launchArgs")}</span>
        <TextField
          label={t("accounts.detail.launchArgs")}
          placeholder={t("accounts.detail.launchArgsDefault")}
          value={a.launchArgs ?? ""}
          onCommit={(launchArgs) => update.mutateAsync({ id: a.id, patch: { launchArgs } })}
        />
      </Section>
      {settings?.customFlagsScope === "account" && (
        <Section>
          <Row label={t("flags.title")} hint={t("flags.accountHint")}>
            <Button size="md" icon="pencil" onClick={() => setEditing(true)}>
              {count ? t("flags.count", { count }) : t("flags.edit")}
            </Button>
          </Row>
          <FlagsDialog
            open={editing}
            onClose={() => setEditing(false)}
            title={t("flags.accountTitle", { name: a.alias || a.displayName })}
            flags={a.customFlags}
            onSave={(customFlags) => quietUpdate.mutateAsync({ id: a.id, patch: { customFlags } })}
          />
        </Section>
      )}
      {/* Given on the Performance page ("Chosen accounts"): here it is only named and removed. */}
      {a.profile && (
        <Section>
          <Row label={t("accounts.detail.profile")} hint={t("accounts.detail.profileHint")}>
            <Button size="md" onClick={() => save({ profile: {} })}>{t("common.remove")}</Button>
          </Row>
          <p className="text-[13px] text-fg-2">{t(presetOf(a.profile)?.label ?? "performance.custom")}</p>
        </Section>
      )}
      {/* Only once a client of its was given something: there is nothing to set here, only to forget. */}
      {a.clientLimits && (
        <Section>
          <Row label={t("accounts.detail.clientLimits")} hint={t("accounts.detail.clientLimitsHint")}>
            <Button size="md" onClick={() => save({ clearClientLimits: true })}>{t("accounts.detail.clientLimitsClear")}</Button>
          </Row>
          {/* Nothing to list: it keeps "no limits", over what its group gives. */}
          <p className="text-[13px] text-fg-2">{keptWords(a.clientLimits) || t("common.default")}</p>
        </Section>
      )}
      <Section>
        <Row label={t("accounts.detail.muteDiscord")} hint={t("accounts.detail.muteDiscordShort")}>
          <Switch label={t("accounts.detail.muteDiscord")} checked={a.discordMuted} onChange={(discordMuted) => save({ discordMuted })} />
        </Row>
        <Disclosure summary={t("settings.moreAbout", { label: t("accounts.detail.muteDiscord") })}>{t("accounts.detail.muteDiscordHint")}</Disclosure>
      </Section>
    </>
  );
}
