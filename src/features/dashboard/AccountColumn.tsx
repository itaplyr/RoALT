import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import "../accounts/accounts.css";
import { api, type AccountSummary, type Instance } from "../../api";
import { browseAs } from "../../app/navigation";
import { qk, useAccountInfo, useMetrics, useSettings, useSystemUsage, useVault } from "../../data";
import { t } from "../../i18n";
import { Skeleton } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { Meter } from "../../ui/Meter";
import { Num } from "../../ui/motion";
import { toast } from "../../ui/Toast";
import { DetailCookie } from "../accounts/DetailCookie";
import { PresenceBlock, RecentServers, Section, StatusLine } from "../accounts/DetailOverview";
import { accountName, useRemoveAccounts } from "../accounts/shared";
import { mb, pct } from "../monitor/format";
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

  const validate = useMutation({
    mutationFn: async () => ({ started: Date.now(), list: await api.accountsValidate([a.id]) }),
    onSuccess: ({ started, list }) => {
      qc.setQueryData(qk.accounts, list);
      // Roblox couldn't be asked (network, rate limit): the old stamp stays.
      const done = (list.find((x) => x.id === a.id)?.statusCheckedAt ?? 0) >= started - 1000;
      toast(done ? t("accounts.toast.checked", { count: 1 }) : t("accounts.toast.missed", { count: 1 }));
    },
  });
  // Home drops the tab once the account is gone from the list.
  const removing = useRemoveAccounts([a]);
  const checking = validate.isPending;

  return (
    <aside className="panel acct-col" aria-label={t("home.acct.label")}>
      <Section label={t("home.acct.status")}>
        <StatusLine account={a} checking={checking} onValidate={() => !checking && validate.mutate()} />
        {(a.status === "invalid" || a.status === "challenged") && (
          <Button icon="signin" block onClick={() => browseAs(a.id, "login")}>
            {t("accounts.detail.signInAgain")}
          </Button>
        )}
        {d ? <PresenceBlock p={d.presence} /> : !info.isError && <Skeleton width={160} height={18} />}
      </Section>

      <Performance account={a} inst={inst} />
      <RecentServers account={a} playing={!!inst} />

      <Section label={t("home.acct.tools")}>
        {/* Side by side while both labels fit, else one per line (a label never wraps). */}
        <div className="flex flex-wrap gap-2">
          <Button icon="globe" className="flex-[1_1_0]" onClick={() => browseAs(a.id)}>
            {t("accounts.menu.openSession")}
          </Button>
          <Button variant="danger" icon="trash" className="flex-[1_1_0]" onClick={() => removing.ask([a.id])}>
            {t("accounts.detail.removeEllipsis")}
          </Button>
        </div>
      </Section>
      {vault && settings && <DetailCookie account={a} vaultMode={vault.mode} clipboardSeconds={settings.clipboardClearSeconds} label={t("accounts.detail.cookie")} />}

      {removing.dialogs}
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
