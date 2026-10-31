import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { GameServer, PrivateServer } from "../../api";
import { Icon } from "../../ui/Icon";
import { toast, toastError } from "../../ui/Toast";
import { fmtNum, t } from "../../i18n";

export type Pick = { kind: "public"; id: string } | { kind: "private"; id: number } | null;

export const short = (id: string) => `${id.slice(0, 8)}…${id.slice(-4)}`;
const copyJob = (id: string) => writeText(id).then(() => toast(t("servers.jobCopied")), toastError);

function Players({ playing, max }: { playing: number; max: number }) {
  return (
    <div className="flex items-center gap-3">
      <div aria-hidden="true" className="flex-1 h-1.5 bg-hover border border-line">
        <div
          className="h-1 transition-[width] duration-500"
          style={{ width: `${Math.min(100, (playing / (max || 1)) * 100)}%`, background: playing >= max ? "var(--orange)" : "var(--text)" }}
        />
      </div>
      <span className="mono text-xs w-11 text-end">{playing}/{max}</span>
    </div>
  );
}

export function PublicTable({ servers, pick, onPick }: { servers: GameServer[]; pick: string | null; onPick: (id: string) => void }) {
  if (!servers.length) return null;
  return (
    <table className="w-full min-w-[560px] border-collapse" aria-label={t("servers.publicServers")}>
      <thead>
        <tr>
          <th className="w-12 ps-6"><span className="sr-only">{t("servers.pick")}</span></th>
          <th className="label">{t("servers.jobId")}</th>
          <th className="label w-[34%]">{t("servers.players")}</th>
          <th className="label text-end">{t("servers.ping")}</th>
          <th className="label text-end pe-4">FPS</th>
          <th className="w-16"><span className="sr-only">{t("servers.actions")}</span></th>
        </tr>
      </thead>
      <tbody>
        {servers.map((s) => {
          const id = short(s.id);
          return (
            <tr key={s.id} className="row h-11 cursor-pointer" data-selected={pick === s.id || undefined} onClick={() => onPick(s.id)}>
              <td className="ps-6">
                <input type="radio" className="radio" name="server" aria-label={t("servers.pickServer", { id })} checked={pick === s.id} onChange={() => onPick(s.id)} />
              </td>
              <td className="mono text-xs">{id}</td>
              <td><Players playing={s.playing} max={s.maxPlayers} /></td>
              <td className="mono text-xs text-end">{s.ping === null ? "—" : t("servers.pingMs", { ms: s.ping })}</td>
              <td className="mono text-xs text-end pe-4">{s.fps === null ? "—" : fmtNum(s.fps, { minimumFractionDigits: 1, maximumFractionDigits: 1 })}</td>
              <td className="pe-4">
                <button
                  type="button"
                  className="wc w-9 h-8 ms-auto"
                  aria-label={t("servers.copyJobIdFor", { id })}
                  title={t("servers.copyJobId")}
                  onClick={(e) => {
                    e.stopPropagation();
                    void copyJob(s.id);
                  }}
                >
                  <Icon name="copy" size={12} />
                </button>
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

/** Access codes stay out of the UI: a row only says whether this account can join. */
export function PrivateTable({ servers, pick, onPick }: { servers: PrivateServer[]; pick: number | null; onPick: (id: number) => void }) {
  if (!servers.length) return null;
  return (
    <table className="w-full min-w-[560px] border-collapse" aria-label={t("servers.privateServers")}>
      <thead>
        <tr>
          <th className="w-12 ps-6"><span className="sr-only">{t("servers.pick")}</span></th>
          <th className="label">{t("servers.name")}</th>
          <th className="label">{t("servers.owner")}</th>
          <th className="label w-[30%]">{t("servers.players")}</th>
          <th className="label text-end pe-6">{t("servers.access")}</th>
        </tr>
      </thead>
      <tbody>
        {servers.map((s) => {
          const name = s.name || t("servers.unnamed");
          const can = s.accessCode !== null;
          return (
            <tr
              key={s.vipServerId}
              className={`row h-11 ${can ? "cursor-pointer" : ""}`}
              data-selected={pick === s.vipServerId || undefined}
              onClick={() => can && onPick(s.vipServerId)}
            >
              <td className="ps-6">
                <input
                  type="radio"
                  className="radio"
                  name="server"
                  aria-label={t("servers.pickName", { name })}
                  disabled={!can}
                  checked={pick === s.vipServerId}
                  onChange={() => onPick(s.vipServerId)}
                />
              </td>
              <td className="pe-3 max-w-0 truncate" title={name}>{name}</td>
              <td className="pe-3 max-w-0 truncate text-dim">{s.ownerName ? `@${s.ownerName}` : "—"}</td>
              <td><Players playing={s.playing} max={s.maxPlayers} /></td>
              <td className={`text-end pe-6 ${can ? "" : "text-dim"}`}>{can ? t("servers.joinable") : t("servers.noAccess")}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
