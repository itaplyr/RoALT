import { useLayoutEffect, useRef, type ReactNode } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { GameServer, PrivateServer } from "../../api";
import { useAccounts, useInstances } from "../../data";
import { Skeleton } from "../../ui/Bits";
import { IconButton } from "../../ui/Button";
import { riseIn, STAGGER, useFreshItems } from "../../ui/motion";
import { StatusPill } from "../../ui/Status";
import { toast, toastError } from "../../ui/Toast";
import { fmtList, fmtNum, isRtl, t } from "../../i18n";
import { accountName } from "../accounts/shared";
import { isRunning } from "../launch/Running";
import { copyRoaltLink } from "../launch/target";
import { NONE } from "../monitor/format";

export type Pick = { kind: "public"; id: string } | { kind: "private"; id: number } | null;

export const short = (id: string) => `${id.slice(0, 8)}…${id.slice(-4)}`;
const copyJob = (id: string) => writeText(id).then(() => toast(t("servers.jobCopied")), toastError);

/** Rows that arrive (the first page, the next one, a refresh) rise in 12 ms apart, capped at the
 * 12th: riseIn steps 24 ms, so half steps. A list past STAGGER.listLimit rows just shows them.
 * Returns the tbody's ref. */
function useRowRise(ids: (string | number)[]) {
  const body = useRef<HTMLTableSectionElement>(null);
  const fresh = useFreshItems(ids);
  useLayoutEffect(() => {
    if (ids.length > STAGGER.listLimit) return;
    fresh.forEach((id, i) => {
      const row = body.current?.querySelector(`[data-id="${CSS.escape(String(id))}"]`);
      if (row) riseIn(row, Math.min(i, 12) / 2);
    });
  });
  return body;
}

/** Eight placeholder rows at a real row's size while the first page loads; `cells` follow the
 * radio's column. */
const ghosts = (cells: ReactNode) =>
  Array.from({ length: 8 }, (_, i) => (
    <tr key={i} aria-hidden="true" className="row h-11 pointer-events-none">
      <td className="ps-5"><Skeleton width={18} height={18} radius={999} /></td>
      {cells}
    </tr>
  ));
const SEATS_GHOST = (
  <div className="flex items-center gap-3">
    <Skeleton height={6} radius={999} className="flex-1" />
    <Skeleton width={34} />
  </div>
);

/** Seats taken, at the true ratio; a new count springs to its share (servers.css). A clip, not a
 * width: nothing is laid out per frame, and a long list costs no script. */
function Players({ playing, max }: { playing: number; max: number }) {
  const open = 100 - Math.min(100, (playing / (max || 1)) * 100);
  return (
    <div className="flex items-center gap-3">
      <span aria-hidden="true" className="meter srv-seats flex-1">
        <span
          className="w-full"
          data-full={playing >= max || undefined}
          style={{ clipPath: isRtl() ? `inset(0 0 0 ${open}% round 3px)` : `inset(0 ${open}% 0 0 round 3px)` }}
        />
      </span>
      <span className="mono text-xs w-11 text-end">{playing}/{max}</span>
    </div>
  );
}

/** Which servers the user's own running clients are in: job id to those accounts' names. */
function useMine() {
  const instances = useInstances().data ?? [];
  const names = new Map((useAccounts().data ?? []).map((a) => [a.id, accountName(a)]));
  const mine = new Map<string, string[]>();
  for (const i of instances) {
    if (!isRunning(i) || !i.jobId) continue;
    mine.set(i.jobId, [...(mine.get(i.jobId) ?? []), names.get(i.accountId) ?? String(i.accountId)]);
  }
  return mine;
}

/** `servers` null: the first page is on its way. `fastest`: the list is in Roblox's best-ping order
 * for the asking account, so row 1 is fastest. A server one of the user's clients is in says so. */
export function PublicTable({ servers, placeId, pick, fastest, radio, onPick }: {
  servers: GameServer[] | null;
  /** The game the servers are of: their RoALT links need it. */
  placeId: number | null;
  pick: string | null;
  fastest: boolean;
  /** The radio group's name, one per browser. */
  radio: string;
  onPick: (id: string) => void;
}) {
  const body = useRowRise(servers?.map((s) => s.id) ?? []);
  const mine = useMine();
  if (servers?.length === 0) return null;
  return (
    <table className="w-full border-collapse" aria-label={t("servers.publicServers")}>
      <thead>
        <tr>
          <th className="w-12 ps-5"><span className="sr-only">{t("servers.pick")}</span></th>
          <th>{t("servers.jobId")}</th>
          <th className="w-[30%]">{t("servers.players")}</th>
          <th className="srv-opt text-end">{t("servers.friends")}</th>
          <th className="text-end" data-tip={t("servers.pingTip")}>{t("servers.ping")}</th>
          <th className="srv-opt text-end pe-4">FPS</th>
          <th className="w-20"><span className="sr-only">{t("servers.actions")}</span></th>
        </tr>
      </thead>
      <tbody ref={body}>
        {servers?.map((s, i) => {
          const id = short(s.id);
          const on = pick === s.id;
          const here = mine.get(s.id);
          return (
            <tr key={s.id} data-id={s.id} className="row h-11 cursor-pointer" data-selected={on || undefined} onClick={() => onPick(s.id)}>
              <td className="ps-5">
                <input type="radio" className="radio" name={radio} aria-label={t("servers.pickServer", { id })} checked={on} onChange={() => onPick(s.id)} />
              </td>
              <td className="whitespace-nowrap">
                <span className="mono text-xs">{id}</span>
                {fastest && i === 0 && <span className="tag ms-2">{t("servers.fastest")}</span>}
                {here && (
                  <span className="tag ms-2" data-tip={fmtList(here)}>
                    {t("servers.yours")}{here.length > 1 && ` ×${fmtNum(here.length)}`}
                    <span className="sr-only">: {fmtList(here)}</span>
                  </span>
                )}
              </td>
              <td><Players playing={s.playing} max={s.maxPlayers} /></td>
              <td className="srv-opt mono text-xs text-end">{s.friendCount ?? NONE}</td>
              <td className="mono text-xs text-end">{s.ping === null ? NONE : t("servers.pingMs", { ms: s.ping })}</td>
              <td className="srv-opt mono text-xs text-end pe-4">{s.fps === null ? NONE : fmtNum(s.fps, { minimumFractionDigits: 1, maximumFractionDigits: 1 })}</td>
              <td className="pe-3">
                <span className="flex justify-end">
                  {placeId !== null && (
                    <IconButton
                      icon="link"
                      size="sm"
                      label={t("share.copyLinkFor", { id })}
                      onClick={(e) => {
                        e.stopPropagation();
                        copyRoaltLink({ kind: "server", placeId, jobId: s.id });
                      }}
                    />
                  )}
                  <IconButton
                    icon="copy"
                    size="sm"
                    label={t("servers.copyJobIdFor", { id })}
                    onClick={(e) => {
                      e.stopPropagation();
                      void copyJob(s.id);
                    }}
                  />
                </span>
              </td>
            </tr>
          );
        }) ??
          ghosts(
            <>
              <td><Skeleton width={104} /></td>
              <td>{SEATS_GHOST}</td>
              <td className="srv-opt" />
              <td><Skeleton width={40} className="ms-auto" /></td>
              <td className="srv-opt" />
              <td />
            </>,
          )}
      </tbody>
    </table>
  );
}

/** Access codes stay out of the UI: a row only says whether this account can join. */
export function PrivateTable({ servers, pick, radio, onPick }: {
  /** Null: the first page is on its way. */
  servers: PrivateServer[] | null;
  pick: number | null;
  radio: string;
  onPick: (id: number) => void;
}) {
  const body = useRowRise(servers?.map((s) => s.vipServerId) ?? []);
  if (servers?.length === 0) return null;
  return (
    <table className="w-full border-collapse" aria-label={t("servers.privateServers")}>
      <thead>
        <tr>
          <th className="w-12 ps-5"><span className="sr-only">{t("servers.pick")}</span></th>
          <th>{t("servers.name")}</th>
          <th className="srv-opt">{t("servers.owner")}</th>
          <th className="w-[26%]">{t("servers.players")}</th>
          <th className="text-end pe-5">{t("servers.access")}</th>
        </tr>
      </thead>
      <tbody ref={body}>
        {servers?.map((s) => {
          const name = s.name || t("servers.unnamed");
          const can = s.accessCode !== null;
          const on = pick === s.vipServerId;
          return (
            <tr
              key={s.vipServerId}
              data-id={s.vipServerId}
              className={`row h-11 ${can ? "cursor-pointer" : ""}`}
              data-selected={on || undefined}
              onClick={() => can && onPick(s.vipServerId)}
            >
              <td className="ps-5">
                <input
                  type="radio"
                  className="radio"
                  name={radio}
                  aria-label={t("servers.pickName", { name })}
                  disabled={!can}
                  checked={on}
                  onChange={() => onPick(s.vipServerId)}
                />
              </td>
              <td className="pe-3 max-w-0">
                <span className="block truncate" title={name}>{name}</span>
              </td>
              <td className="srv-opt pe-3 max-w-0 truncate text-fg-2">{s.ownerName ? `@${s.ownerName}` : NONE}</td>
              <td><Players playing={s.playing} max={s.maxPlayers} /></td>
              <td className="text-end pe-5">
                <StatusPill tone={can ? "ok" : "neutral"} label={can ? t("servers.joinable") : t("servers.noAccess")} />
              </td>
            </tr>
          );
        }) ??
          ghosts(
            <>
              <td><Skeleton width={120} /></td>
              <td className="srv-opt"><Skeleton width={84} /></td>
              <td>{SEATS_GHOST}</td>
              <td className="pe-5"><Skeleton width={76} height={22} radius={999} className="ms-auto" /></td>
            </>,
          )}
      </tbody>
    </table>
  );
}
