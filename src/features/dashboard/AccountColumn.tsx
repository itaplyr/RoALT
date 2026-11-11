import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import "../accounts/accounts.css";
import { api, type AccountSummary, type Instance } from "../../api";
import { qk, setAccounts, useAccountInfo, useMetrics, useSettings, useSystemUsage, useVault } from "../../data";
import { t, tx } from "../../i18n";
import { Skeleton } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { ConfirmDialog } from "../../ui/Dialog";
import { Meter } from "../../ui/Meter";
import { Num } from "../../ui/motion";
import { toast } from "../../ui/Toast";
import { DetailCookie } from "../accounts/DetailCookie";
import { PresenceBlock, RecentServers, Section } from "../accounts/DetailOverview";
import { accountName, ago, Status } from "../accounts/shared";
import { mb, pct } from "../monitor/MonitorView";
import { LimitsDialog } from "./LimitsDialog";

/** The account tab's second column: how the account stands, what its client uses, where it played,
 * and the account's own tools. `inst`: its client, while one runs. */
export function AccountColumn({ account: a, inst }: { account: AccountSummary; inst?: Instance }) {
  const qc = useQueryClient();
  const info = useAccountInfo(a.id);
  // Only info that belongs to this Roblox user (local ids can be reused after a removal).
  const d = info.data?.userId === a.userId ? info.data : undefined;
  const vault = useVault().data;
  const settings = useSettings().data;
  const [removing, setRemoving] = useState<"ask" | "main" | null>(null);

  const validate = useMutation({
    mutationFn: async () => ({ started: Date.now(), list: await api.accountsValidate([a.id]) }),
    onSuccess: ({ started, list }) => {
      qc.setQueryData(qk.accounts, list);
      // Roblox couldn't be asked (network, rate limit): the old stamp stays.
      const done = (list.find((x) => x.id === a.id)?.statusCheckedAt ?? 0) >= started - 1000;
      toast(done ? t("accounts.toast.checked", { count: 1 }) : t("accounts.toast.missed", { count: 1 }));
    },
  });
  const session = useMutation({
    mutationFn: () => api.accountOpenSession(a.id),
    onSuccess: () => toast(a.status === "valid" ? t("accounts.detail.sessionOpenedValid") : t("accounts.detail.sessionOpenedOther")),
  });
  const remove = useMutation({
    mutationFn: () => api.accountsRemove([a.id]),
    onSuccess: () => {
      setRemoving(null);
      // Home drops the tab once the account is gone from the list.
      setAccounts(qc, (list) => list.filter((x) => x.id !== a.id));
      qc.removeQueries({ queryKey: qk.info(a.id) });
      toast(t("accounts.toast.removed", { count: 1 }));
    },
  });
  const checking = validate.isPending;

  return (
    <aside className="panel acct-col" aria-label={t("home.acct.label")}>
      <Section label={t("home.acct.status")}>
        <div className="flex items-center justify-between gap-3 min-h-7">
          <span className="flex items-center gap-2 min-w-0">
            <span className="flex-none">
              <Status status={a.status} checking={checking} swap />
            </span>
            <span className="meta truncate">
              {a.statusCheckedAt ? t("accounts.detail.checked", { when: ago(a.statusCheckedAt) }) : t("accounts.notValidated")}
            </span>
          </span>
          <Button size="sm" variant="ghost" icon="refresh" busy={checking} onClick={() => !checking && validate.mutate()}>
            {t("accounts.menu.validate")}
          </Button>
        </div>
        {(a.status === "invalid" || a.status === "challenged") && (
          <Button icon="signin" block busy={session.isPending} onClick={() => session.mutate()}>
            {t("accounts.detail.signInAgain")}
          </Button>
        )}
        {d ? <PresenceBlock p={d.presence} /> : !info.isError && <Skeleton width={160} height={18} />}
      </Section>

      <Performance account={a} inst={inst} />
      <RecentServers account={a} playing={!!inst} />

      <Section label={t("home.acct.tools")}>
        <div className="grid grid-cols-2 gap-2">
          <Button icon="external" busy={session.isPending} onClick={() => session.mutate()}>
            {t("accounts.menu.openSession")}
          </Button>
          <Button variant="danger" icon="trash" onClick={() => setRemoving("ask")}>
            {t("accounts.detail.removeEllipsis")}
          </Button>
        </div>
      </Section>
      {vault && settings && <DetailCookie account={a} vaultMode={vault.mode} clipboardSeconds={settings.clipboardClearSeconds} label={t("accounts.detail.cookie")} />}

      <ConfirmDialog
        open={removing === "ask"}
        onClose={() => setRemoving(null)}
        // The MAIN goes only after a second, explicit confirm.
        onConfirm={() => (a.main ? setRemoving("main") : remove.mutate())}
        busy={remove.isPending}
        title={t("accounts.removeTitle", { count: 1 })}
        text={
          <>
            {tx("accounts.removeOneText", { name: <span className="text-fg font-medium">{accountName(a)}</span> })}
            {a.main && <p className="alert mt-2">{t("accounts.removeMainWarning", { name: `@${a.username}` })}</p>}
          </>
        }
        confirmLabel={t("common.remove")}
      />
      <ConfirmDialog
        open={removing === "main"}
        onClose={() => setRemoving(null)}
        onConfirm={() => remove.mutate()}
        busy={remove.isPending}
        title={t("accounts.removeMainTitle")}
        text={t("accounts.removeMainConfirm", { name: `@${a.username}` })}
        confirmLabel={t("common.remove")}
      />
    </aside>
  );
}

/** What its client uses now, with its limits a click away; without a client, a word on when. */
function Performance({ account: a, inst }: { account: AccountSummary; inst?: Instance }) {
  const m = useMetrics().data?.find((x) => x.instanceId === inst?.id);
  const sys = useSystemUsage(!!inst).data;
  const [limits, setLimits] = useState(false);
  if (!inst) {
    return (
      <Section label={t("home.acct.performance")}>
        <p className="meta">{t("home.acct.notRunning")}</p>
      </Section>
    );
  }
  const memShare = m && sys ? (m.memoryMb / Math.max(1, sys.memTotalMb)) * 100 : null;
  // Label, the bar's share of the PC, the number shown and how.
  const rows: [string, number | null, number | null, (n: number) => string][] = [
    [t("home.cpu"), m?.cpuPct ?? null, m?.cpuPct ?? null, pct],
    [t("home.memory"), memShare, m?.memoryMb ?? null, mb],
    [t("home.gpu"), m?.gpuPct ?? null, m?.gpuPct ?? null, pct],
  ];
  return (
    <Section label={t("home.acct.performance")}>
      <dl className="acct-meters">
        {rows.map(([label, share, value, format]) => (
          <div key={label} className="contents">
            <dt className="text-fg-2">{label}</dt>
            <dd className="contents">
              <Meter value={share} label={label} />
              <span className="mono text-[13px] text-end">
                <Num value={value} format={format} />
              </span>
            </dd>
          </div>
        ))}
      </dl>
      <Button size="md" icon="sliders" className="self-start" onClick={() => setLimits(true)}>
        {t("home.limits")}
      </Button>
      <LimitsDialog instanceId={limits ? inst.id : null} name={accountName(a)} onClose={() => setLimits(false)} />
    </Section>
  );
}
