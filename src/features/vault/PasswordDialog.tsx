import { useRef, useState, type FormEvent, type ReactNode } from "react";
import { asError } from "../../api";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { Field } from "../../ui/Field";
import { IconSwap, Morph, Swap } from "../../ui/motion";
import { friendly } from "../../ui/Toast";
import { getLang, t } from "../../i18n";
import { useOpenCount } from "./useOpenCount";
import "./vault.css";

export const MIN_PASSWORD = 8;

type FormProps = {
  text: ReactNode;
  submitLabel: string;
  onSubmit: (v: { current: string; next: string }) => Promise<unknown>;
  /** After a successful submit. */
  onDone: () => void;
  onCancel: () => void;
  /** While a submit runs (an owner can refuse to close meanwhile). */
  onBusy?: (busy: boolean) => void;
  current?: boolean;
  next?: boolean;
  currentLabel?: string;
  nextLabel?: string;
  cancelLabel?: string;
  /** What a wrong password says, when it isn't the vault's (a backup's own password, say). */
  wrongText?: string;
};

/** Where a problem shows: under its field, or under the whole form. */
type Slot = "current" | "next" | "confirm" | "form";

/**
 * The password step on its own (for a dialog with several steps): the current password, a new one
 * (with confirmation), both, or neither (a question with an async answer). onSubmit rejecting with
 * kind "cancelled" keeps it quietly; any other rejection shows inline, at the field it is about, and a
 * wrong password clears the field. One key shows or hides every field. Its passwords live in its own
 * state: they go once used, and when it unmounts.
 */
export function PasswordForm({
  text, submitLabel, onSubmit, onDone, onCancel, onBusy, wrongText,
  current = false, next = false, currentLabel = t("settings.masterPassword"), nextLabel = t("vault.newPassword"), cancelLabel = t("common.cancel"),
}: FormProps) {
  const form = useRef<HTMLFormElement>(null);
  const [cur, setCur] = useState("");
  const [nw, setNw] = useState("");
  const [confirm, setConfirm] = useState("");
  const [peek, setPeek] = useState(false);
  // The confirmation is checked once it has been left, then as either field changes.
  const [left, setLeft] = useState(false);
  const [problem, setProblem] = useState<{ at: Slot; text: string } | null>(null);
  const [shakes, setShakes] = useState(0);
  const [busy, setBusyState] = useState(false);
  const setBusy = (b: boolean) => {
    setBusyState(b);
    onBusy?.(b);
  };

  const fail = (at: Slot, msg: string) => {
    setProblem({ at, text: msg });
    setShakes((n) => n + 1);
    // To the field to fix; a problem of the whole form leaves focus where it is.
    requestAnimationFrame(() => form.current?.querySelector<HTMLElement>("input[aria-invalid=true]")?.focus());
  };
  const edit = (at: Slot, set: (v: string) => void) => (v: string) => {
    set(v);
    // A mismatch is about both passwords: either edit hands it to the live check.
    if (problem?.at === at || (at === "next" && problem?.at === "confirm")) setProblem(null);
  };
  const errorAt = (at: Slot) => (problem?.at === at ? problem.text : undefined);
  const mismatch = left && !!confirm && confirm !== nw;

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (busy) return;
    if (current && !cur) return fail("current", t("vault.enterField", { label: getLang() === "en" ? currentLabel.toLowerCase() : currentLabel }));
    if (next && nw.length < MIN_PASSWORD) return fail("next", t("vault.tooShort", { count: MIN_PASSWORD }));
    if (next && nw !== confirm) {
      setLeft(true);
      return fail("confirm", t("vault.mismatch"));
    }
    setBusy(true);
    setProblem(null);
    try {
      await onSubmit({ current: cur, next: nw });
      // Used: the typed passwords go now, not once the step or the dialog has finished leaving.
      setCur("");
      setNw("");
      setConfirm("");
      onDone();
    } catch (err) {
      const e2 = asError(err);
      if (e2.kind === "wrongPassword") setCur("");
      if (e2.kind !== "cancelled") fail(e2.kind === "wrongPassword" && current ? "current" : "form", e2.kind === "wrongPassword" ? (wrongText ?? t("ui.error.wrongPassword")) : friendly(e2));
    } finally {
      setBusy(false);
    }
  }

  const togglePeek = () => setPeek(!peek);
  return (
    <form ref={form} onSubmit={submit} className={`flex flex-col gap-4 px-[22px] pb-5 ${shakes ? (shakes % 2 ? "shake" : "shake2") : ""}`}>
      <div className="text-fg-2">{text}</div>
      {current && (
        <PasswordField
          label={currentLabel}
          value={cur}
          onChange={edit("current", setCur)}
          autoComplete="current-password"
          error={errorAt("current")}
          withKey
          autoFocus
          peek={peek}
          onPeek={togglePeek}
        />
      )}
      {next && (
        <>
          <PasswordField
            label={nextLabel}
            value={nw}
            onChange={edit("next", setNw)}
            autoComplete="new-password"
            hint={t("vault.minLength", { count: MIN_PASSWORD })}
            error={errorAt("next")}
            withKey={!current}
            autoFocus={!current}
            peek={peek}
            onPeek={togglePeek}
          />
          <PasswordField
            label={t("vault.confirm")}
            value={confirm}
            onChange={edit("confirm", setConfirm)}
            onBlur={() => setLeft(true)}
            autoComplete="new-password"
            error={errorAt("confirm") ?? (mismatch ? t("vault.mismatch") : undefined)}
            peek={peek}
            onPeek={togglePeek}
          />
        </>
      )}
      {problem?.at === "form" && <p role="alert" className="alert">{problem.text}</p>}
      <div className="flex justify-end gap-2 pt-1">
        <Button onClick={onCancel} disabled={busy}>{cancelLabel}</Button>
        {/* Not disabled while busy: the click that started it keeps focus (submit ignores a second one). */}
        <Button type="submit" variant="primary" busy={busy} data-autofocus={(!current && !next) || undefined}>
          <Swap k={busy ? "busy" : "idle"}>{busy ? t("vault.working") : submitLabel}</Swap>
        </Button>
      </div>
    </form>
  );
}

/** PasswordForm in its own dialog, its height following the inline errors. Passwords and the
 * show/hide key reset with every opening, and are dropped when the dialog closes. */
export function PasswordDialog({ open, onClose, title, ...form }: Omit<FormProps, "onDone" | "onCancel" | "onBusy"> & { open: boolean; onClose: () => void; title: string }) {
  const [busy, setBusy] = useState(false);
  const opened = useOpenCount(open);
  // Closing mid-request would not cancel it, so wait for it.
  const close = () => {
    if (!busy) onClose();
  };
  return (
    <Dialog open={open} onClose={close} title={title}>
      {open && (
        <Morph key={opened} axis="height">
          <PasswordForm {...form} onDone={onClose} onCancel={close} onBusy={setBusy} />
        </Morph>
      )}
    </Dialog>
  );
}

/** A password input with its label, hint and error (the unlock card's fields too). */
export function PasswordField({ label, value, onChange, onBlur, autoComplete, hint, error, withKey, autoFocus, peek, onPeek }: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  onBlur?: () => void;
  autoComplete: string;
  hint?: string;
  error?: string;
  /** The form's first field carries the key that shows or hides every field. */
  withKey?: boolean;
  autoFocus?: boolean;
  peek: boolean;
  onPeek: () => void;
}) {
  // A toggle keeps its name; aria-pressed (and the icon) tell whether it is on.
  const peekLabel = t("vault.showPassword");
  return (
    <Field label={label} hint={hint} error={error}>
      {({ id, describedBy, invalid }) => {
        const input = (
          <input
            id={id}
            type={peek ? "text" : "password"}
            className={withKey ? undefined : "field"}
            value={value}
            onChange={(e) => onChange(e.target.value)}
            onBlur={onBlur}
            autoComplete={autoComplete}
            spellCheck={false}
            aria-invalid={invalid || undefined}
            aria-describedby={describedBy}
            data-autofocus={autoFocus || undefined}
          />
        );
        if (!withKey) return input;
        return (
          <div className="field vault-pw">
            {input}
            {/* A pointer press leaves focus in the field, so typing goes on. */}
            <button type="button" className="wc" aria-label={peekLabel} data-tip={peekLabel} aria-pressed={peek} onPointerDown={(e) => e.preventDefault()} onClick={onPeek}>
              <IconSwap name={peek ? "eyeOff" : "eye"} />
            </button>
          </div>
        );
      }}
    </Field>
  );
}
