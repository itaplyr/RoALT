import { Fragment, useEffect, useRef, useState } from "react";
import { motion } from "motion/react";
import { save as saveFile } from "@tauri-apps/plugin-dialog";
import { api, asError } from "../../api";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { Checkbox } from "../../ui/Field";
import { Icon } from "../../ui/Icon";
import { DUR, IconSwap, RISE, SPRING, staggerDelay, Swap, useReducedMotionFlag } from "../../ui/motion";
import { friendly } from "../../ui/Toast";
import { t } from "../../i18n";
import "./vault.css";

type Note = { ok: boolean; text: string; by?: "copy" | "save" };

/**
 * A new recovery code, shown once: Copy, Save to file…, its two limits, and "I saved it", which
 * gates Done. The owner holds `saved`: it also decides whether Esc may close. Results show here, not
 * as toasts (a toast would sit under the dialog's backdrop).
 */
export function RecoveryCodeStep({ code, saved, onSaved, refused, onDone }: {
  code: string;
  saved: boolean;
  onSaved: (saved: boolean) => void;
  /** Counts the closes refused before the tick: each one nudges "I saved it" and moves focus to it. */
  refused: number;
  onDone: () => void;
}) {
  const [note, setNote] = useState<Note | null>(null);
  const reduced = useReducedMotionFlag();
  const tick = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (refused) tick.current?.querySelector("input")?.focus();
  }, [refused]);

  const failed = (e: unknown) => {
    const err = asError(e);
    if (err.kind !== "cancelled") setNote({ ok: false, text: friendly(err) });
  };
  const copy = () => api.vaultRecoveryCopy(code).then(() => setNote({ ok: true, text: t("vault.recovery.copied"), by: "copy" }), failed);
  async function saveToFile() {
    try {
      const path = await saveFile({ defaultPath: "RoALT-recovery-code.txt", filters: [{ name: t("vault.recovery.fileFilter"), extensions: ["txt"] }] });
      if (!path) return;
      await api.vaultRecoverySave(path, code);
      setNote({ ok: true, text: t("vault.recovery.savedTo"), by: "save" });
    } catch (e) {
      failed(e);
    }
  }

  return (
    <div className="dlg-body flex flex-col gap-4">
      <p className="text-fg-2">{t("vault.recovery.showText")}</p>
      {/* Left to right in every language: its groups are inline blocks, which bidi would reorder. */}
      <output className="vault-code selectable mono" dir="ltr" aria-label={t("vault.recovery.code")}>
        {code.split("-").map((group, i) => (
          <Fragment key={i}>
            {i > 0 && <span className="vault-dash">-</span>}
            <motion.span
              className="inline-block"
              initial={{ opacity: 0, y: reduced ? 0 : RISE.md }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ ...SPRING.smooth, delay: DUR.fast / 1000 + staggerDelay(i * 2) }}
            >
              {group}
            </motion.span>
          </Fragment>
        ))}
      </output>
      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap gap-2">
          <Button className="flex-1" onClick={copy} data-autofocus>
            <IconSwap name={note?.by === "copy" ? "check" : "copy"} />
            {t("common.copy")}
          </Button>
          <Button className="flex-1" onClick={saveToFile}>
            <IconSwap name={note?.by === "save" ? "check" : "download"} />
            {t("vault.recovery.saveFile")}
          </Button>
        </div>
        <p role="status" className="vault-note">
          <Swap k={note?.text ?? ""} variant="rise">
            {note && (
              <span className={note.ok ? "inline-flex items-center gap-1.5 text-fg-2" : "alert"}>
                {note.ok && <Icon name="check" size={14} className="text-ok" />}
                {note.text}
              </span>
            )}
          </Swap>
        </p>
      </div>
      <ul className="vault-limits">
        <li>{t("vault.recovery.limitFile")}</li>
        <li>{t("vault.recovery.limitPc")}</li>
      </ul>
      <div className="flex flex-wrap items-center justify-between gap-3 pt-1">
        <div ref={tick} className={refused ? (refused % 2 ? "shake" : "shake2") : undefined}>
          <Checkbox checked={saved} onChange={onSaved} label={t("vault.recovery.savedTick")} />
        </div>
        <Button variant="primary" className="vault-done" disabled={!saved} onClick={onDone}>{t("common.done")}</Button>
      </div>
    </div>
  );
}

/** Shows a new recovery code, once. It won't close until the user says it is saved. */
export function RecoveryCodeDialog({ code, onClose }: { code: string | null; onClose: () => void }) {
  // Per showing: a new code (or the same one after a lock) starts unticked.
  const [gate, setGate] = useState({ code, saved: false, refused: 0 });
  if (gate.code !== code) setGate({ code, saved: false, refused: 0 });
  const close = () => (gate.saved ? onClose() : setGate({ ...gate, refused: gate.refused + 1 }));
  return (
    <Dialog open={!!code} onClose={close} title={t("vault.recovery.showTitle")} width={460}>
      {code && <RecoveryCodeStep key={code} code={code} saved={gate.saved} onSaved={(saved) => setGate({ ...gate, saved })} refused={gate.refused} onDone={close} />}
    </Dialog>
  );
}
