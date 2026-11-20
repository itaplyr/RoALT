import { useId, useState } from "react";
import { api, type ClientMetrics } from "../../api";
import { qk, queryClient, useMetrics, useSetSettings, useSettings, useSystemUsage } from "../../data";
import { t } from "../../i18n";
import { Button, IconButton } from "../../ui/Button";
import { Segmented, Switch } from "../../ui/controls";
import { ConfirmDialog } from "../../ui/Dialog";
import { Dial, Meter } from "../../ui/Meter";
import { Num } from "../../ui/motion";
import { PillSeparator, PillToolbar } from "../../ui/PillToolbar";
import { MenuItem, MenuSeparator, Popover, useAnchor } from "../../ui/Popover";
import { StatusDot, StatusPill, type Tone } from "../../ui/Status";
import { toastError } from "../../ui/Toast";
import { mb, pct } from "../monitor/format";
import { useWindowTools } from "./windowTools";

const MORE = "home-more";
const PC = "home-pc";

/** A share as a micro-bar and its number; the number eases between samples. */
function Mini({ label, value }: { label: string; value: number | null }) {
  return (
    <span className="home-mini">
      <span className="home-mini-label">{label}</span>
      <span aria-hidden="true" className="contents">
        <Meter value={value} label={label} className="home-mini-bar" />
      </span>
      <Num value={value} format={pct} />
    </span>
  );
}

/** What all Roblox clients use together, as micro-bars, after a word on how much room the whole PC
 * has; it opens their dials and what each number counts. `short`: the word's shape and CPU only. */
function PcSegment({ short }: { short: boolean }) {
  const anchor = useAnchor();
  const [open, setOpen] = useState(false);
  const metrics = useMetrics().data ?? [];
  const sys = useSystemUsage(true).data ?? null;
  const total = (of: (m: ClientMetrics) => number | null) => (metrics.some((m) => of(m) !== null) ? metrics.reduce((n, m) => n + (of(m) ?? 0), 0) : null);
  const ram = sys ? Math.max(1, sys.memTotalMb) : null;
  const memUsed = total((m) => m.memoryMb);
  const commit = total((m) => m.privateMb);
  const cpu = total((m) => m.cpuPct);
  const mem = memUsed !== null && ram ? (memUsed / ram) * 100 : null;
  // Each client's own busiest engine: on one GPU the sum comes close, with clients on different
  // GPUs it says too much.
  const gpu = total((m) => m.gpuPct);
  const pcMem = sys && ram ? (sys.memUsedMb / ram) * 100 : null;
  const worst = Math.max(sys?.cpuPct ?? 0, pcMem ?? 0);
  // Without the PC's numbers there is nothing to say about its room.
  const verdict: { tone: Tone; label: string } | null = !sys ? null
    : worst < 60 ? { tone: "ok", label: t("home.plenty") }
    : worst < 85 ? { tone: "warn", label: t("home.busy") }
    : { tone: "err", label: t("home.flatOut") };
  const dials: [string, number | null, string][] = [
    [t("home.cpu"), cpu, t("home.allClients")],
    [t("home.memory"), mem, memUsed !== null && sys ? `${t("home.memOf", { used: mb(memUsed), total: mb(sys.memTotalMb) })} · ${t("home.commitAll", { commit: mb(commit ?? 0) })}` : t("home.allClients")],
    [t("home.gpu"), gpu, t("home.allClients")],
  ];
  const name = [t("home.robloxClients"), verdict?.label, ...dials.map(([label, value]) => `${label} ${pct(value)}`)].filter(Boolean).join(", ");
  return (
    <>
      <button type="button" className="home-pc" popoverTarget={PC} aria-expanded={open} aria-label={name} style={anchor.style}>
        {verdict && (short ? <StatusDot tone={verdict.tone} label={verdict.label} /> : <StatusPill tone={verdict.tone} label={verdict.label} />)}
        {(short ? dials.slice(0, 1) : dials).map(([label, value]) => (
          <Mini key={label} label={label} value={value} />
        ))}
      </button>
      <Popover
        id={PC}
        anchor={anchor.name}
        role="dialog"
        label={t("home.robloxClients")}
        className="home-pc-pop"
        onToggle={(on) => {
          setOpen(on);
          // Read out what it shows; Esc gives the focus back to the segment.
          if (on) requestAnimationFrame(() => document.getElementById(PC)?.querySelector<HTMLElement>("[data-autofocus]")?.focus());
        }}
      >
        {open && (
          <div tabIndex={-1} data-autofocus className="flex flex-col gap-3 p-2.5">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <h2 className="font-semibold">{t("home.robloxClients")}</h2>
              {verdict && <StatusPill tone={verdict.tone} label={verdict.label} />}
            </div>
            {sys && <p className="text-[13px] text-fg-2">{t("home.verdictHint", { cpu: pct(sys.cpuPct), memory: pct(pcMem) })}</p>}
            <ul className="flex flex-col gap-3">
              {dials.map(([label, value, hint]) => (
                <li key={label} className="flex items-center gap-3.5">
                  <Dial value={value} label={label} tone="accent" size={52} />
                  <span className="min-w-0 flex flex-col">
                    <span className="font-medium">{label}</span>
                    <span className="text-[13px] text-fg-2">{hint}</span>
                  </span>
                </li>
              ))}
            </ul>
          </div>
        )}
      </Popover>
    </>
  );
}

/** Anti-AFK where it's seen, running clients or not; how often it presses stays in Settings. */
function AfkSwitch() {
  const afk = useSettings().data?.antiAfk;
  const set = useSetSettings();
  const hint = useId();
  if (!afk) return null;
  return (
    <label className="home-afk" data-tip={t("settings.antiAfkShort")}>
      {t("settings.antiAfk")}
      <Switch label={t("settings.antiAfk")} describedBy={hint} checked={afk.enabled} onChange={(enabled) => set.mutate({ antiAfk: { ...afk, enabled } })} />
      <span id={hint} hidden>{t("settings.antiAfkShort")}</span>
    </label>
  );
}

/** How far a narrow header folds the pill: 1 the window tools to icons, 2 also the PC to its word's
 * shape and CPU, 3 also the tools into "…". */
export type Fold = 0 | 1 | 2 | 3;

/** Home's header pill. With clients running: the PC's room, Anti-AFK, Detailed or Compact and the
 * window tools, folded as far as `fold` says. With none: Anti-AFK alone. */
export function HomeToolbar({ running, fold, detailed, onDetailed }: { running: number; fold: Fold; detailed: boolean; onDetailed: (on: boolean) => void }) {
  const settings = useSettings().data;
  const tools = useWindowTools();
  const more = useAnchor();
  const [pressed, setPressed] = useState<"tile" | "min" | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [closing, setClosing] = useState(false);
  if (!settings) return null;
  if (!running)
    return (
      <PillToolbar label={t("home.robloxClients")}>
        <AfkSwitch />
      </PillToolbar>
    );

  const icons = fold >= 1;
  const folded = fold >= 3;
  // The pressed tool spins; the others wait for it.
  const press = (k: "tile" | "min", f: () => Promise<unknown>) => () => {
    setPressed(k);
    void f().finally(() => setPressed(null));
  };
  const tile = press("tile", () => tools.tile(false));
  const minimize = press("min", tools.minimizeAll);
  const closeAll = () => {
    setClosing(true);
    api
      .instancesCloseAll()
      .catch(toastError)
      .finally(() => {
        setClosing(false);
        setConfirm(false);
      });
  };
  return (
    <>
      <PillToolbar label={t("home.robloxClients")}>
        <PcSegment short={fold >= 2} />
        <PillSeparator />
        <AfkSwitch />
        <PillSeparator />
        <Segmented
          label={t("settings.density")}
          value={detailed ? "detailed" : "compact"}
          options={[
            { value: "detailed", label: t("settings.densityDetailed") },
            { value: "compact", label: t("settings.densityCompact") },
          ]}
          onChange={(v) => onDetailed(v === "detailed")}
        />
        <PillSeparator />
        {!folded && (
          <>
            <Button variant="ghost" size="md" icon="grid" busy={pressed === "tile"} disabled={tools.busy} className={icons ? "btn-icon" : ""} aria-label={icons ? t("dashboard.tile") : undefined} data-tip={icons ? t("dashboard.tile") : t("dashboard.tileHint")} onClick={tile}>
              {!icons && t("dashboard.tile")}
            </Button>
            <Button variant="ghost" size="md" icon="minimize" busy={pressed === "min"} disabled={tools.busy} className={icons ? "btn-icon" : ""} aria-label={icons ? t("dashboard.minimizeAll") : undefined} data-tip={icons ? t("dashboard.minimizeAll") : undefined} onClick={minimize}>
              {!icons && t("dashboard.minimizeAll")}
            </Button>
          </>
        )}
        <IconButton icon="dots" label={t("home.more")} popoverTarget={MORE} style={more.style} />
      </PillToolbar>
      <Popover
        id={MORE}
        anchor={more.name}
        placement="below-end"
        role="menu"
        label={t("home.more")}
      >
        {folded && (
          <>
            <MenuItem icon="grid" disabled={tools.busy} onSelect={tile}>{t("dashboard.tile")}</MenuItem>
            <MenuItem icon="minimize" disabled={tools.busy} onSelect={minimize}>{t("dashboard.minimizeAll")}</MenuItem>
          </>
        )}
        <MenuItem icon="external" disabled={tools.busy} onSelect={() => void tools.restoreAll()}>{t("dashboard.restoreAll")}</MenuItem>
        <MenuItem icon="grid" disabled={tools.busy} onSelect={() => void tools.tile(true)}>
          {t("dashboard.tileMicro")}{" "}
          <span dir="ltr" className="text-fg-3 tabular-nums">
            {settings.microWidth} × {settings.microHeight}
          </span>
        </MenuItem>
        <MenuItem
          icon="refresh"
          onSelect={() => {
            for (const queryKey of [qk.instances, qk.metrics, qk.multi]) void queryClient.invalidateQueries({ queryKey });
          }}
        >
          {t("dashboard.refresh")}
        </MenuItem>
        <MenuSeparator />
        <MenuItem icon="x" danger onSelect={() => setConfirm(true)}>{t("home.closeAllEllipsis")}</MenuItem>
      </Popover>
      <ConfirmDialog
        open={confirm}
        onClose={() => !closing && setConfirm(false)}
        onConfirm={closeAll}
        busy={closing}
        title={t("launch.running.closeAllTitle")}
        text={t("launch.running.closeAllText", { count: running })}
        confirmLabel={t("home.closeAllClients")}
      />
    </>
  );
}
