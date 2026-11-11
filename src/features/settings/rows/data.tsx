import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { open as openFile, save as saveFile } from "@tauri-apps/plugin-dialog";
import { api, asError, type ConflictPolicy, type ImportReport } from "../../../api";
import { qk } from "../../../data";
import { fmtNum, t, tx } from "../../../i18n";
import { announce } from "../../../ui/a11y";
import { Skeleton } from "../../../ui/Bits";
import { Button } from "../../../ui/Button";
import { Segmented, Switch } from "../../../ui/controls";
import { ConfirmDialog, Dialog, DialogSteps } from "../../../ui/Dialog";
import { Num } from "../../../ui/motion";
import { friendly } from "../../../ui/Toast";
import { PasswordDialog, PasswordForm } from "../../vault/PasswordDialog";
import { useOpenCount } from "../../vault/useOpenCount";
import { ActionButton } from "../kit/ActionButton";
import { useRow } from "../kit/rowError";
import { SettingRow } from "../kit/SettingRow";

const backupFilter = () => [{ name: t("settings.backupFilter"), extensions: ["roalt"] }];

/** Open the folder or RoALT's own logs; move it elsewhere (confirmed: RoALT restarts to use it).
 * There is no Move while ROALT_DATA_DIR decides the folder. */
function FolderActions({ movable }: { movable: boolean }) {
  const row = useRow();
  const [to, setTo] = useState<string | null>(null);
  const [moving, setMoving] = useState(false);
  async function pick() {
    const path = await openFile({ directory: true, multiple: false, title: t("settings.moveDialog") }).catch(row.fail);
    if (path) setTo(path);
  }
  // Settles only on failure: on success RoALT restarts from the new folder.
  const move = () => {
    setMoving(true);
    api.dataDirMove(to!).catch((e) => {
      row.fail(e);
      setMoving(false);
      setTo(null);
    });
  };
  return (
    <>
      <Button size="md" onClick={() => api.dataDirOpen().catch(row.fail)}>{t("common.open")}</Button>
      <Button size="md" data-tip={t("settings.openLogsHint")} onClick={() => api.logsOpen().catch(row.fail)}>{t("settings.openLogs")}</Button>
      {movable && <Button size="md" onClick={() => void pick()}>{t("settings.moveEllipsis")}</Button>}
      <ConfirmDialog
        open={!!to}
        onClose={() => !moving && setTo(null)}
        onConfirm={() => !moving && move()}
        busy={moving}
        title={t("settings.moveTitle")}
        text={tx("settings.moveText", { path: <span className="mono text-fg break-all">{to}</span> })}
        confirmLabel={moving ? t("settings.moving") : t("settings.moveRestart")}
        safe
      />
    </>
  );
}

export function DataFolderRow() {
  const dir = useQuery({ queryKey: qk.dataDir, queryFn: api.dataDirInfo });
  const d = dir.data;
  return (
    <SettingRow
      id="dataFolder"
      detail={
        d ? (
          <span className="mono block truncate selectable text-fg-2" title={d.path}>{d.path}</span>
        ) : dir.isError ? (
          <span className="alert">{friendly(asError(dir.error))}</span>
        ) : (
          <Skeleton width={260} height={12} className="mt-1" />
        )
      }
    >
      <FolderActions movable={!!d && !d.overridden} />
    </SettingRow>
  );
}

const COUNTS = [
  ["added", "settings.importAdded"],
  ["updated", "settings.importUpdated"],
  ["skipped", "settings.importSkipped"],
  ["failed", "settings.importFailed"],
] as const;

/** What the import did; each count tweens up from 0 as the step arrives. */
function ImportResult({ report, onDone }: { report: ImportReport; onDone: () => void }) {
  const [arrived, setArrived] = useState(false);
  useEffect(() => {
    setArrived(true);
    announce(COUNTS.map(([k, label]) => `${fmtNum(report[k])} ${t(label)}`).join(", "));
  }, [report]);
  return (
    <div className="dlg-body flex flex-col gap-5">
      <dl className="well set-report">
        {COUNTS.map(([k, label]) => (
          <div key={k}>
            <dt className="meta">{t(label)}</dt>
            <dd className="set-count" style={k === "failed" && report.failed ? { color: "var(--attention)" } : undefined}>
              <Num value={arrived ? report[k] : 0} format={(n) => fmtNum(Math.round(n))} />
            </dd>
          </div>
        ))}
      </dl>
      <div className="flex justify-end">
        <Button variant="primary" onClick={onDone} data-autofocus>{t("common.done")}</Button>
      </div>
    </div>
  );
}

/** One backup file into RoALT: its password and what to do with accounts already here, then the
 * report, in one dialog. */
function ImportFlow({ path, onClose }: { path: string | null; onClose: () => void }) {
  const [policy, setPolicy] = useState<ConflictPolicy>("skip");
  const [report, setReport] = useState<ImportReport | null>(null);
  const [busy, setBusy] = useState(false);
  const got = useRef<ImportReport | null>(null);
  const close = () => !busy && onClose();
  return (
    <Dialog open={!!path} onClose={close} title={t("settings.importTitle")}>
      {path && (
        <DialogSteps step={report ? "report" : "form"}>
          {report ? (
            <ImportResult report={report} onDone={onClose} />
          ) : (
            <PasswordForm
              text={
                <div className="flex flex-col gap-4">
                  <p className="well mono text-[13px] break-all px-3 py-2.5 selectable">{path.split(/[\\/]/).pop()}</p>
                  <div className="flex flex-col gap-1.5">
                    <span className="label text-fg">{t("settings.importExisting")}</span>
                    <Segmented
                      label={t("settings.importExisting")}
                      value={policy}
                      options={[{ value: "skip", label: t("settings.importKeep") }, { value: "replace", label: t("settings.importReplace") }]}
                      onChange={setPolicy}
                    />
                  </div>
                </div>
              }
              submitLabel={t("settings.import")}
              current
              currentLabel={t("settings.backupPassword")}
              // A file that doesn't open may be damaged, not only locked with another password.
              wrongText={t("settings.importWrongPassword")}
              onSubmit={async ({ current }) => {
                got.current = await api.backupImport(path, current, policy);
              }}
              onDone={() => setReport(got.current)}
              onCancel={close}
              onBusy={setBusy}
            />
          )}
        </DialogSteps>
      )}
    </Dialog>
  );
}

/** Each opening starts over, even one that comes while the last is still fading out. */
function ImportDialog({ path, onClose }: { path: string | null; onClose: () => void }) {
  const opened = useOpenCount(!!path);
  return <ImportFlow key={opened} path={path} onClose={onClose} />;
}

function BackupActions() {
  const row = useRow();
  const master = row.ctx.vault.mode === "master";
  const [importing, setImporting] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  async function pick() {
    const path = await openFile({ multiple: false, directory: false, filters: backupFilter() }).catch(row.fail);
    if (path) setImporting(path);
  }
  return (
    <>
      <Button size="md" icon="upload" onClick={() => void pick()}>{t("settings.importEllipsis")}</Button>
      <Button size="md" icon="download" onClick={() => setExporting(true)}>{t("settings.exportEllipsis")}</Button>
      <ImportDialog path={importing} onClose={() => setImporting(null)} />
      <PasswordDialog
        open={exporting}
        onClose={() => setExporting(false)}
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
          // No file picked: the dialog stays, quietly.
          if (!path) throw { kind: "cancelled", message: "" };
          row.setResult(t("settings.exported", { count: await api.backupExport(path, next, master ? current : undefined) }));
        }}
      />
    </>
  );
}

export function BackupRow() {
  return (
    <SettingRow id="backup">
      <BackupActions />
    </SettingRow>
  );
}

/** Clean now (says what it freed) beside the switch that cleans at start and every hour. */
function CleanLogs() {
  const row = useRow();
  const [busy, setBusy] = useState(false);
  const clean = () => {
    setBusy(true);
    row.setError(null);
    row.setResult(null);
    api
      .logsClean()
      .then(
        (r) =>
          row.setResult(
            r.files ? t("settings.logsDeleted", { count: r.files, mb: fmtNum(r.bytes / 1048576, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) }) : t("settings.logsNone"),
          ),
        row.fail,
      )
      .finally(() => setBusy(false));
  };
  return (
    <>
      <ActionButton busy={busy} label={t("settings.cleanNow")} busyLabel={t("settings.cleaning")} onRun={clean} />
      <Switch label={t("settings.cleanLogs")} checked={row.ctx.s.cleanLogs} onChange={(cleanLogs) => void row.write({ cleanLogs })} describedBy={row.describedBy} />
    </>
  );
}

export function CleanLogsRow() {
  return (
    <SettingRow id="cleanLogs">
      <CleanLogs />
    </SettingRow>
  );
}

/** Forgets what the sign-in and session windows kept; refused while one of them is open. */
function ResetBrowserData() {
  const row = useRow();
  const [busy, setBusy] = useState(false);
  const reset = () => {
    setBusy(true);
    row.setError(null);
    row.setResult(null);
    api
      .browserDataReset()
      .then((done) => (done ? row.setResult(t("settings.browserDataDone")) : row.setError(t("settings.browserDataBusy"))), row.fail)
      .finally(() => setBusy(false));
  };
  // Not disabled while busy: that would drop the keyboard focus.
  return (
    <Button size="md" busy={busy} aria-describedby={row.describedBy} onClick={() => !busy && reset()}>
      {t("settings.browserDataReset")}
    </Button>
  );
}

export function BrowserDataRow() {
  return (
    <SettingRow id="browserData">
      <ResetBrowserData />
    </SettingRow>
  );
}
