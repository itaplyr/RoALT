import { useState, type ReactNode } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, type AccountStatus, type AccountSummary, type Presence, type PresenceKind } from "../../api";
import { qk, setAccounts, useHeadshot } from "../../data";
import { fmtDate, t, tx, type Key } from "../../i18n";
import { ConfirmDialog } from "../../ui/Dialog";
import { TONE_COLOR, type Shape, type Tone } from "../../ui/Status";
import { toast } from "../../ui/Toast";

/** The "MAIN" tag beside the user's own account: identity, not status, so no hue. */
export function MainBadge() {
  return (
    <span className="tag flex-none text-[12px] font-semibold text-fg bg-transparent" style={{ boxShadow: "inset 0 0 0 1px var(--edge)" }}>
      {t("accounts.mainBadge")}
    </span>
  );
}

/** Colours offered for accounts and groups (from the design canvas). */
export const SWATCHES = ["#C6F432", "#7AB0FF", "#5FE0C8", "#FFA05C", "#B49CFF", "#FF8FC7", "#ECEEF1", "#5E6672"];

export const accountName = (a: AccountSummary) => a.alias || a.displayName || a.username;

/** Removing accounts: a confirm, then a second, explicit one when the MAIN is among them. Render
 * `dialogs`. `onRemoved` runs with the ids once they are gone, before the dialogs close. */
export function useRemoveAccounts(accounts: AccountSummary[], onRemoved?: (ids: number[]) => void) {
  const qc = useQueryClient();
  const [ids, setIds] = useState<number[]>([]);
  const [step, setStep] = useState<"ask" | "main" | null>(null);
  const remove = useMutation({
    mutationFn: (ids: number[]) => api.accountsRemove(ids),
    onSuccess: (_, ids) => {
      onRemoved?.(ids);
      setAccounts(qc, (list) => list.filter((a) => !ids.includes(a.id)));
      // Ids can be reused by the next account added: drop what belonged to these.
      for (const id of ids) qc.removeQueries({ queryKey: qk.info(id) });
      setStep(null);
      toast(t("accounts.toast.removed", { count: ids.length }));
    },
  });
  const main = accounts.find((a) => a.main && ids.includes(a.id));
  const one = ids.length === 1 ? accounts.find((a) => a.id === ids[0]) : undefined;
  const dialogs = (
    <>
      <ConfirmDialog
        open={step === "ask"}
        onClose={() => setStep(null)}
        // The MAIN goes only after a second, explicit confirm.
        onConfirm={() => (main ? setStep("main") : remove.mutate(ids))}
        busy={remove.isPending}
        title={t("accounts.removeTitle", { count: ids.length })}
        text={
          <>
            {ids.length === 1
              ? tx("accounts.removeOneText", { name: <span className="text-fg font-medium">{one ? accountName(one) : t("accounts.thisAccount")}</span> })
              : t("accounts.removeManyText", { count: ids.length })}
            {main && <p className="alert mt-2">{t("accounts.removeMainWarning", { name: `@${main.username}` })}</p>}
          </>
        }
        confirmLabel={t("common.remove")}
      />
      <ConfirmDialog
        open={step === "main"}
        onClose={() => setStep(null)}
        onConfirm={() => remove.mutate(ids)}
        busy={remove.isPending}
        title={t("accounts.removeMainTitle")}
        text={t("accounts.removeMainConfirm", { name: `@${main?.username ?? ""}` })}
        confirmLabel={t("common.remove")}
      />
    </>
  );
  const ask = (next: number[]) => {
    setIds(next);
    setStep("ask");
  };
  return { ask, dialogs, open: step !== null };
}

export function initials(a: AccountSummary) {
  return accountName(a).replace(/[^\p{L}\p{N}]/gu, "").slice(0, 2).toUpperCase() || "?";
}

/** Accounts without a colour get a stable one from their user id. */
export const avatarColor = (a: AccountSummary) => a.color ?? SWATCHES[Math.abs(a.userId) % 6];

/** Dark ink on light colours, white on dark ones (custom #rrggbb can be anything). */
function inkOn(hex: string) {
  const n = parseInt(hex.slice(1), 16);
  const lum = 0.2126 * ((n >> 16) & 255) + 0.7152 * ((n >> 8) & 255) + 0.0722 * (n & 255);
  return lum > 110 ? "#0A0B0D" : "#FFFFFF";
}

/** The Roblox headshot over the account colour; the initials show until it loads, or if it fails. */
export function Avatar({ account, size, url }: { account: AccountSummary; size: number; url?: string | null }) {
  const src = useHeadshot(account.id) ?? url ?? null;
  const [loaded, setLoaded] = useState<string | null>(null);
  const [broken, setBroken] = useState<string | null>(null);
  const bg = avatarColor(account);
  const show = src !== null && src !== broken;
  const done = show && loaded === src;
  return (
    <span
      aria-hidden="true"
      className="relative flex-none grid place-content-center font-bold overflow-hidden rounded-full"
      // The colour backs the initials only: a headshot has a transparent background of its own.
      style={{ width: size, height: size, background: done ? "transparent" : bg, color: inkOn(bg), fontSize: Math.round(size * 0.36) }}
    >
      <span className={done ? "opacity-0" : undefined}>{initials(account)}</span>
      {show && (
        <img
          key={src}
          src={src}
          alt=""
          loading="lazy"
          decoding="async"
          className={`absolute inset-0 w-full h-full object-cover ${done ? "opacity-100" : "opacity-0"}`}
          onLoad={() => setLoaded(src)}
          onError={() => setBroken(src)}
        />
      )}
    </span>
  );
}

/** Status is told apart by shape and word, never by colour alone. Status hues only: a user's
 * accent must not repaint "valid" or "in game". */
type Look = { label: Key; tone: Tone; shape: Shape; text: string };

const LOOK: Record<AccountStatus, Look> = {
  valid: { label: "accounts.status.valid", tone: "ok", shape: "dot", text: "var(--fg)" },
  invalid: { label: "accounts.status.invalid", tone: "attention", shape: "diamond", text: "var(--attention)" },
  challenged: { label: "accounts.status.challenged", tone: "attention", shape: "triangle", text: "var(--attention)" },
  unknown: { label: "accounts.status.unknown", tone: "neutral", shape: "ring", text: "var(--fg-2)" },
};

const PRESENCE: Record<PresenceKind, Look> = {
  inGame: { label: "accounts.presence.inGame", tone: "ok", shape: "dot", text: "var(--fg)" },
  online: { label: "accounts.presence.online", tone: "info", shape: "dot", text: "var(--fg)" },
  studio: { label: "accounts.presence.studio", tone: "info", shape: "ring", text: "var(--fg)" },
  offline: { label: "accounts.presence.offline", tone: "neutral", shape: "ring", text: "var(--fg-2)" },
  unknown: { label: "accounts.presence.unknown", tone: "neutral", shape: "ring", text: "var(--fg-2)" },
};

function Mark({ look, label, detail, pulse }: { look: Look; label: string; detail?: string; pulse?: boolean }) {
  return (
    <span className="inline-flex items-center gap-2 min-w-0 max-w-full" style={{ color: look.text }}>
      <span aria-hidden="true" className={`status-mark acc-mark ${pulse ? "pulse" : ""}`} data-shape={look.shape} style={{ color: TONE_COLOR[look.tone] }} />
      <span className="truncate flex-none max-w-[calc(100%-15px)]" title={label}>{label}</span>
      {detail && <span className="mono text-xs text-fg-3 truncate">{detail}</span>}
    </span>
  );
}

/** Swaps in what it shows when `k` changes after the first render (a check's result arriving),
 * not on mount: a long list doesn't play it for every row. */
function Swapped({ k, children }: { k: string; children: ReactNode }) {
  const [seen, setSeen] = useState({ k, moved: false });
  if (seen.k !== k) setSeen({ k, moved: true });
  return (
    <span key={k} className={`inline-flex min-w-0 max-w-full ${seen.moved ? "acc-swap" : ""}`}>
      {children}
    </span>
  );
}

/** "In game · Sky Islands" where Roblox names the game. */
export function PresenceStatus({ presence: p }: { presence: Presence }) {
  const label = p.kind === "inGame" && p.lastLocation ? t("accounts.presence.inGameAt", { place: p.lastLocation }) : t(PRESENCE[p.kind].label);
  return <Mark look={PRESENCE[p.kind]} label={label} />;
}

const CHECKING: Look = { label: "accounts.status.checking", tone: "neutral", shape: "ring", text: "var(--fg-3)" };

/** Cookie status; a valid account shows its presence instead once that is known. `swap`: a change
 * pops in; only where few show at once (a check of 300 rows would pop 300). */
export function Status({ status, presence, checking, detail, swap }: {
  status: AccountStatus;
  presence?: Presence;
  checking?: boolean;
  detail?: string;
  swap?: boolean;
}) {
  const p = status === "valid" && presence && presence.kind !== "unknown" ? presence : null;
  const mark = checking ? (
    // Long lists (swap false) get a still ring: hundreds of endless pulses cost every frame.
    <Mark look={CHECKING} label={t(CHECKING.label)} pulse={swap !== false} />
  ) : p ? (
    <PresenceStatus presence={p} />
  ) : (
    <Mark look={LOOK[status]} label={t(LOOK[status].label)} detail={detail} />
  );
  return swap ? <Swapped k={checking ? "checking" : p ? `${p.kind}:${p.lastLocation}` : status}>{mark}</Swapped> : mark;
}

export function ago(ms: number | null, now = Date.now()) {
  if (ms === null) return t("common.never");
  const s = Math.max(0, (now - ms) / 1000);
  if (s < 60) return t("accounts.ago.justNow");
  if (s < 3600) return t("accounts.ago.minutes", { count: Math.floor(s / 60) });
  if (s < 86400) return t("accounts.ago.hours", { count: Math.floor(s / 3600) });
  return t("accounts.ago.days", { count: Math.floor(s / 86400) });
}

export const formatDate = (ms: number) => fmtDate(ms, { day: "numeric", month: "short", year: "numeric" });

/** "7 y 7 m" since an ISO date; null when the date doesn't parse. */
export function accountAge(created: string, now = new Date()) {
  const c = new Date(created);
  if (Number.isNaN(c.getTime())) return null;
  const m = Math.max(
    0,
    (now.getFullYear() - c.getFullYear()) * 12 + now.getMonth() - c.getMonth() - (now.getDate() < c.getDate() ? 1 : 0),
  );
  return t("accounts.age", { y: Math.floor(m / 12), m: m % 12 });
}
