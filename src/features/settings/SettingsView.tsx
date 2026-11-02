import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openFile, save as saveFile } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, asError, events, type BandwidthStatus, type ConflictPolicy, type ImportReport, type InstallProgress, type RobloxClient, type Settings, type VaultStatus } from "../../api";
import { qk, useMultiStatus, useNetwork, useSetSettings } from "../../data";
import { NumberField, Segmented, Switch, TextField } from "../../ui/controls";
import { ConfirmDialog, Dialog } from "../../ui/Dialog";
import { fmtDate, fmtNum, LANGUAGES, t, tx, type Key } from "../../i18n";
import { Icon } from "../../ui/Icon";
import { friendly, toast, toastError } from "../../ui/Toast";
import { MultiNotice, useMultiToggle } from "../launch/Running";
import { PasswordDialog } from "../vault/PasswordDialog";
import { unhideUpdate } from "./UpdateBanner";

/** Accent presets: value ("" is the theme's own lime) and name. Any other colour comes from the picker. */
const ACCENTS: [string, Key][] = [
  ["", "settings.accent.lime"],
  ["#ff4fd8", "settings.accent.neon"],
  ["#f48fb8", "settings.accent.sakura"],
  ["#e2603f", "settings.accent.brick"],
  ["#ff9a3c", "settings.accent.orange"],
  ["#ffd23f", "settings.accent.gold"],
  ["#2ee6c5", "settings.accent.mint"],
  ["#4f8cff", "settings.accent.blue"],
  ["#a66bff", "settings.accent.violet"],
];
const pickerFilter = () => [{ name: t("settings.images"), extensions: ["png", "jpg", "jpeg", "webp", "gif"] }];

export const SETTINGS_SECTIONS: readonly (readonly [string, Key])[] = [
  ["general", "settings.section.general"],
  ["automation", "settings.section.automation"],
  ["security", "settings.section.security"],
  ["data", "settings.section.data"],
  ["about", "settings.section.about"],
  ["launching", "settings.section.launching"],
  ["shortcuts", "settings.section.shortcuts"],
];

const CLOSED_KEY = "roalt.settingsClosed";
/** The sections folded on this PC; all open on first run. When this PC keeps no storage, they last until RoALT closes. */
const closed = (() => {
  try {
    return new Set<string>(JSON.parse(localStorage.getItem(CLOSED_KEY) ?? "[]"));
  } catch {
    return new Set<string>();
  }
})();

const SHORTCUTS: [Key, string][] = [
  ["settings.shortcut.dashboard", "Ctrl 1"],
  ["settings.shortcut.accounts", "Ctrl 2"],
  ["settings.shortcut.launch", "Ctrl 3"],
  ["settings.shortcut.servers", "Ctrl 4"],
  ["settings.shortcut.settings", "Ctrl 5"],
  ["settings.shortcut.monitor", "Ctrl 6"],
  ["settings.shortcut.discord", "Ctrl 7"],
  ["settings.shortcut.debug", "Ctrl 8"],
  ["settings.shortcut.performance", "Ctrl 9"],
  ["settings.shortcut.add", "Ctrl N"],
  ["settings.shortcut.filter", "Ctrl F"],
  ["settings.shortcut.selectAll", "Ctrl A"],
  ["settings.shortcut.validate", "Ctrl R"],
  ["settings.shortcut.remove", "Delete"],
  ["settings.shortcut.launchNow", "Ctrl Enter"],
  ["settings.shortcut.refresh", "F5"],
  ["settings.shortcut.lock", "Ctrl L"],
  ["settings.shortcut.close", "Esc"],
];

const backupFilter = () => [{ name: t("settings.backupFilter"), extensions: ["roalt"] }];
const RELEASES = "https://github.com/inspectsoftware/RoALT/releases";
const cancelled = () => ({ kind: "cancelled", message: "" });

type VaultDialog = "set" | "change" | "remove" | "export";

export function SettingsView({ vault, settings, onPerformance, onRecoveryCode }: { vault: VaultStatus; settings: Settings; onPerformance: () => void; onRecoveryCode: (code: string) => void }) {
  const qc = useQueryClient();
  const set = useSetSettings();
  const master = vault.mode === "master";
  const [dlg, setDlg] = useState<VaultDialog | null>(null);
  // Apart from `dlg`: the offer opens as the "set" dialog closes.
  const [recoveryAsk, setRecoveryAsk] = useState<"offer" | "create" | null>(null);
  const [importPath, setImportPath] = useState("");
  const [importOpen, setImportOpen] = useState(false);
  const multi = useMultiStatus().data;
  const multiToggle = useMultiToggle((st) => st.message && toast(st.message, "error"));
  // Asked again each time Settings opens: Roblox may have been installed meanwhile.
  const client = useQuery({ queryKey: qk.client, queryFn: api.robloxClient, staleTime: 0 }).data;
  const dataDir = useQuery({ queryKey: qk.dataDir, queryFn: api.dataDirInfo }).data;
  const [movePath, setMovePath] = useState("");
  const [moveOpen, setMoveOpen] = useState(false);
  const [moving, setMoving] = useState(false);

  const setMode = async (mode: "dpapi" | "master", current?: string, next?: string, done?: string) => {
    await api.vaultSetMode(mode, current || undefined, next || undefined);
    qc.invalidateQueries({ queryKey: qk.vault });
    if (done) toast(done);
  };

  async function pickImport() {
    try {
      const path = await openFile({ multiple: false, directory: false, filters: backupFilter() });
      if (path) {
        setImportPath(path);
        setImportOpen(true);
      }
    } catch (e) {
      toastError(e);
    }
  }

  async function pickDataDir() {
    try {
      const path = await openFile({ directory: true, multiple: false, title: t("settings.moveDialog") });
      if (path) {
        setMovePath(path);
        setMoveOpen(true);
      }
    } catch (e) {
      toastError(e);
    }
  }

  async function moveData() {
    setMoving(true);
    try {
      await api.dataDirMove(movePath);
    } catch (e) {
      toastError(e);
      setMoving(false);
      setMoveOpen(false);
    }
  }

  return (
    <main className="scroll flex-1 min-w-0 px-8 pt-5 pb-12">
      <div className="enter max-w-[1400px] flex flex-col">
        <header className="flex flex-wrap items-start justify-between gap-3 mb-2">
          <h1 className="page-title">{t("settings.title")}</h1>
          <dl aria-label={t("settings.section.about")} className="panel grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-0.5 px-3.5 py-2 text-[13px] max-w-[280px]">
            <dt className="text-dim">RoALT</dt>
            <dd className="mono truncate">{import.meta.env.APP_VERSION}</dd>
            <dt className="text-dim">Roblox</dt>
            <dd className="mono truncate" title={client?.version ?? undefined}>{client?.version ?? t("settings.versionUnknown")}</dd>
          </dl>
        </header>

        <div className="grid items-start gap-8 mt-5" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(min(440px, 100%), 1fr))" }}>
        <div className="flex flex-col gap-8 min-w-0">
        <Section id="general" title={t("settings.section.general")}>
        <Row title={t("settings.theme")} hint={t("settings.themeHint")}>
          <Segmented
            label={t("settings.theme")}
            value={settings.theme}
            options={[{ value: "dark", label: t("settings.themeDark") }, { value: "light", label: t("settings.themeLight") }, { value: "system", label: t("settings.themeSystem") }]}
            onChange={(theme) => set.mutate({ theme })}
          />
        </Row>
        <Row title={t("settings.language")} hint={t("settings.languageHint")}>
          <select
            className="field w-auto"
            aria-label={t("settings.language")}
            value={settings.language}
            onChange={(e) => set.mutate({ language: e.target.value })}
          >
            <option value="">{t("settings.languageWindows")}</option>
            {LANGUAGES.map(([code, name]) => (
              <option key={code} value={code} lang={code}>{name}</option>
            ))}
          </select>
        </Row>
        <Row title={t("settings.accentColour")} hint={t("settings.accentHint")}>
          <AccentPicker value={settings.accent} onChange={(accent) => set.mutate({ accent })} />
        </Row>
        <BackgroundRow dim={settings.backgroundDim} onDim={(backgroundDim) => set.mutate({ backgroundDim })} />
        <Row title={t("settings.density")} hint={t("settings.densityHint")}>
          <Segmented
            label={t("settings.density")}
            value={settings.density}
            options={[{ value: "detailed", label: t("settings.densityDetailed") }, { value: "compact", label: t("settings.densityCompact") }]}
            onChange={(density) => set.mutate({ density })}
          />
        </Row>
        <Row title={t("settings.discord")} hint={t("settings.discordHint")}>
          <Switch label={t("settings.discord")} checked={settings.discordPresence} onChange={(discordPresence) => set.mutate({ discordPresence })} />
        </Row>
        <Row title={t("settings.closeToTray")} hint={t("settings.closeToTrayHint")}>
          <Switch label={t("settings.closeToTray")} checked={settings.closeToTray} onChange={(closeToTray) => set.mutate({ closeToTray })} />
        </Row>
        <Row title={t("settings.tabLayout")} hint={t("settings.tabLayoutHint")}>
          <Segmented
            label={t("settings.tabLayout")}
            value={settings.tabLayout}
            options={[{ value: "top", label: t("settings.tabLayoutTop") }, { value: "side", label: t("settings.tabLayoutSide") }]}
            onChange={(tabLayout) => set.mutate({ tabLayout })}
          />
        </Row>
        <Row title={t("settings.skipSplash")} hint={t("settings.skipSplashHint")}>
          <Switch label={t("settings.skipSplash")} checked={settings.skipSplash} onChange={(skipSplash) => set.mutate({ skipSplash })} />
        </Row>
        </Section>

        <Section id="automation" title={t("settings.section.automation")}>
        <Row
          title={t("settings.panic")}
          hint={t("settings.panicHint")}
        >
          <TextField
            label={t("settings.panic")}
            className="w-48"
            placeholder={t("settings.placeholderOff")}
            value={settings.panicHotkey}
            onCommit={(panicHotkey) => set.mutateAsync({ panicHotkey })}
          />
        </Row>
        <ShutdownRow />
        <Row
          title={t("settings.cleanLogs")}
          hint={t("settings.cleanLogsHint")}
        >
          <div className="flex items-center gap-3">
            <CleanLogsButton />
            <Switch label={t("settings.cleanLogs")} checked={settings.cleanLogs} onChange={(cleanLogs) => set.mutate({ cleanLogs })} />
          </div>
        </Row>
        </Section>

        <Section id="security" title={t("settings.section.security")}>
        <Row
          title={t("settings.encryption")}
          hint={
            master
              ? t("settings.encryptionHintMaster")
              : t("settings.encryptionHintDpapi")
          }
        >
          <Segmented
            label={t("settings.encryption")}
            value={vault.mode}
            options={[{ value: "dpapi", label: t("settings.encryptionDpapi") }, { value: "master", label: t("settings.masterPassword") }]}
            onChange={(m) => setDlg(m === "master" ? "set" : "remove")}
          />
        </Row>
        {master && (
          <>
            <Row title={t("settings.masterPassword")} hint={t("settings.masterPasswordHint")}>
              <button className="btn" onClick={() => setDlg("change")}>{t("settings.change")}</button>
            </Row>
            <Row title={t("vault.recovery.code")} hint={vault.hasRecovery ? t("vault.recovery.rowHintOn") : t("vault.recovery.rowHintOff")}>
              <button className="btn" onClick={() => setRecoveryAsk("create")}>
                {vault.hasRecovery ? t("vault.recovery.replace") : t("vault.recovery.create")}
              </button>
            </Row>
            <Row title={t("settings.autoLock")} hint={t("settings.autoLockHint")}>
              <Segmented
                label={t("settings.autoLockLabel")}
                mono
                value={settings.autoLockMinutes}
                options={[0, 5, 15, 30, 60].map((m) => ({ value: m, label: m ? t("settings.minutes", { n: m }) : t("common.off") }))}
                onChange={(autoLockMinutes) => set.mutate({ autoLockMinutes })}
              />
            </Row>
          </>
        )}
        <Row
          title={t("settings.clipboard")}
          hint={master ? t("settings.clipboardHintMaster") : t("settings.clipboardHintHello")}
        >
          <NumberField
            unit={t("settings.unit.seconds")}
            short={t("settings.unit.s")}
            min={5}
            max={300}
            value={settings.clipboardClearSeconds}
            onCommit={(clipboardClearSeconds) => set.mutate({ clipboardClearSeconds })}
          />
        </Row>
        <PublicNetworkRow settings={settings} />
        </Section>

        <Section id="data" title={t("settings.section.data")}>
        <Row title={t("settings.dataFolder")} hint={dataDir ? <PathHint path={dataDir.path} /> : ""}>
          <div className="flex flex-wrap gap-2">
            <button className="btn" onClick={() => api.dataDirOpen().catch(toastError)}>{t("common.open")}</button>
            <button className="btn" title={t("settings.openLogsHint")} onClick={() => api.logsOpen().catch(toastError)}>{t("settings.openLogs")}</button>
            {dataDir && !dataDir.overridden && <button className="btn" onClick={pickDataDir}>{t("settings.moveEllipsis")}</button>}
          </div>
        </Row>
        <Row
          title={t("settings.cache")}
          hint={t("settings.cacheHint")}
        >
          <NumberField
            unit={t("settings.unit.minutes")}
            short={t("settings.unit.min")}
            min={1}
            max={1440}
            value={settings.cacheMinutes}
            onCommit={(cacheMinutes) => set.mutate({ cacheMinutes })}
          />
        </Row>
        <Row title={t("settings.backup")} hint={t("settings.backupHint")}>
          <div className="flex flex-wrap gap-2">
            <button className="btn" onClick={pickImport}>
              <Icon name="upload" size={14} />
              {t("settings.importEllipsis")}
            </button>
            <button className="btn btn-primary" onClick={() => setDlg("export")}>
              <Icon name="download" size={14} />
              {t("settings.exportEllipsis")}
            </button>
          </div>
        </Row>
        </Section>

        <Section id="about" title={t("settings.section.about")}>
        <SelfUpdateRow on={settings.selfUpdate} onChange={(selfUpdate) => set.mutate({ selfUpdate })} />
        <Row title={<>RoALT <span className="mono">{import.meta.env.APP_VERSION}</span></>} hint={t("settings.aboutHint")} last>
          <button className="btn" onClick={() => openUrl(RELEASES).catch(toastError)}>
            {t("settings.openReleases")}
            <Icon name="external" size={12} />
          </button>
        </Row>
        </Section>
        </div>
        <div className="flex flex-col gap-8 min-w-0">
        <Section id="launching" title={t("settings.section.launching")}>
        <Row
          title={t("settings.multi")}
          hint={t("settings.multiHint")}
        >
          <Switch label={t("settings.multi")} checked={multi?.enabled ?? settings.multiInstance} onChange={multiToggle.toggle} />
          {multiToggle.dialog}
        </Row>
        <MultiNotice wanted={false} />
        <Row
          title={t("settings.protectLogin")}
          hint={t("settings.protectLoginHint")}
        >
          <Switch
            label={t("settings.protectLogin")}
            checked={settings.protectSharedLogin}
            onChange={(protectSharedLogin) => set.mutate({ protectSharedLogin }, { onSuccess: () => qc.invalidateQueries({ queryKey: qk.multi }) })}
          />
        </Row>
        <Row title={t("settings.launchDelay")} hint={t("settings.launchDelayHint")}>
          <NumberField
            unit={t("settings.unit.secondsBetween")}
            short={t("settings.unit.s")}
            min={2}
            max={60}
            step={0.5}
            value={settings.launchDelayMs / 1000}
            onCommit={(s) => set.mutate({ launchDelayMs: Math.round(s * 1000) })}
          />
        </Row>
        <Row title={t("settings.headless")} hint={t("settings.headlessHint")}>
          <Switch label={t("settings.headless")} checked={settings.headless} onChange={(headless) => set.mutate({ headless })} />
        </Row>
        <Row title={t("settings.keepRunning")} hint={t("settings.keepRunningHint")}>
          <Switch label={t("settings.keepRunning")} checked={settings.keepRunningOnClose} onChange={(keepRunningOnClose) => set.mutate({ keepRunningOnClose })} />
        </Row>
        <Row title={t("settings.performanceMoved")} hint={t("settings.performanceMovedHint")}>
          <button className="btn" onClick={onPerformance}>
            {t("settings.openPerformance")}
          </button>
        </Row>
        <Row title={t("settings.autoRejoin")} hint={t("settings.autoRejoinHint")}>
          <Switch label={t("settings.autoRejoin")} checked={settings.autoRejoin} onChange={(autoRejoin) => set.mutate({ autoRejoin })} />
        </Row>
        <Row
          title={t("settings.autoRestart")}
          hint={t("settings.autoRestartHint")}
        >
          <Switch
            label={t("settings.autoRestart")}
            checked={settings.autoRestartOnCrash}
            onChange={(autoRestartOnCrash) => set.mutate({ autoRestartOnCrash })}
          />
        </Row>
        <Row
          title={t("settings.antiAfk")}
          hint={settings.antiAfk.enabled ? t("settings.antiAfkHowHint") : t("settings.antiAfkHint")}
        >
          <Switch label={t("settings.antiAfk")} checked={settings.antiAfk.enabled} onChange={(enabled) => set.mutate({ antiAfk: { ...settings.antiAfk, enabled } })} />
        </Row>
        {settings.antiAfk.enabled && (
          <Row title={t("settings.antiAfkEvery")} hint={t("settings.antiAfkEveryHint")}>
            <NumberField
              unit={t("settings.unit.minutes")}
              short={t("settings.unit.min")}
              min={1}
              max={19}
              value={settings.antiAfk.intervalMin}
              onCommit={(intervalMin) => set.mutate({ antiAfk: { ...settings.antiAfk, intervalMin } })}
            />
          </Row>
        )}
        <Row
          title={t("settings.breaker")}
          hint={t("settings.breakerHint")}
        >
          <div className="flex flex-wrap gap-3">
            <NumberField unit={t("settings.unit.relaunches")} short={t("settings.unit.tries")} min={1} max={20} value={settings.breakerMax} onCommit={(breakerMax) => set.mutate({ breakerMax })} />
            <NumberField
              label={t("settings.breakerIn")}
              unit={t("settings.unit.minutes")}
              short={t("settings.unit.min")}
              min={1}
              max={240}
              value={settings.breakerWindowMin}
              onCommit={(breakerWindowMin) => set.mutate({ breakerWindowMin })}
            />
          </div>
        </Row>
        <Row title={t("settings.closeGrace")} hint={t("settings.closeGraceHint")}>
          <NumberField unit={t("settings.unit.seconds")} short={t("settings.unit.s")} min={1} max={60} value={settings.closeGraceS} onCommit={(closeGraceS) => set.mutate({ closeGraceS })} />
        </Row>
        <Row
          title={t("settings.launchArgs")}
          hint={t("settings.launchArgsHint")}
        >
          <TextField
            label={t("settings.launchArgs")}
            className="w-64"
            placeholder={t("settings.placeholderNone")}
            value={settings.launchArgs}
            onCommit={(launchArgs) => set.mutateAsync({ launchArgs })}
          />
        </Row>
        <ClientRow client={client} />
        <Row
          title={t("settings.autoUpdate")}
          hint={t("settings.autoUpdateHint")}
        >
          <Switch label={t("settings.autoUpdate")} checked={settings.autoUpdate} onChange={(autoUpdate) => set.mutate({ autoUpdate })} />
        </Row>
        </Section>

        <Section id="shortcuts" title={t("settings.section.shortcuts")}>
        <table className="w-full border-collapse mt-2">
          <tbody>
            {SHORTCUTS.map(([what, key]) => (
              <tr key={key} className="h-10 border-b border-line">
                <td>{t(what)}</td>
                <td className="text-end"><span className="kbd">{key}</span></td>
              </tr>
            ))}
          </tbody>
        </table>
        </Section>
        </div>
        </div>
      </div>

      <PasswordDialog
        open={dlg === "set"}
        onClose={() => setDlg(null)}
        title={t("settings.setMasterTitle")}
        text={t("settings.setMasterText")}
        submitLabel={t("settings.setMasterSubmit")}
        next
        nextLabel={t("settings.masterPassword")}
        onSubmit={async ({ next }) => {
          await setMode("master", undefined, next, t("settings.setMasterDone"));
          setRecoveryAsk("offer");
        }}
      />
      <PasswordDialog
        open={!!recoveryAsk}
        onClose={() => setRecoveryAsk(null)}
        title={recoveryAsk === "offer" ? t("vault.recovery.offerTitle") : vault.hasRecovery ? t("vault.recovery.replaceTitle") : t("vault.recovery.createTitle")}
        text={recoveryAsk === "offer" ? t("vault.recovery.offerText") : vault.hasRecovery ? t("vault.recovery.replaceText") : t("vault.recovery.createText")}
        submitLabel={vault.hasRecovery ? t("vault.recovery.replace") : t("vault.recovery.create")}
        cancelLabel={recoveryAsk === "offer" ? t("vault.recovery.skip") : undefined}
        current
        onSubmit={async ({ current }) => {
          onRecoveryCode(await api.vaultRecoveryCreate(current));
          qc.invalidateQueries({ queryKey: qk.vault });
        }}
      />
      <PasswordDialog
        open={dlg === "change"}
        onClose={() => setDlg(null)}
        title={t("settings.changeMasterTitle")}
        text={vault.hasRecovery ? `${t("settings.changeMasterText")} ${t("vault.recovery.changeReminder")}` : t("settings.changeMasterText")}
        submitLabel={t("settings.changeMasterSubmit")}
        current
        currentLabel={t("settings.currentPassword")}
        next
        onSubmit={({ current, next }) => setMode("master", current, next, t("settings.changeMasterDone"))}
      />
      <PasswordDialog
        open={dlg === "remove"}
        onClose={() => setDlg(null)}
        title={t("settings.removeMasterTitle")}
        text={t("settings.removeMasterText")}
        submitLabel={t("settings.removeMasterSubmit")}
        current
        onSubmit={({ current }) => setMode("dpapi", current, undefined, t("settings.removeMasterDone"))}
      />
      <PasswordDialog
        open={dlg === "export"}
        onClose={() => setDlg(null)}
        title={t("settings.exportTitle")}
        text={t("settings.exportText")}
        submitLabel={t("settings.exportSubmit")}
        current={master}
        currentLabel={t("settings.masterPassword")}
        next
        nextLabel={t("settings.backupPassword")}
        onSubmit={async ({ current, next }) => {
          const date = new Date().toISOString().slice(0, 10);
          const path = await saveFile({ defaultPath: `RoALT-backup-${date}.roalt`, filters: backupFilter() });
          if (!path) throw cancelled();
          const n = await api.backupExport(path, next, master ? current : undefined);
          toast(t("settings.exported", { count: n }));
        }}
      />
      <ImportDialog open={importOpen} path={importPath} onClose={() => setImportOpen(false)} />
      <ConfirmDialog
        open={moveOpen}
        onClose={() => !moving && setMoveOpen(false)}
        onConfirm={() => void moveData()}
        busy={moving}
        title={t("settings.moveTitle")}
        text={tx("settings.moveText", { path: <span className="mono text-text break-all">{movePath}</span> })}
        confirmLabel={moving ? t("settings.moving") : t("settings.moveRestart")}
        safe
      />
    </main>
  );
}

function ImportDialog({ open, path, onClose }: { open: boolean; path: string; onClose: () => void }) {
  const [pw, setPw] = useState("");
  const [policy, setPolicy] = useState<ConflictPolicy>("skip");
  const [report, setReport] = useState<ImportReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) {
      setPw("");
      return;
    }
    setReport(null);
    setError(null);
  }, [open]);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!pw || busy) return;
    setBusy(true);
    setError(null);
    try {
      setReport(await api.backupImport(path, pw, policy));
      setPw("");
    } catch (err) {
      const e2 = asError(err);
      setError(e2.kind === "wrongPassword" || e2.kind === "crypto" ? t("settings.importWrongPassword") : friendly(e2));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onClose={onClose} title={t("settings.importTitle")}>
      {report ? (
        <div className="enter flex flex-col">
          <div className="grid grid-cols-4 border-b border-line">
            {(["added", "updated", "skipped", "failed"] as const).map((k) => (
              <div key={k} className="px-5 py-4 border-e border-line last:border-e-0">
                <div className="label">{t(({ added: "settings.importAdded", updated: "settings.importUpdated", skipped: "settings.importSkipped", failed: "settings.importFailed" } as const)[k])}</div>
                <div className="mono text-[22px] font-medium mt-0.5">{report[k]}</div>
              </div>
            ))}
          </div>
          <div className="flex justify-end p-5">
            <button className="btn btn-primary" onClick={onClose} data-autofocus>{t("common.done")}</button>
          </div>
        </div>
      ) : (
        <form onSubmit={submit} className="flex flex-col gap-4 p-5">
          <div className="well mono text-xs text-dim break-all px-3 py-2.5 selectable">{path.split(/[\\/]/).pop()}</div>
          <label className="flex flex-col gap-1.5">
            <span className="label">{t("settings.backupPassword")}</span>
            <input
              type="password"
              className="field"
              value={pw}
              onChange={(e) => setPw(e.target.value)}
              autoComplete={t("settings.placeholderOff")}
              aria-invalid={!!error}
            />
          </label>
          <div className="flex flex-col gap-1.5">
            <span className="label">{t("settings.importExisting")}</span>
            <Segmented
              label={t("settings.importExisting")}
              value={policy}
              options={[{ value: "skip", label: t("settings.importKeep") }, { value: "replace", label: t("settings.importReplace") }]}
              onChange={setPolicy}
            />
          </div>
          {error && <div role="alert" className="alert">{error}</div>}
          <div className="flex justify-end gap-2 pt-1">
            <button type="button" className="btn" onClick={onClose}>{t("common.cancel")}</button>
            <button type="submit" className="btn btn-primary" disabled={busy || !pw}>{busy ? t("settings.importing") : t("settings.import")}</button>
          </div>
        </form>
      )}
    </Dialog>
  );
}

/** The Roblox bandwidth limit: typed, then applied with admin rights only on the button. */
export function BandwidthRow() {
  const qc = useQueryClient();
  const status = useQuery({ queryKey: qk.bandwidth, queryFn: api.bandwidthStatus, staleTime: 0 }).data;
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (status) setText(status.kbps ? String(status.kbps) : "");
  }, [status]);
  const apply = (kbps: number) => {
    setBusy(true);
    api
      .bandwidthApply(kbps)
      .then((st: BandwidthStatus) => {
        qc.setQueryData(qk.bandwidth, st);
        qc.invalidateQueries({ queryKey: qk.settings });
        toast(kbps ? t("settings.bandwidthLimited", { kbps: fmtNum(kbps) }) : t("settings.bandwidthOffToast"));
      }, toastError)
      .finally(() => setBusy(false));
  };
  const n = Number(text || "0");
  const valid = Number.isInteger(n) && (n === 0 || (n >= 64 && n <= 10_000_000));
  const state = !status ? "" : !status.kbps ? t("settings.bandwidthStateOff") : status.active ? t("settings.bandwidthInForce", { kbps: fmtNum(status.kbps) }) : t("settings.bandwidthStale", { kbps: fmtNum(status.kbps) });
  return (
    <Row
      title={t("settings.bandwidth")}
      hint={
        <>
          {t("settings.bandwidthHint")} {state}
        </>
      }
    >
      <div className="flex flex-wrap items-center gap-2">
        <label className="flex items-center gap-2">
          <span className="sr-only">{t("settings.bandwidthUnit")}</span>
          <input
            className="field mono w-24"
            type="number"
            min={0}
            max={10_000_000}
            placeholder={t("settings.placeholderNone")}
            value={text}
            aria-invalid={!valid}
            onChange={(e) => setText(e.target.value)}
          />
          <span className="text-dim">kbit/s</span>
        </label>
        <button
          className="btn"
          disabled={busy || !valid}
          title={valid ? t("settings.adminRights") : t("settings.bandwidthRange")}
          onClick={() => apply(n)}
        >
          {busy ? t("settings.waitingWindows") : t("settings.bandwidthApply")}
        </button>
        {status?.kbps !== 0 && status && (
          <button className="btn" disabled={busy} onClick={() => apply(0)}>{t("settings.bandwidthRemove")}</button>
        )}
      </div>
    </Row>
  );
}

const SHUTDOWN_PRESETS = [30, 60, 120, 240, 480];

/** "1 h 05 min" style, for a preset or what is left. */
function duration(secs: number) {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.floor(secs % 60);
  const pad = (n: number) => String(n).padStart(2, "0");
  return h ? t("settings.durHours", { h, m: pad(m) }) : m ? t("settings.durMinutes", { m, s: pad(s) }) : t("settings.durSeconds", { s });
}

/** The next time the clock shows `hhmm` ("21:30"), as Unix ms. */
function nextAt(hhmm: string) {
  const [h, m] = hhmm.split(":").map(Number);
  const at = new Date();
  at.setHours(h, m, 0, 0);
  if (at.getTime() <= Date.now()) at.setDate(at.getDate() + 1);
  return at.getTime();
}

/** Auto-shutdown: Windows' own timer (shutdown /s), with a countdown and Cancel. */
function ShutdownRow() {
  const qc = useQueryClient();
  const at = useQuery({ queryKey: qk.shutdown, queryFn: api.shutdownStatus, staleTime: 0 }).data ?? null;
  const [time, setTime] = useState("");
  const [closeClients, setCloseClients] = useState(true);
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (at === null) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [at]);
  const run = (p: Promise<number | null>, done: (at: number | null) => string) => {
    setBusy(true);
    p.then((a) => {
      qc.setQueryData(qk.shutdown, a);
      toast(done(a));
    }, toastError).finally(() => setBusy(false));
  };
  const schedule = (when: { atMs?: number; inSecs?: number }) =>
    run(api.shutdownSchedule(when, closeClients), (a) => t("settings.shutdownAt", { time: fmtDate(a ?? 0, { hour: "2-digit", minute: "2-digit" }) }));
  return (
    <Row
      title={t("settings.shutdown")}
      hint={
        closeClients ? t("settings.shutdownHintClose") : t("settings.shutdownHint")
      }
    >
      {at !== null ? (
        <div className="flex flex-wrap items-center gap-3">
          <span className="mono" role="timer">
            {t("settings.shutdownIn", { left: duration(Math.max(0, (at - now) / 1000)), time: fmtDate(at, { hour: "2-digit", minute: "2-digit" }) })}
          </span>
          <button className="btn" disabled={busy} onClick={() => run(api.shutdownCancel(), () => t("settings.shutdownOff"))}>{t("common.cancel")}</button>
        </div>
      ) : (
        <div className="flex flex-col items-end gap-2">
          <div className="flex flex-wrap items-center gap-1.5">
            {SHUTDOWN_PRESETS.map((m) => (
              <button key={m} className="btn btn-sm mono" disabled={busy} onClick={() => schedule({ inSecs: m * 60 })}>
                {m < 60 ? t("settings.minutesShort", { n: m }) : t("settings.hoursShort", { n: m / 60 })}
              </button>
            ))}
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <input className="field mono" type="time" aria-label={t("settings.shutdownAtLabel")} value={time} onChange={(e) => setTime(e.target.value)} />
            <button className="btn btn-sm" disabled={busy || !time} onClick={() => schedule({ atMs: nextAt(time) })}>{t("settings.atThisTime")}</button>
            <label className="flex items-center gap-2">
              <span className="text-xs text-dim">{t("settings.closeClientsFirst")}</span>
              <Switch label={t("settings.closeClientsFirst")} checked={closeClients} onChange={setCloseClients} />
            </label>
          </div>
        </div>
      )}
    </Row>
  );
}

/** Public networks: allowed by default; turned off, launching is refused there and a master
 * vault locks on joining one. Allowing them again is confirmed. */
function PublicNetworkRow({ settings }: { settings: Settings }) {
  const set = useSetSettings();
  const network = useNetwork().data;
  const [confirm, setConfirm] = useState(false);
  const allowed = settings.allowPublicNetworks;
  const now = network?.public ? t("settings.publicNow") : t("settings.publicNotNow");
  return (
    <>
      <Row
        last
        title={t("settings.public")}
        hint={
          <>
            {t("settings.publicHint")} {now}
          </>
        }
      >
        <Switch
          label={t("settings.public")}
          checked={allowed}
          onChange={(on) => (on ? setConfirm(true) : set.mutate({ allowPublicNetworks: false }))}
        />
      </Row>
      <ConfirmDialog
        open={confirm}
        onClose={() => setConfirm(false)}
        onConfirm={() => {
          setConfirm(false);
          set.mutate({ allowPublicNetworks: true });
        }}
        title={t("settings.publicConfirmTitle")}
        text={
          t("settings.publicConfirmText")
        }
        confirmLabel={t("settings.publicConfirm")}
      />
    </>
  );
}

/** Deletes old Roblox logs now and says how much it freed. */
function CleanLogsButton() {
  const [busy, setBusy] = useState(false);
  const clean = () => {
    setBusy(true);
    api
      .logsClean()
      .then(
        (r) =>
          toast(
            r.files
              ? t("settings.logsDeleted", { count: r.files, mb: fmtNum(r.bytes / 1048576, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) })
              : t("settings.logsNone"),
          ),
        toastError,
      )
      .finally(() => setBusy(false));
  };
  return (
    <button className="btn btn-sm" disabled={busy} onClick={clean}>
      {busy ? t("settings.cleaning") : t("settings.cleanNow")}
    </button>
  );
}

export function Heading({ id, children }: { id: string; children: ReactNode }) {
  return <h2 id={id} className="section-title mt-8 first-of-type:mt-5 scroll-mt-3">{children}</h2>;
}

/** A section that folds. Native <details> keeps its children mounted, so an install's progress and
 * half-typed fields survive folding. The sidebar's jump opens it from outside; onToggle hears that too. */
function Section({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  const toggled = (open: boolean) => {
    if (open) closed.delete(id);
    else closed.add(id);
    try {
      localStorage.setItem(CLOSED_KEY, JSON.stringify([...closed]));
    } catch {
      // No storage: kept in `closed`.
    }
  };
  return (
    <details className="group" open={!closed.has(id)} onToggle={(e) => toggled(e.currentTarget.open)}>
      <summary className="flex items-center gap-2 w-fit cursor-pointer">
        <h2 id={id} className="section-title scroll-mt-3">{title}</h2>
        <Icon name="chevron" size={14} className="text-dim -rotate-90 rtl:rotate-90 group-open:rotate-0 rtl:group-open:rotate-0" />
      </summary>
      <div className="panel px-5 py-1 mt-3 [&>*:last-child]:border-b-0">{children}</div>
    </details>
  );
}

function PathHint({ path }: { path: string }) {
  return <span className="mono block truncate selectable" title={path}>{path}</span>;
}

const ROBLOX_DOWNLOAD = "https://www.roblox.com/download";

function installStep(p: InstallProgress | null) {
  switch (p?.stage) {
    case "downloading":
      return t("settings.installDownloading", { percent: p.percent });
    case "verifying":
      return t("settings.installVerifying");
    case "installing":
      return t("settings.installRunning");
    default:
      return null;
  }
}

/** The self-updater's switch and Check now, with what the check found. */
function SelfUpdateRow({ on, onChange }: { on: boolean; onChange: (on: boolean) => void }) {
  const qc = useQueryClient();
  const [result, setResult] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function check() {
    setBusy(true);
    setResult(null);
    try {
      const u = await api.selfUpdateCheck();
      unhideUpdate();
      qc.setQueryData(qk.appUpdate, u);
      setResult(u.available ? t("settings.updateAvailable", { version: u.available }) : t("settings.upToDate"));
    } catch (e) {
      setResult(friendly(asError(e)));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Row
      title={t("settings.selfUpdate")}
      hint={t("settings.selfUpdateHint")}
    >
      <span className="flex flex-wrap items-center gap-3">
        {result && <span className="text-xs text-dim" role="status">{result}</span>}
        <button className="btn btn-sm" disabled={busy} onClick={() => void check()}>
          {busy ? t("settings.checking") : t("settings.checkNow")}
        </button>
        <Switch label={t("settings.selfUpdate")} checked={on} onChange={onChange} />
      </span>
    </Row>
  );
}

/** The client RoALT starts, its version, and Install / Update Roblox (Roblox's own client only). */
function ClientRow({ client }: { client: RobloxClient | undefined }) {
  const qc = useQueryClient();
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const [failed, setFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    const un = events.onRobloxInstall(setProgress);
    return () => void un.then((f) => f());
  }, []);
  const step = installStep(progress);
  const offer = client && !client.bootstrapper;

  async function install() {
    if (busy) return;
    setBusy(true);
    setFailed(false);
    try {
      const c = await api.robloxInstall();
      qc.setQueryData(qk.client, c);
      toast(c.version ? t("settings.robloxInstalledVersion", { version: c.version }) : t("settings.robloxInstalled"));
    } catch (e) {
      setFailed(true);
      toastError(e);
    } finally {
      setBusy(false);
    }
  }

  const version = client?.path && !client.bootstrapper && (
    <span className="mono block truncate">
      {client.outdated && client.latest
        ? t("settings.clientOutdated", { version: client.version ?? t("settings.versionUnknown"), latest: client.latest })
        : client.latest && client.version === client.latest
          ? t("settings.clientLatest", { version: client.version })
          : (client.version ?? t("settings.versionUnknown"))}
    </span>
  );
  return (
    <Row
      title={t("settings.client")}
      hint={
        client?.path ? (
          <>
            <PathHint path={client.path} />
            {version}
          </>
        ) : client ? (
          t("settings.robloxMissing")
        ) : (
          ""
        )
      }
    >
      <span className="flex flex-wrap items-center gap-3">
        {client && <ClientStatus client={client} />}
        {step ? (
          <span className="text-xs text-dim" role="status">{step}</span>
        ) : (
          offer && (
            <button className="btn btn-sm" disabled={busy} onClick={() => void install()} title={t("settings.clientInstallHint")}>
              {!client.path ? t("settings.installRoblox") : client.outdated ? t("settings.updateRoblox") : t("settings.reinstallRoblox")}
            </button>
          )
        )}
        {failed && !step && (
          <button className="btn btn-sm" onClick={() => openUrl(ROBLOX_DOWNLOAD).catch(toastError)}>
            <Icon name="external" size={11} />
            roblox.com/download
          </button>
        )}
      </span>
    </Row>
  );
}

function ClientStatus({ client }: { client: RobloxClient }) {
  const [label, color] = !client.path
    ? [t("settings.clientNotInstalled"), "var(--orange)"]
    : client.bootstrapper
      ? [t("settings.clientBootstrapper"), "var(--blue)"]
      : client.outdated
        ? [t("settings.clientOutdatedBadge"), "var(--orange)"]
        : [t("settings.clientDetected"), "var(--accent-text)"];
  return (
    <span className="mono inline-flex items-center gap-1.5 text-xs">
      <span aria-hidden="true" className="w-1.5 h-1.5 flex-none" style={{ background: color }} />
      {label}
    </span>
  );
}

function AccentPicker({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const custom = !!value && !ACCENTS.some(([v]) => v === value);
  return (
    <div role="group" aria-label={t("settings.accentColour")} className="flex flex-wrap items-center gap-2">
      {ACCENTS.map(([v, name]) => (
        <button
          key={name}
          type="button"
          title={t(name)}
          aria-label={t(name)}
          aria-pressed={v === value}
          className={`w-7 h-7 rounded-full border forced-color-adjust-none ${v === value ? "border-text outline-2 outline-text outline-offset-2 focus-visible:outline-[var(--focus)]" : "border-line-strong"}`}
          style={{ background: v || "#c6f432" }}
          onClick={() => v !== value && onChange(v)}
        />
      ))}
      <label title={t("settings.anyColour")} className={`relative w-7 h-7 rounded-full overflow-hidden border cursor-pointer has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-[var(--focus)] has-[:focus-visible]:outline-offset-2 ${custom ? "border-text outline-2 outline-text outline-offset-2" : "border-line-strong"}`}
        style={{ background: custom ? value : "conic-gradient(red, yellow, lime, cyan, blue, magenta, red)" }}>
        <span className="sr-only">{t("settings.customColour")}</span>
        <input type="color" className="absolute inset-0 opacity-0 cursor-pointer" value={value || "#c6f432"} onChange={(e) => onChange(e.target.value)} />
      </label>
    </div>
  );
}

/** Pick, replace or remove the window's background image, and how much the theme covers it. */
function BackgroundRow({ dim, onDim }: { dim: number; onDim: (v: number) => void }) {
  const qc = useQueryClient();
  const has = !!useQuery({ queryKey: qk.background, queryFn: api.backgroundGet, staleTime: Infinity }).data?.byteLength;
  const done = () => qc.invalidateQueries({ queryKey: qk.background });
  const pick = async () => {
    const path = await openFile({ multiple: false, directory: false, filters: pickerFilter(), title: t("settings.backgroundDialog") }).catch(toastError);
    if (path) api.backgroundSet(path).then(done, toastError);
  };
  return (
    <Row title={t("settings.background")} hint={t("settings.backgroundHint")}>
      <div className="flex flex-wrap items-center gap-2">
        {has && (
          <Segmented
            label={t("settings.backgroundDim")}
            value={dim}
            options={[40, 60, 80].map((v) => ({ value: v, label: `${v}%`, title: t("settings.backgroundCovers", { n: v }) }))}
            onChange={onDim}
            mono
          />
        )}
        <button className="btn" onClick={pick}>{has ? t("settings.change") : t("settings.choose")}</button>
        {has && <button className="btn" onClick={() => api.backgroundClear().then(done, toastError)}>{t("common.remove")}</button>}
      </div>
    </Row>
  );
}

export function Row({ title, hint, children, last }: { title: ReactNode; hint: ReactNode; children: ReactNode; last?: boolean }) {
  return (
    <div className={`flex flex-wrap items-center gap-x-6 gap-y-2 py-4 ${last ? "" : "border-b border-line"}`}>
      <div className="flex-[1_1_280px] min-w-0 flex flex-col gap-0.5">
        <span className="font-medium">{title}</span>
        <span className="text-[13px] text-dim">{hint}</span>
      </div>
      {children}
    </div>
  );
}
