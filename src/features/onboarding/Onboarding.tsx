import { useEffect, useState } from "react";
import { api, type AccountSummary, type Settings } from "../../api";
import { qk, useMultiStatus, useSetSettings, useVault } from "../../data";
import { LANGUAGES, t } from "../../i18n";
import { useQueryClient } from "@tanstack/react-query";
import { Button } from "../../ui/Button";
import { Segmented } from "../../ui/controls";
import { Dialog, DialogSteps } from "../../ui/Dialog";
import { Select } from "../../ui/Field";
import { Icon, type IconName } from "../../ui/Icon";
import { toastError } from "../../ui/Toast";
import { ACCENTS } from "../settings/rows/appearance";
import { VaultFlowDialog } from "../vault/VaultFlowDialog";
import { QA } from "./qa";
import "./onboarding.css";

const STEPS = ["welcome", "look", "safe", "use", "account", "done"] as const;
type Step = (typeof STEPS)[number];

/** The first-run setup: each step writes what's picked at once (skipping later keeps it), then a
 * short tour of the real screen. `onFinish(tour)`: finished or skipped; `tour` when it's wanted. */
export function Onboarding({ open, settings, accounts, onAdd, onFinish }: {
  open: boolean;
  settings: Settings;
  accounts: AccountSummary[];
  onAdd: (tab: "login" | "many") => void;
  onFinish: (tour: boolean) => void;
}) {
  const [at, setAt] = useState(0);
  const [dir, setDir] = useState<1 | -1>(1);
  const [wasOpen, setWasOpen] = useState(open);
  // Each opening starts at the beginning.
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) setAt(0);
  }
  const step: Step = STEPS[at];
  const move = (by: 1 | -1) => {
    setDir(by);
    setAt((i) => Math.min(STEPS.length - 1, Math.max(0, i + by)));
  };
  // The first account just came in: on to the end.
  const count = accounts.length;
  const [before, setBefore] = useState(count);
  if (count !== before) {
    setBefore(count);
    if (open && step === "account" && count > before) move(1);
  }

  return (
    <Dialog open={open} onClose={() => onFinish(false)} title={t("onboarding.title")} width={560}>
      <DialogSteps step={step} direction={dir}>
        <div className="dlg-body onb-body">
          {step === "welcome" && <Welcome />}
          {step === "look" && <Look settings={settings} />}
          {step === "safe" && <Safe />}
          {step === "use" && <Use settings={settings} />}
          {step === "account" && <Account count={count} onAdd={onAdd} />}
          {step === "done" && <Done />}
        </div>
      </DialogSteps>
      <div className="dlg-foot items-center">
        <span className="me-auto text-[13px] text-fg-3">{t("onboarding.stepOf", { n: at + 1, total: STEPS.length })}</span>
        {step === "done" ? (
          <>
            <Button size="md" onClick={() => onFinish(false)}>{t("onboarding.finish")}</Button>
            <Button size="md" variant="primary" data-autofocus onClick={() => onFinish(true)}>{t("onboarding.tour")}</Button>
          </>
        ) : (
          <>
            {at === 0 ? (
              <Button size="md" variant="ghost" onClick={() => onFinish(false)}>{t("onboarding.skip")}</Button>
            ) : (
              <Button size="md" variant="ghost" onClick={() => move(-1)}>{t("onboarding.back")}</Button>
            )}
            <Button size="md" variant="primary" data-autofocus onClick={() => move(1)}>
              {at === 0 ? t("onboarding.start") : t("onboarding.next")}
            </Button>
          </>
        )}
      </div>
    </Dialog>
  );
}

function Head({ icon, title, text }: { icon: IconName; title: string; text: string }) {
  return (
    <div className="onb-head">
      <span className="onb-icon" aria-hidden="true"><Icon name={icon} size={20} /></span>
      <h3 className="onb-title">{title}</h3>
      <p className="text-fg-2">{text}</p>
    </div>
  );
}

function Welcome() {
  return (
    <>
      <Head icon="accounts" title={t("onboarding.welcome.title")} text={t("onboarding.welcome.text")} />
      {QA && <p className="onb-qa">{t("onboarding.qa")}</p>}
    </>
  );
}

function Look({ settings }: { settings: Settings }) {
  const set = useSetSettings();
  const theme = settings.theme === "custom" ? "system" : settings.theme;
  return (
    <>
      <Head icon="palette" title={t("onboarding.look.title")} text={t("onboarding.look.text")} />
      <div className="onb-fields">
        <label className="onb-field">
          <span>{t("settings.language")}</span>
          <Select
            value={settings.language}
            onChange={(language) => set.mutate({ language })}
            options={[{ value: "", label: t("settings.languageWindows") }, ...LANGUAGES.map(([value, text]) => ({ value: value as string, label: text, lang: value }))]}
          />
        </label>
        <div className="onb-field">
          <span>{t("settings.theme")}</span>
          <Segmented
            label={t("settings.theme")}
            className="self-start"
            value={theme}
            onChange={(theme) => set.mutate({ theme })}
            options={[
              { value: "system" as const, label: t("settings.themeSystem") },
              { value: "dark" as const, label: t("settings.themeDark") },
              { value: "light" as const, label: t("settings.themeLight") },
            ]}
          />
        </div>
        <div className="onb-field">
          <span>{t("settings.accentColour")}</span>
          <div role="group" aria-label={t("settings.accentColour")} className="set-swatches self-start">
            <button type="button" className="swatch set-mono" aria-pressed={!settings.accent} aria-label={t("settings.accent.mono")} data-tip={t("settings.accent.mono")} onClick={() => set.mutate({ accent: "" })}>
              <span aria-hidden="true" className="set-mono-half" />
            </button>
            {ACCENTS.map(([v, name]) => (
              <button key={v} type="button" className="swatch" style={{ background: v }} aria-pressed={settings.accent === v} aria-label={t(name)} data-tip={t(name)} onClick={() => set.mutate({ accent: v })} />
            ))}
          </div>
        </div>
      </div>
    </>
  );
}

/** A card to pick: a title, its words, and whether it's the one on. */
function Card({ on, title, text, tag, onPick }: { on: boolean; title: string; text: string; tag?: string; onPick: () => void }) {
  return (
    <button type="button" className="onb-card" aria-pressed={on} onClick={onPick}>
      <span className="onb-check" aria-hidden="true">{on && <Icon name="check" size={13} />}</span>
      <span className="flex flex-col gap-1 min-w-0">
        <span className="onb-card-title">
          {title}
          {tag && <span className="tag">{tag}</span>}
        </span>
        <span className="text-[13px] text-fg-2">{text}</span>
      </span>
    </button>
  );
}

function Safe() {
  const mode = useVault().data?.mode ?? "dpapi";
  const [setting, setSetting] = useState(false);
  return (
    <>
      <Head icon="lock" title={t("onboarding.safe.title")} text={t("onboarding.safe.text")} />
      <div className="onb-cards">
        <Card on={mode === "dpapi"} title={t("onboarding.safe.windows")} text={t("onboarding.safe.windowsText")} tag={t("common.default")} onPick={() => {}} />
        <Card on={mode === "master"} title={t("onboarding.safe.master")} text={t("onboarding.safe.masterText")} onPick={() => mode !== "master" && setSetting(true)} />
      </div>
      {mode === "master" && <p className="text-[13px] text-fg-3">{t("onboarding.safe.change")}</p>}
      <VaultFlowDialog flow={setting ? "setMaster" : null} onClose={() => setSetting(false)} />
    </>
  );
}

function Use({ settings }: { settings: Settings }) {
  const set = useSetSettings();
  const qc = useQueryClient();
  const multi = useMultiStatus().data?.enabled ?? true;
  const online = settings.autoRejoin && settings.autoRestartOnCrash && settings.antiAfk.enabled;
  const toggleMulti = () =>
    api.multiInstanceSet(!multi).then((s) => qc.setQueryData(qk.multi, s), toastError);
  const toggleOnline = () => {
    const on = !online;
    set.mutate({ autoRejoin: on, autoRestartOnCrash: on, antiAfk: { ...settings.antiAfk, enabled: on }, closeToTray: on });
  };
  return (
    <>
      <Head icon="play" title={t("onboarding.use.title")} text={t("onboarding.use.text")} />
      <div className="onb-cards">
        <Card on={multi} title={t("onboarding.use.many")} text={t("onboarding.use.manyText")} onPick={toggleMulti} />
        <Card on={online} title={t("onboarding.use.online")} text={t("onboarding.use.onlineText")} onPick={toggleOnline} />
      </div>
      <p className="text-[13px] text-fg-3">{t("onboarding.use.more")}</p>
    </>
  );
}

function Account({ count, onAdd }: { count: number; onAdd: (tab: "login" | "many") => void }) {
  return (
    <>
      <Head icon="accounts" title={t("onboarding.account.title")} text={count ? t("onboarding.account.have", { count }) : t("onboarding.account.text")} />
      <div className="flex flex-wrap gap-2">
        <Button size="md" variant={count ? "secondary" : "primary"} icon="plus" onClick={() => onAdd("login")}>{t("onboarding.account.signIn")}</Button>
        <Button size="md" variant="ghost" onClick={() => onAdd("many")}>{t("onboarding.account.import")}</Button>
      </div>
    </>
  );
}

function Done() {
  return <Head icon="check" title={t("onboarding.done.title")} text={t("onboarding.done.text")} />;
}

/** When the setup opens: once the splash is gone and the accounts are known; `start` says whether. */
export function useOnboardingOpen(ready: boolean, start: () => boolean) {
  const [open, setOpen] = useState(false);
  const [checked, setChecked] = useState(false);
  useEffect(() => {
    if (!ready || checked) return;
    setChecked(true);
    if (start()) setOpen(true);
  }, [ready, checked, start]);
  return [open, setOpen] as const;
}
