import { useState } from "react";
import type { AccountPatch, AccountSummary } from "../../api";
import { requestNavigation } from "../../app/navigation";
import { useSettings, useUpdateAccount } from "../../data";
import { t } from "../../i18n";
import { Disclosure } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { OffOrNumberField, Switch, TextField } from "../../ui/controls";
import { RANGES } from "../../ui/ranges";
import { FlagsDialog } from "../performance/FlagsDialog";
import { Row, Section } from "./DetailOverview";

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

  return (
    <>
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
      <Section>
        <Row label={t("accounts.detail.muteDiscord")} hint={t("accounts.detail.muteDiscordShort")}>
          <Switch label={t("accounts.detail.muteDiscord")} checked={a.discordMuted} onChange={(discordMuted) => save({ discordMuted })} />
        </Row>
        <Disclosure summary={t("settings.moreAbout", { label: t("accounts.detail.muteDiscord") })}>{t("accounts.detail.muteDiscordHint")}</Disclosure>
      </Section>
    </>
  );
}
