import { useEffect, useId, useReducer, useRef, type DialogHTMLAttributes, type ReactNode, type RefObject } from "react";
import { AnimatePresence, motion } from "motion/react";
import { IconButton } from "./Button";
import { isRtl, t } from "../i18n";
import { DUR, Morph, SPRING, Swap, TWEEN, useReducedMotionFlag } from "./motion";

/** How long .dlg takes to fade out (overlays.css, --dur-dialog-exit), and a little. */
const FADE_MS = DUR.dialogExit + 90;

/** A native modal <dialog> (Dialog, Drawer): showModal and close follow `open`; Esc and a click on the
 * backdrop call onClose. `opened` runs right after showModal, to place the focus. Spread the result
 * on the <dialog>. */
export function useModal(open: boolean, onClose: () => void, opened: (d: HTMLDialogElement) => void): DialogHTMLAttributes<HTMLDialogElement> & { ref: RefObject<HTMLDialogElement | null> } {
  const ref = useRef<HTMLDialogElement>(null);
  const downOnBackdrop = useRef(false);
  const openedAt = useRef(0);
  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) {
      d.showModal();
      openedAt.current = Date.now();
      opened(d);
    }
    if (!open && d.open) d.close();
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps
  return {
    ref,
    // React passes a nested dialog's cancel and close up the tree: only this one's own count.
    onCancel: (e) => {
      if (e.target !== e.currentTarget) return;
      e.preventDefault();
      onClose();
    },
    onPointerDown: (e) => (downOnBackdrop.current = e.target === e.currentTarget),
    // A press on the backdrop let go inside is no click on the backdrop.
    onPointerUp: (e) => (downOnBackdrop.current &&= e.target === e.currentTarget),
    onClick: (e) => {
      // Not the second click of the double-click that opened it.
      if (downOnBackdrop.current && e.target === e.currentTarget && Date.now() - openedAt.current > 400) onClose();
    },
    // Should the browser close it anyway while its owner keeps it open (a second Esc closes a dialog
    // natively, past onCancel): back, and ask the owner.
    onClose: (e) => {
      if (e.target !== e.currentTarget) return;
      if (open && !e.currentTarget.open) {
        e.currentTarget.showModal();
        onClose();
      }
    },
  };
}

/**
 * Native modal <dialog>. Keep it mounted and drive `open`, so CSS can animate it out.
 * Esc and a click on the backdrop call onClose. Focus goes to [data-autofocus] or the first text
 * field, else to the close button: a checkbox isn't a place to start (Space would change it).
 */
export function Dialog({ open, onClose, title, width = 440, describedBy, children }: {
  open: boolean;
  onClose: () => void;
  title: string;
  width?: number;
  describedBy?: string;
  children: ReactNode;
}) {
  const titleId = useId();
  // While it fades out it keeps showing what it showed, whatever its owner has moved on to; then
  // that is let go, so a closed dialog holds on to no typed cookie.
  const kept = useRef<{ title: string; children: ReactNode } | null>(null);
  const [, letGo] = useReducer((n: number) => n + 1, 0);
  if (open) kept.current = { title, children };
  useEffect(() => {
    if (open) return;
    const timer = setTimeout(() => {
      kept.current = null;
      letGo();
    }, FADE_MS);
    return () => clearTimeout(timer);
  }, [open]);
  // Closed and faded, it renders nothing: a closed dialog can hold a thousand rows otherwise.
  const shown = kept.current ?? { title, children: null };
  // Each opening starts the title afresh: it cross-fades between steps, never from the title the
  // owner had while closed (built from empty values).
  const opens = useRef(0);
  const wasOpen = useRef(open);
  if (open && !wasOpen.current) opens.current++;
  wasOpen.current = open;

  const modal = useModal(open, onClose, (d) => {
    // showModal would focus the header's close button; prefer the marked or first field.
    const first = d.querySelector<HTMLElement>("[data-autofocus]") ?? d.querySelector<HTMLElement>("input:not([disabled]):not([type=checkbox]):not([type=radio]), textarea");
    first?.focus();
  });

  return (
    <dialog
      {...modal}
      className="dlg"
      style={{ width }}
      aria-labelledby={titleId}
      aria-describedby={describedBy}
      data-tauri-drag-region="false"
      // Esc is handled here, so the browser never force-closes the dialog (a second Esc skips
      // `cancel`, and re-showing it would bury a dialog opened over it). Not while a menu inside
      // has Esc, nor for a dialog nested in this one.
      onKeyDown={(e) => {
        if (e.key !== "Escape" || e.defaultPrevented) return;
        const d = e.currentTarget;
        if ((e.target as Element).closest("dialog") !== d || d.querySelector("[popover]:popover-open:not([data-passive])")) return;
        e.preventDefault();
        onClose();
      }}
    >
      <div className="dlg-head">
        {/* The title changes with the step of a flow: it cross-fades as the body slides. */}
        <h2 id={titleId} className="dlg-title relative">
          <Swap key={opens.current} k={shown.title}>{shown.title}</Swap>
        </h2>
        <IconButton icon="x" label={t("common.close")} onClick={onClose} />
      </div>
      {shown.children}
    </dialog>
  );
}

/** Steps of one flow inside one dialog (confirm, password, result): the dialog springs to each
 * step's height while the step slides in from the side it comes from. Focus moves to the new step's
 * [data-autofocus]. `direction`: 1 forward, -1 back. */
export function DialogSteps({ step, direction = 1, children }: { step: string | number; direction?: 1 | -1; children: ReactNode }) {
  const reduced = useReducedMotionFlag();
  const ref = useRef<HTMLDivElement>(null);
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    // The arriving step only: under popLayout the leaving one is still in the DOM, before it.
    const id = requestAnimationFrame(() => ref.current?.lastElementChild?.querySelector<HTMLElement>("[data-autofocus]")?.focus());
    return () => cancelAnimationFrame(id);
  }, [step]);
  const x = reduced ? 0 : 18 * direction * (isRtl() ? -1 : 1);
  return (
    <Morph axis="height" transition={SPRING.size}>
      <div ref={ref} style={{ position: "relative" }}>
        <AnimatePresence mode="popLayout" initial={false}>
          <motion.div
            key={step}
            initial={{ opacity: 0, x }}
            animate={{ opacity: 1, x: 0, transition: SPRING.gentle }}
            exit={{ opacity: 0, x: -x, transition: TWEEN.exit }}
          >
            {children}
          </motion.div>
        </AnimatePresence>
      </div>
    </Morph>
  );
}

export function ConfirmDialog({ open, onClose, onConfirm, title, text, confirmLabel, busy, safe }: {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
  title: string;
  text: ReactNode;
  confirmLabel: string;
  busy?: boolean;
  /** Not destructive: the confirm button is the primary one, not the red one. */
  safe?: boolean;
}) {
  const textId = useId();
  return (
    <Dialog open={open} onClose={onClose} title={title} width={420} describedBy={textId}>
      <div id={textId} className="dlg-body text-fg-2">{text}</div>
      <div className="dlg-foot">
        <button type="button" className="btn" onClick={onClose} data-autofocus>{t("common.cancel")}</button>
        {/* Not `disabled` while busy: that would drop the focus of whoever pressed it. */}
        <button type="button" className={`btn ${safe ? "btn-primary" : "btn-danger"}`} onClick={() => !busy && onConfirm()} aria-disabled={busy || undefined}>
          {confirmLabel}
        </button>
      </div>
    </Dialog>
  );
}
