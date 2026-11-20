import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion, useAnimate } from "motion/react";
import { api, asError, type VaultStatus } from "../../api";
import { qk } from "../../data";
import { Button } from "../../ui/Button";
import { Field, Input } from "../../ui/Field";
import { DUR, EASE, Morph, prefersReducedMotion, SPRING, Swap, TWEEN } from "../../ui/motion";
import { friendly } from "../../ui/Toast";
import { getLang, t } from "../../i18n";
import { PasswordField, useNewPassword } from "./PasswordDialog";
import { publishRecoveryCode } from "./recoveryCode";
import "./vault.css";

export type LockReason = "start" | "manual" | "idle";

/** What a form tells the card: a failed try, its problem gone, the vault open. */
type Card = { onFail: () => void; onFixed: () => void; onOpen: () => void };
/** How long the shackle lifts before the card starts to leave. */
const LIFT_MS = 120;

/**
 * The locked vault: one card, the master password in it. With a recovery code, "Forgot password?"
 * morphs the card into a reset (code, new password, confirm) that unlocks too. The fresh code goes
 * to `onRecovered` when given (the old path), else to RecoveryCodeHost.
 */
export function UnlockScreen({ status, reason, onRecovered }: { status: VaultStatus; reason: LockReason; onRecovered?: (code: string) => void }) {
  const qc = useQueryClient();
  const [card, animate] = useAnimate<HTMLDivElement>();
  const [recovering, setRecovering] = useState(false);
  const [wrong, setWrong] = useState(false);
  const [open, setOpen] = useState(false);

  const events: Card = {
    onFail: () => {
      setWrong(true);
      // Reduced motion: no shake; the red keyhole and the field's outline still say it.
      if (!prefersReducedMotion()) animate(card.current, { x: [0, -6, 5, -3, 2, 0] }, { duration: 0.42, ease: EASE.out });
    },
    onFixed: () => setWrong(false),
    // The shackle lifts and the card leaves; then the shell takes its place. On a timer, not the
    // animation's end: a window out of sight draws no frames, and the vault is open already.
    onOpen: () => {
      setWrong(false);
      setOpen(true);
      animate(card.current, { opacity: 0, scale: 0.97 }, { ...TWEEN.exit, delay: LIFT_MS / 1000 });
      setTimeout(() => {
        qc.setQueryData<VaultStatus>(qk.vault, { ...status, locked: false });
        qc.invalidateQueries();
      }, LIFT_MS + DUR.exit);
    },
  };
  const toggle = () => {
    setRecovering(!recovering);
    setWrong(false);
  };

  // From the tray with auto-lock off it comes as "idle" too.
  const why = reason === "idle" && status.autoLockMinutes > 0 ? t("vault.lockedIdle", { count: status.autoLockMinutes }) : reason === "start" ? "" : t("vault.lockedManual");
  const lead = [why, t("vault.prompt", { count: status.accounts })].filter(Boolean).join(" ");

  return (
    <main className="vault-stage scroll flex-1 min-w-0">
      <div ref={card} className="vault-card enter">
        <Lock open={open} wrong={wrong} />
        {/* The card springs to the new mode's height; the old one fades out before the new one fades in. */}
        <Morph axis="height" transition={SPRING.size}>
          <div className="relative">
            <AnimatePresence mode="popLayout" initial={false}>
              <motion.div
                key={recovering ? "recover" : "unlock"}
                initial={{ opacity: 0 }}
                animate={{ opacity: 1, transition: { ...TWEEN.fade, delay: DUR.exit / 1000 } }}
                exit={{ opacity: 0, transition: TWEEN.exit }}
              >
                {recovering ? (
                  <RecoverForm {...events} onBack={toggle} onRecovered={onRecovered} />
                ) : (
                  <UnlockForm {...events} lead={lead} hasRecovery={status.hasRecovery} onForgot={toggle} />
                )}
              </motion.div>
            </AnimatePresence>
          </div>
        </Morph>
        <p className="vault-local">{t("vault.local")}</p>
      </div>
    </main>
  );
}

function UnlockForm({ lead, hasRecovery, onForgot, onFail, onFixed, onOpen }: Card & { lead: string; hasRecovery: boolean; onForgot: () => void }) {
  const form = useRef<HTMLFormElement>(null);
  const label = t("settings.masterPassword");
  const [pw, setPw] = useState("");
  const [peek, setPeek] = useState(false);
  // `field`: about the password (under it, the field outlined); else about the attempt (under the form).
  const [problem, setProblem] = useState<{ field: boolean; text: string } | null>(null);
  // Without a code, a wrong password brings up how to start over; it stays as long as the card.
  const [tried, setTried] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => form.current?.querySelector<HTMLElement>("[data-autofocus]")?.focus(), []);

  const fail = (field: boolean, text: string) => {
    setProblem({ field, text });
    onFail();
    refocus(form.current);
  };

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (busy) return;
    if (!pw) return fail(true, t("vault.enterField", { label: getLang() === "en" ? label.toLowerCase() : label }));
    setBusy(true);
    try {
      await api.vaultUnlock(pw);
      // Busy until the card has gone.
      onOpen();
    } catch (err) {
      const e2 = asError(err);
      const wrong = e2.kind === "wrongPassword";
      setBusy(false);
      if (wrong) setTried(true);
      fail(wrong, wrong ? t("ui.error.wrongPassword") : friendly(e2));
    }
  }

  return (
    <form ref={form} onSubmit={submit} className="flex flex-col gap-4">
      <Head title={t("vault.title")} text={lead} />
      <PasswordField
        label={label}
        value={pw}
        onChange={(v) => {
          setPw(v);
          if (problem) {
            setProblem(null);
            onFixed();
          }
        }}
        autoComplete="current-password"
        error={problem?.field ? problem.text : undefined}
        withKey
        autoFocus
        peek={peek}
        onPeek={() => setPeek(!peek)}
      />
      {problem && !problem.field && <p role="alert" className="alert">{problem.text}</p>}
      {tried && !hasRecovery && <p className="vault-forgot well selectable">{t("vault.forgot")}</p>}
      <div className="flex flex-col gap-2">
        <Button type="submit" variant="primary" block busy={busy}>
          <Swap k={busy ? "busy" : "idle"}>{busy ? t("vault.unlocking") : t("vault.unlock")}</Swap>
        </Button>
        {hasRecovery && (
          <Button variant="ghost" size="md" className="self-center" onClick={onForgot} disabled={busy}>
            {t("vault.recovery.forgotLink")}
          </Button>
        )}
      </div>
    </form>
  );
}

/** Where a reset's problem shows: under its field, or under the form. */
type Slot = "code" | "next" | "confirm" | "form";

function RecoverForm({ onBack, onRecovered, onFail, onFixed, onOpen }: Card & { onBack: () => void; onRecovered?: (code: string) => void }) {
  const form = useRef<HTMLFormElement>(null);
  const [code, setCode] = useState("");
  const pw = useNewPassword();
  const [problem, setProblem] = useState<{ at: Slot; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => form.current?.querySelector<HTMLElement>("[data-autofocus]")?.focus(), []);

  const fail = (at: Slot, text: string) => {
    setProblem({ at, text });
    onFail();
    refocus(form.current);
  };
  const edit = (at: Slot, set: (v: string) => void) => (v: string) => {
    set(v);
    // A mismatch is about both passwords: either edit hands it to the live check.
    if (problem?.at === at || (at === "next" && problem?.at === "confirm")) {
      setProblem(null);
      onFixed();
    }
  };
  const errorAt = (at: Slot) => (problem?.at === at ? problem.text : undefined);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (busy) return;
    if (!code.trim()) return fail("code", t("vault.recovery.enterCode"));
    const bad = pw.check();
    if (bad) return fail(...bad);
    setBusy(true);
    setProblem(null);
    onFixed();
    try {
      const fresh = await api.vaultRecover(code, pw.nw);
      // At once: the old code is void now, and the card is about to go.
      if (onRecovered) onRecovered(fresh);
      else publishRecoveryCode(fresh);
      onOpen();
    } catch (err) {
      const e2 = asError(err);
      setBusy(false);
      if (e2.kind === "wrongPassword") fail("code", t("vault.recovery.wrongCode"));
      else fail("form", friendly(e2));
    }
  }

  return (
    <form
      ref={form}
      onSubmit={submit}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !busy) onBack();
      }}
      className="flex flex-col gap-4"
    >
      <Head title={t("vault.recovery.forgotTitle")} text={t("vault.recovery.forgotText")} />
      <Field label={t("vault.recovery.code")} error={errorAt("code")}>
        {({ id, describedBy, invalid }) => (
          <Input
            id={id}
            mono
            invalid={invalid}
            aria-describedby={describedBy}
            className="uppercase tracking-[0.12em]"
            value={code}
            onChange={(e) => edit("code", setCode)(e.target.value)}
            placeholder="XXX-XXX-XXX"
            autoComplete="off"
            data-autofocus
          />
        )}
      </Field>
      {pw.fields({ edit, errorAt, withKey: true })}
      {problem?.at === "form" && <p role="alert" className="alert">{problem.text}</p>}
      <div className="flex flex-col gap-2 pt-1">
        <Button type="submit" variant="primary" block busy={busy}>
          <Swap k={busy ? "busy" : "idle"}>{busy ? t("vault.working") : t("vault.recovery.forgotSubmit")}</Swap>
        </Button>
        <Button variant="ghost" size="md" className="self-center" onClick={onBack} disabled={busy}>
          {t("vault.recovery.backToPassword")}
        </Button>
      </div>
    </form>
  );
}

/** After a failed try: to the field to fix, its text selected so typing replaces it. */
function refocus(form: HTMLFormElement | null) {
  requestAnimationFrame(() => {
    const field = form?.querySelector<HTMLInputElement>("input[aria-invalid=true]");
    field?.focus();
    field?.select();
  });
}

function Head({ title, text }: { title: string; text: string }) {
  return (
    <div className="vault-head">
      <h1>{title}</h1>
      <p>{text}</p>
    </div>
  );
}

/** The lock, in 1.5 px outline. Its keyhole is the accent, red after a failed try. The shackle drops
 * shut as the card arrives and lifts when the vault opens. */
function Lock({ open, wrong }: { open: boolean; wrong: boolean }) {
  const above = useId();
  return (
    <svg className="vault-lock" data-wrong={wrong || undefined} width="40" height="40" viewBox="0 0 40 40" fill="none" stroke="currentColor" strokeWidth={1.5} strokeLinecap="round" aria-hidden="true">
      <clipPath id={above}>
        <rect width="40" height="19" />
      </clipPath>
      {/* Cut at the body's top edge, so the long leg stays inside it when the shackle lifts. */}
      <g clipPath={`url(#${above})`}>
        <motion.path
          d="M14 25V13a6 6 0 0 1 12 0v6"
          initial={{ y: -5 }}
          animate={{ y: open ? -5 : 0 }}
          transition={{ ...SPRING.snappy, delay: open ? 0 : DUR.fast / 1000 }}
        />
      </g>
      <rect x="9" y="19" width="22" height="16" rx="5" />
      <g className="vault-keyhole">
        <circle cx="20" cy="25.5" r="2.25" stroke="none" />
        <path d="M20 27v3" />
      </g>
    </svg>
  );
}
