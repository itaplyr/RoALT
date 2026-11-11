import { useEffect, useId, useRef, useState } from "react";
import type { AccountSummary } from "../../api";
import { t } from "../../i18n";
import { announce } from "../../ui/a11y";
import { Button, IconButton } from "../../ui/Button";
import { Collapse, DUR, Swap } from "../../ui/motion";
import { accountName } from "../accounts/shared";
import { ServerBrowser, type ServerChoice } from "../servers/ServerBrowser";
import { draftFor, type Draft } from "./target";
import "./launch.css";

const FIELD = "servers-game-drawer";

/** Play's server browser, in a sheet from the end side. The server picked becomes the target; a
 * private one masked, so its access code never shows in the field. `asker`: the account that asks
 * Roblox; `count`: how many accounts would join (for the room warnings). */
export function ServerDrawer({ open, onClose, accounts, asker, count, seed, onUse, onServers }: {
  open: boolean;
  onClose: () => void;
  accounts: AccountSummary[];
  asker: AccountSummary | null;
  count: number;
  /** Play's game: the drawer opens on it, else on the game it showed last. */
  seed: number | null;
  onUse: (d: Draft) => void;
  /** The whole Servers view instead. */
  onServers: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const downOnBackdrop = useRef(false);
  const openedAt = useRef(0);
  const titleId = useId();
  const summaryId = useId();
  const [place, setPlace] = useState(seed);
  const [choice, setChoice] = useState<ServerChoice | null>(null);
  const [nudge, setNudge] = useState(0);
  // The browser is there while open and while the sheet slides out; then let go (nothing stays behind).
  const [mounted, setMounted] = useState(open);
  const [was, setWas] = useState(open);
  if (open !== was) {
    setWas(open);
    if (open) {
      setMounted(true);
      if (seed !== null) setPlace(seed);
    }
  }

  useEffect(() => {
    const d = ref.current!;
    if (open && !d.open) {
      d.showModal();
      openedAt.current = Date.now();
      d.querySelector<HTMLElement>(`#${FIELD}`)?.focus();
    }
    if (!open && d.open) d.close();
  }, [open]);
  useEffect(() => {
    if (open || !mounted) return;
    const timer = setTimeout(() => {
      setMounted(false);
      setChoice(null);
    }, DUR.exit + 90);
    return () => clearTimeout(timer);
  }, [open, mounted]);

  const target = choice?.target ?? null;
  const use = () => {
    if (!target) {
      announce(t("servers.pickServerFirst"), "assertive");
      setNudge((n) => n + 1);
      return;
    }
    onUse(draftFor(target));
    onClose();
  };
  const name = asker ? accountName(asker) : "";
  const warnings = [
    choice?.restricted ? t("servers.noJoin", { name }) : null,
    target?.kind === "privateAccess" && count > 1 ? t("servers.accessCheckedFor", { name }) : null,
    choice && choice.free !== null && count > choice.free ? t("servers.onlyFreeSlots", { count: choice.free, accounts: t("servers.nAccounts", { count }) }) : null,
  ];

  return (
    <dialog
      ref={ref}
      className="launch-drawer"
      aria-labelledby={titleId}
      data-tauri-drag-region="false"
      onCancel={(e) => {
        // React passes a dialog's cancel up its tree: Manage's or Buy's Esc is theirs.
        if (e.target !== e.currentTarget) return;
        e.preventDefault();
        onClose();
      }}
      onPointerDown={(e) => (downOnBackdrop.current = e.target === e.currentTarget)}
      onPointerUp={(e) => (downOnBackdrop.current &&= e.target === e.currentTarget)}
      onClick={(e) => {
        // Not the second click of the double-click that opened it.
        if (downOnBackdrop.current && e.target === e.currentTarget && Date.now() - openedAt.current > 400) onClose();
      }}
      // A second Esc closes a dialog natively, past onCancel.
      onClose={(e) => {
        if (e.target === e.currentTarget && open && !e.currentTarget.open) {
          e.currentTarget.showModal();
          onClose();
        }
      }}
    >
      {mounted && (
        <div className="launch-drawer-body">
          <div className="dlg-head">
            <h2 id={titleId} className="dlg-title">{t("play.browse")}</h2>
            <IconButton icon="servers" label={t("app.nav.servers")} onClick={() => (onClose(), onServers())} />
            <IconButton icon="x" label={t("common.close")} onClick={onClose} />
          </div>
          <div className="launch-drawer-main">
            <ServerBrowser variant="drawer" accounts={accounts} asker={asker?.id} place={place} setPlace={setPlace} onChoice={setChoice} onSubmit={use} />
          </div>
          <div className="launch-drawer-foot">
            <div className="flex-[1_1_220px] min-w-0">
              <p id={summaryId} className={`flex min-h-7 items-center text-[13px] font-medium ${target ? "" : "text-fg-3"} ${nudge ? (nudge % 2 ? "shake" : "shake2") : ""}`}>
                <Swap k={choice?.summary ?? ""} variant="rise" className="min-w-0">
                  <span className={`truncate ${target?.kind === "server" ? "mono" : ""}`}>{choice?.summary}</span>
                </Swap>
              </p>
              <div aria-live="polite">
                {warnings.map((w, i) => (
                  <Collapse key={i} open={!!w}>
                    <p className="alert pt-1.5 text-[13px]">{w}</p>
                  </Collapse>
                ))}
              </div>
            </div>
            <Button onClick={onClose}>{t("common.cancel")}</Button>
            <Button variant="primary" aria-disabled={!target || undefined} aria-describedby={target ? undefined : summaryId} onClick={use}>
              {t("servers.useServer")}
            </Button>
          </div>
        </div>
      )}
    </dialog>
  );
}
