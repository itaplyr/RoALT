import { useRef, useState, type FormEvent } from "react";
import { open as openFile, save as saveFile } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api, asError, type Profile } from "../../api";
import { t } from "../../i18n";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { Field } from "../../ui/Field";
import { Collapse } from "../../ui/motion";
import { fromCode as readCode, toCode as shareCode } from "../../ui/shareCode";
import { friendly } from "../../ui/Toast";
import { storedFps } from "../dashboard/previewFps";
import { useJumpTarget } from "../settings/jump";
import { errorText } from "../settings/kit/rowError";
import { SettingSection } from "../settings/kit/SettingSection";

const profileFilter = () => [{ name: t("performance.profileFilter"), extensions: ["json"] }];
/** A share code: this prefix, then the profile's JSON as base64url. */
const CODE = "RoALT-PERF:";
const toCode = (json: string) => shareCode(CODE, json);
const fromCode = (code: string) => readCode(CODE, code);

/** Export, import and share the settings above. What happened shows under the buttons. `block`: its
 * name on Settings' page. */
export function ShareRow({ onProfile, block }: { onProfile: (p: Profile) => void; block?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useJumpTarget("share", ref);
  const [pasteOpen, setPasteOpen] = useState(false);
  const [note, setNote] = useState<{ text: string; failed?: boolean } | null>(null);

  /** Runs one action; its answer (if any) is the result line, a failure the error line. */
  const act = (job: () => Promise<string | void>) => {
    setNote(null);
    job().then(
      (done) => done && setNote({ text: done }),
      (e) => {
        const text = errorText(e);
        if (text) setNote({ text, failed: true });
      },
    );
  };

  const copyCode = () =>
    act(async () => {
      await writeText(toCode(await api.performanceProfile(storedFps())));
      return t("performance.codeCopied");
    });
  const exportFile = () =>
    act(async () => {
      const path = await saveFile({ defaultPath: `RoALT-performance-${new Date().toISOString().slice(0, 10)}.json`, filters: profileFilter() });
      if (!path) return;
      await api.performanceExport(path, storedFps());
      return t("performance.exported");
    });
  const importFile = () =>
    act(async () => {
      const path = await openFile({ multiple: false, directory: false, filters: profileFilter() });
      if (path) onProfile(await api.performanceRead(path));
    });

  return (
    <SettingSection sub label="performance.shareTitle" block={block}>
      <div ref={ref} className="set-row">
        <div className="set-line">
          <p className="set-text set-hint">{t("performance.shareHint")}</p>
          <div className="set-control">
            <Button size="md" onClick={copyCode}>{t("performance.copyCode")}</Button>
            <Button size="md" aria-haspopup="dialog" onClick={() => setPasteOpen(true)}>{t("performance.pasteCode")}</Button>
            <Button size="md" variant="ghost" onClick={exportFile}>{t("performance.export")}</Button>
            <Button size="md" variant="ghost" onClick={importFile}>{t("performance.import")}</Button>
          </div>
        </div>
        <div role="status">
          <Collapse open={!!note && !note.failed}>
            <p className="set-result">{note?.text}</p>
          </Collapse>
        </div>
        <Collapse open={!!note?.failed}>
          <p role="alert" className="alert set-error">{note?.text}</p>
        </Collapse>
      </div>
      <PasteDialog
        open={pasteOpen}
        onClose={() => setPasteOpen(false)}
        onProfile={(p) => {
          setPasteOpen(false);
          onProfile(p);
        }}
      />
    </SettingSection>
  );
}

/** A pasted share code goes to the review once the backend has checked it. */
function PasteDialog({ open, onClose, onProfile }: { open: boolean; onClose: () => void; onProfile: (p: Profile) => void }) {
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [wasOpen, setWasOpen] = useState(open);
  // Closed, it forgets what was pasted.
  if (wasOpen !== open) {
    setWasOpen(open);
    if (!open) {
      setCode("");
      setError(null);
    }
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (busy) return;
    const json = fromCode(code);
    if (json === null) return setError(t("performance.badCode"));
    setBusy(true);
    try {
      onProfile(await api.performanceParse(json));
    } catch (err) {
      setError(friendly(asError(err)));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onClose={onClose} title={t("performance.pasteTitle")}>
      <form onSubmit={submit}>
        <div className="dlg-body">
          <Field label={t("performance.pasteLabel")} error={error}>
            {({ id, describedBy, invalid }) => (
              <textarea
                id={id}
                className="field mono text-xs"
                rows={4}
                value={code}
                onChange={(e) => {
                  setCode(e.target.value);
                  setError(null);
                }}
                autoComplete="off"
                autoCorrect="off"
                autoCapitalize="off"
                spellCheck={false}
                placeholder={`${CODE}…`}
                aria-invalid={invalid || undefined}
                aria-describedby={describedBy}
              />
            )}
          </Field>
        </div>
        <div className="dlg-foot">
          <Button size="md" onClick={onClose}>{t("common.cancel")}</Button>
          {/* Busy, aria-disabled: disabled would drop the focus, and leave it nowhere after an error. */}
          <Button size="md" type="submit" variant="primary" busy={busy} disabled={!code.trim()} aria-disabled={busy || undefined}>
            {t("performance.review")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
