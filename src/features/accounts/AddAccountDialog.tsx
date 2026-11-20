import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { api, asError, type AccountSummary, type ImportReport } from "../../api";
import { qk } from "../../data";
import { fmtList, t, tx } from "../../i18n";
import { Button } from "../../ui/Button";
import { Segmented } from "../../ui/controls";
import { Dialog, DialogSteps } from "../../ui/Dialog";
import { Morph } from "../../ui/motion";
import { friendly, toast } from "../../ui/Toast";

export type Tab = "login" | "cookie" | "many";

/** The start of every .ROBLOSECURITY value: lines holding one are cookie imports. */
const COOKIE_MARK = "_|WARNING:-DO-NOT-SHARE-THIS.";
type Login = { username: string; password: string };

/** The `user:pass` lines of pasted text (pure): split at the first ":" (a password may hold more),
 * both parts trimmed, cookie lines and lines without both parts left out, each username once. */
export function parseLogins(text: string): Login[] {
  const seen = new Set<string>();
  const out: Login[] = [];
  for (const line of text.split(/\r?\n/)) {
    const at = line.indexOf(":");
    if (line.includes(COOKIE_MARK) || at < 0) continue;
    const username = line.slice(0, at).trim();
    // Trimmed: stray spaces around pasted lines are far likelier than a password's own.
    const password = line.slice(at + 1).trim();
    if (!username || !password || seen.has(username.toLowerCase())) continue;
    seen.add(username.toLowerCase());
    out.push({ username, password });
  }
  return out;
}

const TABS: Tab[] = ["login", "cookie", "many"];
const RAM_FILTER = [{ name: "Roblox Account Manager", extensions: ["json"] }];

/** One step of a DialogSteps flow. It puts the focus on its own [data-first] control (else its first
 * field) as it arrives. Two frames late on purpose: DialogSteps focuses the first [data-autofocus] it
 * finds a frame after the change, and that can be in the step still leaving. */
export function Step({ className = "", children }: { className?: string; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    let id = requestAnimationFrame(() => {
      id = requestAnimationFrame(() => (el?.querySelector<HTMLElement>("[data-first]") ?? el?.querySelector<HTMLElement>("input, textarea"))?.focus());
    });
    return () => cancelAnimationFrame(id);
  }, []);
  return (
    <div ref={ref} className={`dlg-body flex flex-col gap-3 ${className}`}>
      {children}
    </div>
  );
}

export function AddAccountDialog({ open, startOn = "login", onClose, onAdded }: {
  open: boolean;
  startOn?: Tab;
  onClose: () => void;
  onAdded: (a: AccountSummary) => void;
}) {
  const qc = useQueryClient();
  const [tab, setTab] = useState<Tab>(startOn);
  const [direction, setDirection] = useState<1 | -1>(1);
  const [error, setError] = useState<string | null>(null);
  // While rendering, not in an effect: Dialog focuses a field of the panel that shows when it opens.
  const [wasOpen, setWasOpen] = useState(open);
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) {
      setTab(startOn);
      setError(null);
    }
  }
  // Closed while its request ran: the account is still added, but the app stays where it is.
  const stillOpen = useRef(open);
  stillOpen.current = open;
  const [waiting, setWaiting] = useState(false);
  const [busy, setBusy] = useState(false);
  const [cookie, setCookie] = useState("");
  const [many, setMany] = useState("");
  // The sign-in under way of a `user:pass` import; `stop` is true unless one runs (or once it should end).
  const [queue, setQueue] = useState<{ n: number; total: number; name: string } | null>(null);
  const stop = useRef(true);

  useEffect(() => {
    if (open) return;
    setCookie("");
    setMany("");
    setError(null);
    // Closed mid-import: no more sign-in windows, and the open one goes.
    if (!stop.current) {
      stop.current = true;
      void api.accountLoginCancel().catch(() => {});
    }
  }, [open]);

  const pick = (next: Tab) => {
    setDirection(TABS.indexOf(next) > TABS.indexOf(tab) ? 1 : -1);
    setTab(next);
    setError(null);
  };

  const added = (a: AccountSummary) => {
    qc.invalidateQueries({ queryKey: qk.accounts });
    qc.invalidateQueries({ queryKey: qk.vault });
    toast(t("accounts.add.added", { name: a.username }));
    if (stillOpen.current) {
      onAdded(a);
      onClose();
    }
  };

  async function signIn() {
    setError(null);
    setWaiting(true);
    try {
      added(await api.accountAddLogin());
    } catch (e) {
      const err = asError(e);
      if (err.kind !== "cancelled") setError(friendly(err));
    } finally {
      setWaiting(false);
    }
  }

  async function paste(e: FormEvent) {
    e.preventDefault();
    const value = cookie;
    setCookie(""); // the field never keeps a submitted cookie
    if (!value.trim() || busy) return;
    setError(null);
    setBusy(true);
    try {
      added(await api.accountAddCookie(value));
    } catch (e) {
      setError(friendly(asError(e)));
    } finally {
      setBusy(false);
    }
  }

  const imported = (r: ImportReport) => {
    qc.invalidateQueries({ queryKey: qk.accounts });
    qc.invalidateQueries({ queryKey: qk.vault });
    const parts = [
      t("accounts.add.importAdded", { count: r.added }),
      r.updated && t("accounts.add.importRefreshed", { count: r.updated }),
      r.skipped && t("accounts.add.importRepeated", { count: r.skipped }),
      r.failed && t("accounts.add.importFailed", { count: r.failed }),
    ].filter((p): p is string => !!p);
    const text = t("accounts.add.importResult", { list: fmtList(parts) });
    if (r.added + r.updated > 0) {
      toast(text);
      onClose();
    } else {
      // The dialog stays: the outcome shows in it, as a toast would sit under its backdrop.
      setError(text);
    }
  };

  /** Pasted lists and Roblox Account Manager files both go one cookie at a time. */
  async function runImport(run: () => Promise<ImportReport>) {
    setError(null);
    setBusy(true);
    try {
      imported(await run());
    } catch (e) {
      setError(friendly(asError(e)));
    } finally {
      setBusy(false);
    }
  }

  function pasteMany(e: FormEvent) {
    e.preventDefault();
    const value = many;
    setMany(""); // as with one cookie, the field never keeps what was submitted
    if (!value.trim() || busy) return;
    const logins = parseLogins(value);
    runImport(async () => {
      // Cookies first (also what says "nothing found" when there is neither), then the sign-ins.
      const r = value.includes(COOKIE_MARK) || !logins.length ? await api.accountsImportCookies(value) : { added: 0, updated: 0, skipped: 0, failed: 0 };
      return logins.length ? signInEach(logins, r) : r;
    });
  }

  /** One Roblox sign-in window after another, each filled in from its line; the user finishes each
   * (captcha, Log In) or closes it to skip. Stop ends it after the open one. */
  async function signInEach(logins: Login[], r: ImportReport) {
    const had = new Set((qc.getQueryData<AccountSummary[]>(qk.accounts) ?? []).map((a) => a.id));
    let refused: string | null = null;
    stop.current = false;
    try {
      for (const [i, fill] of logins.entries()) {
        if (stop.current) break;
        setQueue({ n: i + 1, total: logins.length, name: fill.username });
        try {
          const a = await api.accountAddLogin(fill);
          if (had.has(a.id)) r.updated++;
          else r.added++;
        } catch (e) {
          const err = asError(e);
          if (err.kind === "locked") throw e;
          if (err.kind !== "cancelled") {
            r.failed++;
            refused = friendly(err);
          }
        }
      }
    } finally {
      stop.current = true;
      setQueue(null);
    }
    if (refused) toast(refused, "error");
    return r;
  }

  const stopQueue = () => {
    stop.current = true;
    void api.accountLoginCancel().catch(() => {});
  };

  async function fromRam() {
    if (busy) return;
    const path = await openFile({ multiple: false, directory: false, filters: RAM_FILTER, title: t("accounts.add.ramPickerTitle") }).catch((e) => {
      setError(friendly(asError(e)));
      return null;
    });
    if (path) runImport(() => api.accountsImportRam(path));
  }

  const alert = error && (
    <p role="alert" className="alert">
      {error}
    </p>
  );
  const secret = { autoComplete: "off", autoCorrect: "off", autoCapitalize: "off", spellCheck: false } as const;

  return (
    <Dialog open={open} onClose={onClose} title={t("accounts.add.dialogTitle")} width={520}>
      <div className="dlg-body flex flex-col items-start gap-3 pb-1">
        <p className="-mt-1 text-[13px] text-fg-3">{t("accounts.add.staysHere")}</p>
        <Segmented
          label={t("accounts.add.how")}
          value={tab}
          onChange={pick}
          options={[
            { value: "login", label: t("accounts.add.tabLogin") },
            { value: "cookie", label: t("accounts.add.tabCookie") },
            { value: "many", label: t("accounts.add.tabMany") },
          ]}
        />
      </div>
      <DialogSteps step={tab} direction={direction}>
        {tab === "login" ? (
          <Step className="pt-3">
            <p className="text-fg-2">{t("accounts.add.descLogin")}</p>
            <Morph axis="height">
              {waiting ? (
                <div key="wait" role="status" className="enter flex flex-col gap-2.5 py-1">
                  <div className="live-bar" />
                  <span>{t("accounts.add.waiting")}</span>
                  <span className="text-[13px] text-fg-3">{t("accounts.add.loginPrivate")}</span>
                </div>
              ) : (
                <Button key="go" variant="primary" icon="external" block className="min-h-[52px]" data-first onClick={signIn}>
                  {t("accounts.add.openSignIn")}
                </Button>
              )}
            </Morph>
            {alert}
          </Step>
        ) : tab === "cookie" ? (
          <form onSubmit={paste}>
            <Step className="pt-3">
              <p className="text-fg-2">{t("accounts.add.descCookie")}</p>
              <label className="flex flex-col gap-1.5">
                <span className="label">{t("accounts.add.cookieLabel")}</span>
                <textarea
                  className="field secret mono text-xs"
                  rows={4}
                  value={cookie}
                  onChange={(e) => setCookie(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && !e.shiftKey && (e.preventDefault(), e.currentTarget.form?.requestSubmit())}
                  placeholder={t("accounts.add.cookiePlaceholder")}
                  {...secret}
                />
              </label>
              <p className="text-[13px] text-fg-3">{tx("accounts.add.cookieHelp", {}, { mono: (s) => <span className="mono">{s}</span> })}</p>
              {alert}
              <div className="flex justify-end gap-2 pt-1">
                <Button onClick={onClose}>{t("common.cancel")}</Button>
                <Button type="submit" variant="primary" busy={busy} disabled={!busy && !cookie.trim()}>
                  {busy ? t("accounts.status.checking") : t("accounts.add.title")}
                </Button>
              </div>
            </Step>
          </form>
        ) : (
          <form onSubmit={pasteMany}>
            <Step className="pt-3">
              <p className="text-fg-2">{t("accounts.add.descMany")}</p>
              <label className="flex flex-col gap-1.5">
                <span className="label">{t("accounts.add.manyLabel")}</span>
                <textarea
                  className="field secret mono text-xs"
                  rows={6}
                  value={many}
                  onChange={(e) => setMany(e.target.value)}
                  placeholder={t("accounts.add.manyPlaceholder")}
                  {...secret}
                />
              </label>
              <p className="text-[13px] text-fg-3">{tx("accounts.add.manyHelp", {}, { mono: (s) => <span className="mono">{s}</span> })}</p>
              {queue ? (
                <div role="status" className="flex flex-col gap-2">
                  <div className="live-bar" />
                  <span>{t("accounts.add.queue", { n: queue.n, total: queue.total, name: queue.name })}</span>
                  <span className="text-[13px] text-fg-3">{t("accounts.add.queueHint")}</span>
                  <div>
                    <Button size="sm" icon="stop" onClick={stopQueue}>{t("accounts.add.stop")}</Button>
                  </div>
                </div>
              ) : (
                busy && <div className="live-bar" role="status" aria-label={t("accounts.add.importing")} />
              )}
              {alert}
              <div className="flex flex-col items-start gap-1.5">
                <Button icon="upload" disabled={busy} aria-describedby="acc-ram-hint" onClick={fromRam}>
                  {t("accounts.add.ramImportEllipsis")}
                </Button>
                <p id="acc-ram-hint" className="text-[13px] text-fg-3">{t("accounts.add.ramHint")}</p>
              </div>
              <div className="flex justify-end gap-2 pt-1">
                <Button onClick={onClose}>{t("common.cancel")}</Button>
                <Button type="submit" variant="primary" busy={busy} disabled={!busy && !many.trim()}>
                  {busy ? t("accounts.add.importingEllipsis") : t("accounts.add.import")}
                </Button>
              </div>
            </Step>
          </form>
        )}
      </DialogSteps>
    </Dialog>
  );
}
