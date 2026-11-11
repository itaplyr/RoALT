import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, asError, type AccountSummary, type JoinCandidate, type PresenceKind } from "../../api";
import { qk } from "../../data";
import { t } from "../../i18n";
import { Skeleton } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { Input } from "../../ui/Field";
import { riseIn, STAGGER } from "../../ui/motion";
import { friendly } from "../../ui/Toast";
import { accountName, Avatar, PresenceStatus } from "../accounts/shared";
import "./launch.css";

const RANK: Record<PresenceKind, number> = { inGame: 0, studio: 1, online: 2, unknown: 3, offline: 4 };

/** Friends online plus the other stored accounts, as `account` (the one that asks Roblox) sees them;
 * asked each time it opens, never polled. Without a signed-in account there is nobody to ask. */
export function JoinDialog({ open, onClose, account, accounts, onPick }: {
  open: boolean;
  onClose: () => void;
  account: AccountSummary | null;
  accounts: AccountSummary[];
  onPick: (c: JoinCandidate) => void;
}) {
  const id = account?.id ?? 0;
  const list = useQuery({
    queryKey: [...qk.join, id],
    queryFn: () => api.joinCandidates(id),
    enabled: open && account !== null,
    staleTime: 0,
  });
  const [q, setQ] = useState("");
  useEffect(() => {
    if (open) setQ("");
  }, [open]);

  const needle = q.trim().toLowerCase();
  const ready = !!list.data && !list.isFetching;
  const shown = (list.data ?? [])
    .filter((c) => !needle || `${c.displayName}\n${c.username}`.toLowerCase().includes(needle))
    .sort((a, b) => RANK[a.presence.kind] - RANK[b.presence.kind]);

  // The people rise in as they arrive (a long list just appears); filtering doesn't replay it.
  const rows = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const els = rows.current?.children;
    if (ready && els && els.length <= STAGGER.listLimit) [...els].forEach((el, i) => riseIn(el, i));
  }, [ready, list.dataUpdatedAt]);

  return (
    <Dialog open={open} onClose={onClose} title={account ? t("launch.join.titleAs", { name: accountName(account) }) : t("launch.joinSomeone")} width={480}>
      <div className="px-[22px] pb-3">
        <Input
          leading="search"
          aria-label={t("launch.join.filterPeople")}
          placeholder={t("launch.join.filterByName")}
          autoComplete="off"
          disabled={!account}
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
      </div>
      <div className="scroll max-h-[min(420px,60vh)] px-3 pb-2">
        {!account ? (
          <p className="px-2.5 py-4 text-fg-2">{t("servers.noValidAccount")}</p>
        ) : list.isError ? (
          <div className="flex flex-col items-start gap-3 px-2.5 py-4">
            <p role="alert" className="alert">{friendly(asError(list.error))}</p>
            <Button size="md" icon="refresh" busy={list.isFetching} disabled={list.isFetching} onClick={() => void list.refetch()}>
              {t("common.tryAgain")}
            </Button>
          </div>
        ) : !ready ? (
          <div aria-busy="true" aria-label={t("common.loading")}>
            {[160, 120, 140].map((w) => (
              <div key={w} className="flex items-center gap-3 min-h-[52px] px-2.5">
                <Skeleton width={28} height={28} radius={999} />
                <div className="flex flex-col gap-1.5">
                  <Skeleton width={w} height={12} />
                  <Skeleton width={w * 0.6} height={10} />
                </div>
              </div>
            ))}
          </div>
        ) : shown.length === 0 ? (
          <p className="px-2.5 py-4 text-fg-2">{needle ? t("launch.join.nobodyMatches") : t("launch.join.nobody")}</p>
        ) : (
          <div ref={rows} className="flex flex-col">
            {shown.map((c) => (
              <Row key={c.userId} c={c} account={accounts.find((a) => a.id === c.accountId)} onPick={() => onPick(c)} />
            ))}
          </div>
        )}
      </div>
      <p className="meta px-[22px] py-3 border-t border-line text-pretty">{t("launch.join.footer")}</p>
    </Dialog>
  );
}

function Row({ c, account, onPick }: { c: JoinCandidate; account?: AccountSummary; onPick: () => void }) {
  return (
    <button type="button" className="launch-person" onClick={onPick}>
      {account ? (
        <Avatar account={account} size={28} />
      ) : (
        <span aria-hidden="true" className="launch-initials">
          {c.displayName.replace(/[^\p{L}\p{N}]/gu, "").slice(0, 2).toUpperCase() || "?"}
        </span>
      )}
      <span className="flex flex-col min-w-0 flex-1 leading-tight">
        <span className="font-semibold truncate">{c.displayName}</span>
        <span className="mono text-xs text-fg-3 truncate">@{c.username}</span>
      </span>
      {account && <span className="tag">{t("launch.join.yourAccount")}</span>}
      <span className="flex justify-end min-w-0 max-w-[40%] text-xs">
        <PresenceStatus presence={c.presence} />
      </span>
    </button>
  );
}
