import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../../../api";
import { qk } from "../../../data";
import { t } from "../../../i18n";
import { Button } from "../../../ui/Button";
import { Switch } from "../../../ui/controls";
import { DonateDialog } from "../../donate/DonateDialog";
import { DISCORD_INVITE } from "../../donate/links";
import { ActionButton } from "../kit/ActionButton";
import { useRow } from "../kit/rowError";
import { SettingRow } from "../kit/SettingRow";
import { unhideUpdate } from "../UpdateBanner";

const RELEASES = "https://github.com/inspectsoftware/RoALT/releases";

/** Check now (and what it found) beside the switch for the background checks. A found update shows
 * in the island again, even one Later hid. */
function SelfUpdate() {
  const row = useRow();
  const qc = useQueryClient();
  const [busy, setBusy] = useState(false);
  async function check() {
    setBusy(true);
    row.setError(null);
    row.setResult(null);
    try {
      const u = await api.selfUpdateCheck();
      unhideUpdate();
      qc.setQueryData(qk.appUpdate, u);
      row.setResult(u.available ? t("settings.updateAvailable", { version: u.available }) : t("settings.upToDate"));
    } catch (e) {
      row.fail(e);
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <ActionButton busy={busy} label={t("settings.checkNow")} busyLabel={t("settings.checking")} onRun={() => void check()} />
      <Switch label={t("settings.selfUpdate")} checked={row.ctx.s.selfUpdate} onChange={(selfUpdate) => void row.write({ selfUpdate })} describedBy={row.describedBy} />
    </>
  );
}

export function SelfUpdateRow() {
  return (
    <SettingRow id="selfUpdate">
      <SelfUpdate />
    </SettingRow>
  );
}

function Releases() {
  const row = useRow();
  return (
    <Button size="md" iconEnd="external" aria-describedby={row.describedBy} onClick={() => openUrl(RELEASES).catch(row.fail)}>
      {t("settings.openReleases")}
    </Button>
  );
}

/** RoALT's own version: the one place it shows. */
export function VersionRow() {
  const version = import.meta.env.APP_VERSION;
  return (
    <SettingRow id="version" name={`RoALT ${version}`} title={<span className="set-label">RoALT <span className="mono">{version}</span></span>}>
      <Releases />
    </SettingRow>
  );
}

function Support() {
  const row = useRow();
  const [donating, setDonating] = useState(false);
  return (
    <>
      <Button size="md" iconEnd="external" onClick={() => openUrl(DISCORD_INVITE).catch(row.fail)}>{t("app.joinDiscord")}</Button>
      <Button size="md" icon="heart" onClick={() => setDonating(true)}>{t("donate.button")}</Button>
      <DonateDialog open={donating} onClose={() => setDonating(false)} />
    </>
  );
}

export function SupportRow() {
  return (
    <SettingRow id="support" title={<span className="set-label">{t("donate.title")}</span>}>
      <Support />
    </SettingRow>
  );
}
