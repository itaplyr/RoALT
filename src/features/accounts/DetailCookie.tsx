import { useEffect, useReducer, useState } from "react";
import { useIsPresent } from "motion/react";
import { api, asError, type AccountSummary, type VaultMode } from "../../api";
import { t } from "../../i18n";
import { Button } from "../../ui/Button";
import { ConfirmDialog } from "../../ui/Dialog";
import { Morph } from "../../ui/motion";
import { friendly, toast } from "../../ui/Toast";
import { PasswordDialog } from "../vault/PasswordDialog";
import { Section } from "./DetailOverview";

/** How long a revealed cookie shows; .drain (styles/motion.css) empties over the same 20 s. */
const REVEAL_MS = 20_000;
type Kind = "reveal" | "copy";

/** The account's cookie, masked. Reveal and Copy ask first (MAIN: once more), then the master password
 * or Windows Hello. A revealed cookie hides itself after 20 s, and goes at once when the tab, the
 * account or the vault (lock) changes: all of those unmount this. */
export function DetailCookie({ account: a, vaultMode, clipboardSeconds, label }: { account: AccountSummary; vaultMode: VaultMode; clipboardSeconds: number; label?: string }) {
  const [cookie, setCookie] = useState<{ value: string; until: number } | null>(null);
  // What the password dialog or the MAIN confirm is asking for (they keep showing it while they fade).
  const [ask, setAsk] = useState<Kind | null>(null);
  const [mainAsk, setMainAsk] = useState<Kind | null>(null);
  const [busy, setBusy] = useState<Kind | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [, second] = useReducer((n: number) => n + 1, 0);
  // Another account picked: this panel fades out, and lets go of the cookie before it does.
  const present = useIsPresent();
  if (!present && cookie) setCookie(null);

  useEffect(() => {
    if (!cookie) return;
    const hide = setTimeout(() => setCookie(null), cookie.until - Date.now());
    const count = setInterval(second, 1000);
    return () => {
      clearTimeout(hide);
      clearInterval(count);
    };
  }, [cookie]);

  const run = async (kind: Kind, password?: string) => {
    if (kind === "reveal") {
      const value = await api.accountRevealCookie(a.id, password);
      setCookie({ value, until: Date.now() + REVEAL_MS });
    } else {
      await api.accountCopyCookie(a.id, password);
      toast(t("accounts.detail.cookieCopied", { count: clipboardSeconds }));
    }
  };
  const start = (kind: Kind, confirmed = false) => {
    setError(null);
    if (a.main && !confirmed) setMainAsk(kind);
    else if (vaultMode === "master") setAsk(kind);
    else {
      // Windows Hello: the backend asks. A dismissed prompt stays quiet.
      setBusy(kind);
      run(kind)
        .catch((e) => {
          const err = asError(e);
          if (err.kind !== "cancelled") setError(friendly(err));
        })
        .finally(() => setBusy(null));
    }
  };
  const left = cookie ? Math.max(0, Math.ceil((cookie.until - Date.now()) / 1000)) : 0;

  return (
    <Section label={label}>
      <p className="meta">{a.status === "invalid" ? t("accounts.detail.cookieExpired") : t("accounts.detail.cookieEncrypted")}</p>
      <Morph axis="height">
        {cookie ? (
          <div key="shown" className="enter well overflow-hidden">
            <div className="mono text-[12px] leading-snug p-3 max-h-36 overflow-auto break-all selectable scroll">{cookie.value}</div>
            <div className="h-0.5 bg-surface-3">
              <div className="h-full bg-accent-text drain" />
            </div>
          </div>
        ) : (
          <div key="masked" aria-hidden="true" className="enter field mono flex items-center overflow-hidden text-fg-3 tracking-[0.14em]">
            ••••••••••••••••••••••••
          </div>
        )}
      </Morph>
      <div className="grid grid-cols-2 gap-2">
        <Button icon={cookie ? "eyeOff" : "eye"} busy={busy === "reveal"} onClick={() => (cookie ? setCookie(null) : start("reveal"))}>
          {cookie ? t("accounts.detail.hide") : t("accounts.detail.reveal")}
        </Button>
        <Button icon="copy" busy={busy === "copy"} onClick={() => start("copy")}>
          {t("common.copy")}
        </Button>
      </div>
      <p className="meta">
        {cookie
          ? t("accounts.detail.cookieHides", { count: left })
          : vaultMode === "master"
            ? t("accounts.detail.cookieAskMaster")
            : t("accounts.detail.cookieAskHello")}
      </p>
      {error && (
        <p role="alert" className="alert">
          {error}
        </p>
      )}

      <PasswordDialog
        open={ask !== null}
        onClose={() => setAsk(null)}
        title={ask === "copy" ? t("accounts.detail.copyTitle") : t("accounts.detail.revealTitle")}
        text={ask === "copy" ? t("accounts.detail.copyText", { name: a.username }) : t("accounts.detail.revealText", { name: a.username })}
        submitLabel={ask === "copy" ? t("common.copy") : t("accounts.detail.reveal")}
        current
        onSubmit={({ current }) => run(ask ?? "reveal", current)}
      />
      <ConfirmDialog
        open={mainAsk !== null}
        onClose={() => setMainAsk(null)}
        onConfirm={() => {
          setMainAsk(null);
          if (mainAsk) start(mainAsk, true);
        }}
        title={t("accounts.detail.mainCookieTitle")}
        text={t("accounts.detail.mainCookieText", { name: a.username })}
        confirmLabel={mainAsk === "copy" ? t("common.copy") : t("accounts.detail.reveal")}
      />
    </Section>
  );
}
