import { useRef, useState, type MouseEvent } from "react";
import { useQuery } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { api, type AccountSummary, type Instance } from "../../api";
import { qk, useMetrics, useSystemUsage } from "../../data";
import { isRtl, t } from "../../i18n";
import { Button, IconButton } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import { Meter } from "../../ui/Meter";
import { Morph, Num, SPRING, Swap, TWEEN } from "../../ui/motion";
import { StatusPill, type Tone } from "../../ui/Status";
import { toastError } from "../../ui/Toast";
import { accountName, Avatar } from "../accounts/shared";
import { launchOne, useLaunchStep } from "../launch/launchQueue";
import { STATES, uptime } from "../launch/Running";
import { mb, pct } from "../monitor/MonitorView";
import type { MenuAt } from "./ClientMenu";
import { STATE_TONE } from "./homeLayout";
import type { PreviewFps } from "./previewFps";
import type { useGridDrag } from "./useGridDrag";
import { useThumbnail } from "./useThumbnail";

export type GripProps = ReturnType<ReturnType<typeof useGridDrag>["grip"]>;
type Pill = { tone: Tone; label: string; pulse?: boolean; tip?: string };
type Tag = { label: string; tip?: string; tone?: "attention" };

const capitalized = (s: string) => s.charAt(0).toLocaleUpperCase() + s.slice(1);

/** Whose client it is: its account's name, or the account's id once the account is gone. */
export const clientName = (i: Instance, a: AccountSummary | undefined) => (a ? accountName(a) : t("dashboard.account", { id: i.accountId }));

/** A card's flex width: unsized cards share a row, up to a quarter over their base, so one left alone
 * on a row looks like the rest; a sized one keeps its width unless the window is narrower. */
export const cardFlex = (width: number | undefined, detailed: boolean) => {
  const base = detailed ? 280 : 180;
  const minWidth = `min(${detailed ? 260 : 160}px, 100%)`;
  return width ? { flex: `0 1 ${width}px`, minWidth } : { flex: `1 1 ${base}px`, maxWidth: base * 1.25, minWidth };
};

/** The game's name for a card, from the shared game cache. */
function GameName({ placeId }: { placeId: number | null }) {
  const g = useQuery({ queryKey: qk.game(placeId), queryFn: () => api.gameInfo(placeId!), enabled: placeId !== null }).data;
  return <span className="truncate">{placeId === null ? t("dashboard.noGameYet") : g?.name ?? t("dashboard.place", { id: placeId })}</span>;
}

/** What differs on this client from what Performance gives every client: what was set on it, its
 * account's choices and the FPS cap its launch or account asked for. What Performance itself applies
 * is counted in one tag, `inherited`, whose tip lists it. */
function tagsOf(i: Instance, a: AccountSummary | undefined, afkOn: boolean, defaultFps: number, macro: string | undefined) {
  const own = i.limits;
  const on = i.appliedLimits;
  const tags: Tag[] = [];
  if (a?.fullQuality) tags.push({ label: t("accounts.detail.fullQuality"), tip: t("accounts.detail.fullQualityHint") });
  // Only once the cap is on: it may have been refused.
  if (i.throttled && on.cpuCap !== null) tags.push({ label: t("dashboard.throttled"), tip: t("dashboard.throttledHint"), tone: "attention" });
  if (own.cpuCap !== null) tags.push({ label: t("dashboard.cpuPct", { pct: on.cpuCap ?? own.cpuCap }) });
  if (own.eco) tags.push({ label: t("dashboard.efficiency") });
  if (own.trimEveryMin) tags.push({ label: capitalized(t("launch.limits.trimItem")) });
  if (own.affinity !== null) tags.push({ label: capitalized(t("launch.limits.coresItem")) });
  if (afkOn && !i.antiAfk) tags.push({ label: `${t("dashboard.antiAfk")} · ${t("common.off")}` });
  if (i.micro) tags.push({ label: t("dashboard.micro") });
  if (macro) tags.push({ label: t("dashboard.macro"), tip: t("macros.busy", { name: macro }) });
  if (i.fpsCap) tags.push({ label: t("dashboard.fpsCap", { fps: i.fpsCap }), tip: t("dashboard.launchFpsCap") });
  else if (a?.fpsCap) tags.push({ label: t("dashboard.fpsCap", { fps: a.fpsCap }), tip: t("dashboard.accountFpsCap") });
  const inherited = [
    // A throttled client's cap is the throttle's, said above.
    on.cpuCap !== null && own.cpuCap === null && !i.throttled && t("dashboard.cpuPct", { pct: on.cpuCap }),
    on.eco && !own.eco && t("dashboard.efficiency"),
    !!on.trimEveryMin && own.trimEveryMin === null && capitalized(t("launch.limits.trimItem")),
    on.affinity !== null && own.affinity === null && capitalized(t("launch.limits.coresItem")),
    !i.fpsCap && !a?.fpsCap && !!defaultFps && t("dashboard.fpsCap", { fps: defaultFps }),
  ].filter((s): s is string => !!s);
  if (inherited.length) tags.push({ label: t("dashboard.fromSettingsCount", { count: inherited.length }), tip: `${t("dashboard.fromSettings")}: ${inherited.join(" · ")}` });
  return tags;
}

/** CPU and memory as micro-bars that ease between the 2 s samples. Only this part renders for them,
 * not its card. */
function Usage({ id }: { id: string }) {
  const m = useMetrics().data?.find((x) => x.instanceId === id);
  const ram = useSystemUsage(true).data?.memTotalMb;
  const commit = m && t("dashboard.memCommit", { memory: mb(m.memoryMb), commit: mb(m.privateMb) });
  return (
    <div className="home-usage">
      <span className="home-mini">
        <span className="home-mini-label">{t("home.cpu")}</span>
        <Meter value={m?.cpuPct ?? null} label={t("home.cpu")} className="home-mini-bar" />
        <Num value={m?.cpuPct} format={pct} />
      </span>
      <span className="home-mini" data-tip={commit}>
        <span className="home-mini-label">{t("home.memory")}</span>
        <Meter value={m && ram ? m.memoryMb : null} max={ram || 1} label={commit ?? t("home.memory")} className="home-mini-bar" />
        <Num value={m?.memoryMb} format={mb} />
      </span>
    </div>
  );
}

/** The corner that sizes a card: drag it, arrow keys step, double-click or Home goes back to the
 * shared width. Drags change only the element, so the picture and the other cards don't re-render.
 * `onSizing`: a drag is moving the cards. */
function SizeHandle({ name, detailed, tab, onSize, onSizing }: { name: string; detailed: boolean; tab: number; onSize: (width: number | null) => void; onSizing: () => void }) {
  const from = useRef<{ x: number; y: number; w: number; moved?: boolean } | null>(null);
  // A click, or the second of a double-click, sizes nothing. What a drag set stays, so React knows it.
  const end = (e: { currentTarget: Element }) => {
    if (from.current?.moved) {
      onSizing();
      onSize(parseFloat(tile(e).style.flexBasis));
    }
    from.current = null;
  };
  const tile = (e: { currentTarget: Element }) => e.currentTarget.closest<HTMLElement>(".tile")!;
  const fit = (el: HTMLElement, w: number) => Math.round(Math.max(detailed ? 260 : 160, Math.min(el.parentElement!.clientWidth, w)));
  return (
    <button
      type="button"
      className="size-handle"
      tabIndex={tab}
      aria-label={t("dashboard.resizeCard", { name })}
      data-tip={t("dashboard.resizeHint")}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        e.currentTarget.setPointerCapture(e.pointerId);
        from.current = { x: e.clientX, y: e.clientY, w: tile(e).getBoundingClientRect().width };
      }}
      onPointerMove={(e) => {
        const f = from.current;
        // A hand's tremble in a click isn't a drag.
        if (!f || (!f.moved && Math.hypot(e.clientX - f.x, e.clientY - f.y) < 4)) return;
        f.moved = true;
        onSizing();
        const el = tile(e);
        // The picture sets the height, so a drag down counts as the width that height needs.
        const [dx, dy] = [(e.clientX - f.x) * (isRtl() ? -1 : 1), ((e.clientY - f.y) * 16) / 9];
        const w = fit(el, f.w + (Math.abs(dx) > Math.abs(dy) ? dx : dy));
        el.style.flex = `0 1 ${w}px`;
        el.style.maxWidth = "none";
      }}
      onPointerUp={end}
      onPointerCancel={end}
      onDoubleClick={() => onSize(null)}
      onKeyDown={(e) => {
        const out = isRtl() ? -20 : 20;
        const step = ({ ArrowLeft: -out, ArrowDown: -20, ArrowRight: out, ArrowUp: 20 } as Record<string, number>)[e.key];
        if (e.key === "Home") onSize(null);
        else if (step) onSize(fit(tile(e), tile(e).getBoundingClientRect().width + step));
        else return;
        e.preventDefault();
      }}
    >
      <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round">
        <path d="M9 2 2 9M9 6 6 9" />
      </svg>
    </button>
  );
}

/** One running client: its live picture (a click brings the client to the front), one status pill
 * when something needs a look, and with `detailed` who, where, how long, what differs from the
 * defaults, what it uses and its keys; compact cards show their keys on hover or focus. The card is
 * one Tab stop: what is inside takes the Tab key only while the focus is in the card. */
export function ClientCard({ inst: i, account: a, detailed, width, menuOpen, onSize, onSizing, grip, onMenu, closing, afkOn, defaultFps, fps, macro, now }: {
  inst: Instance;
  account: AccountSummary | undefined;
  detailed: boolean;
  width: number | undefined;
  /** Its menu is open. */
  menuOpen: boolean;
  onSize: (width: number | null) => void;
  onSizing: () => void;
  grip: GripProps;
  onMenu: (at: MenuAt) => void;
  closing: boolean;
  afkOn: boolean;
  /** Performance's FPS cap. */
  defaultFps: number;
  fps: PreviewFps;
  /** The name of the macro it runs. */
  macro: string | undefined;
  now: number;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const { status, painted, ageS } = useThumbnail(i.id, canvas, fps);
  const [inside, setInside] = useState(false);
  // Launches go one at a time: while one starts, Play again says what it is doing.
  const step = useLaunchStep();
  const tab = inside ? 0 : -1;
  // No window to show or minimize yet.
  const noWindow = status === "noWindow" || (status === null && i.state === "starting");
  const hidden = status === "minimized";
  const name = clientName(i, a);
  const rejoining = i.relaunching && i.state === "disconnected";
  const again = i.state === "disconnected" && !i.relaunching && a ? a : null;
  const pictureNote = status === "minimized" ? t("dashboard.minimized")
    : status === "noWindow" ? t("dashboard.noWindow")
    : status === "pending" ? t("dashboard.waitingPicture")
    : status === "unsupported" ? t("dashboard.noPreview")
    : "";
  // The one thing to know, by priority; a client in game with a live picture needs no pill.
  const pill: Pill | null = closing ? { tone: "neutral", label: t("launch.running.closing"), pulse: true }
    : rejoining ? { tone: "info", label: t("launch.state.rejoining"), pulse: true }
    : i.state !== "inGame" ? { tone: STATE_TONE[i.state], label: STATES[i.state][0], pulse: i.state !== "disconnected" }
    : painted && pictureNote ? { tone: "neutral", label: pictureNote, tip: hidden ? t("dashboard.lastPictureHint") : undefined }
    : ageS !== null ? { tone: "neutral", label: t("dashboard.pictureAge", { s: ageS }), tip: t("dashboard.pictureAgeHint") }
    : null;
  const tags = detailed ? tagsOf(i, a, afkOn, defaultFps, macro) : [];
  const show = () => api.instanceShow(i.id, "focus").catch(toastError);
  const minimize = () => api.instanceShow(i.id, hidden ? "restore" : "minimize").catch(toastError);
  const menuFrom = (e: MouseEvent<HTMLElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    onMenu({ inst: i, hidden, x: r.right, y: r.top - 8, from: e.currentTarget, beside: r.left });
  };
  const more = <IconButton icon="dots" label={t("home.moreFor", { name })} aria-haspopup="menu" aria-expanded={menuOpen} tabIndex={tab} onClick={menuFrom} />;

  return (
    <div
      role="group"
      tabIndex={0}
      aria-label={t("dashboard.cardLabel", { name, state: pill?.label ?? STATES[i.state][0] })}
      data-group={i.accountId}
      data-id={i.id}
      data-flip
      className={`tile panel home-card ${detailed ? "" : "home-card-compact"}`}
      style={cardFlex(width, detailed)}
      onFocus={() => setInside(true)}
      onBlur={(e) => !e.currentTarget.contains(e.relatedTarget) && setInside(false)}
      onDoubleClick={(e) => {
        if (!(e.target as Element).closest("button, .home-pic")) void show();
      }}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter") void show();
        // From the keyboard (no pointer position): at the card's corner. Handled here so it
        // works whether or not the webview turns the key into a contextmenu event.
        if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
          e.preventDefault();
          const r = e.currentTarget.getBoundingClientRect();
          onMenu({ inst: i, hidden, x: r.left + 12, y: r.top + 12, from: e.currentTarget });
        }
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        onMenu({ inst: i, hidden, x: e.clientX, y: e.clientY, from: e.currentTarget });
      }}
    >
      <div
        className="home-pic"
        data-painted={painted || undefined}
        data-dim={(painted && (!!pictureNote || ageS !== null)) || undefined}
        onClick={(e) => {
          // Its keys sit on the picture; the second click of a double-click is no new one.
          if (e.detail < 2 && !(e.target as Element).closest("button")) void show();
        }}
      >
        <canvas ref={canvas} aria-hidden="true" />
        {!painted && (
          <span className="home-pic-wait" data-breathe={status === null || status === "pending" || status === "noWindow" || undefined}>
            {pictureNote}
          </span>
        )}
        <AnimatePresence initial={false}>
          {pill && (
            <motion.div
              key="pill"
              className="home-pill"
              data-tip={pill.tip}
              initial={{ opacity: 0, scale: 0.92 }}
              animate={{ opacity: 1, scale: 1, transition: SPRING.snappy }}
              exit={{ opacity: 0, scale: 0.92, transition: TWEEN.exit }}
            >
              <Morph>
                <StatusPill tone={pill.tone} pulse={pill.pulse} label={<Swap k={pill.label}>{pill.label}</Swap>} />
              </Morph>
            </motion.div>
          )}
        </AnimatePresence>
        <button type="button" className="grip" tabIndex={tab} aria-label={t("accounts.reorder", { name })} {...grip}>
          <Icon name="grip" size={14} />
        </button>
        {!detailed && (
          <div className="home-act">
            <IconButton icon="eye" label={t("home.show")} disabled={noWindow} tabIndex={tab} onClick={show} />
            {again ? (
              <IconButton icon="play" label={step ?? t("home.playAgain")} disabled={step !== null} tabIndex={tab} onClick={() => void launchOne(again, i.target, i.fpsCap)} />
            ) : (
              <IconButton icon={hidden ? "external" : "minimize"} label={hidden ? t("dashboard.restore") : t("dashboard.minimize")} disabled={noWindow} tabIndex={tab} onClick={minimize} />
            )}
            {more}
          </div>
        )}
      </div>
      {detailed ? (
        <div className="home-info">
          <div className="flex items-center gap-2.5 min-w-0">
            {a && <Avatar account={a} size={32} />}
            <div className="flex-1 min-w-0 flex flex-col">
              <span className="font-medium truncate">{name}</span>
              <span className="flex gap-1 text-[13px] text-fg-2 min-w-0">
                <GameName placeId={i.placeId} />
                <span className="flex-none tabular-nums">· {uptime(i.startedAt, now)}</span>
              </span>
            </div>
          </div>
          {i.note && !rejoining && <p className="alert text-[13px]">{i.note}</p>}
          {tags.length > 0 && (
            <div className="flex flex-wrap gap-1">
              {/* A tip is also there for the keyboard (focus shows it) and screen readers. */}
              {tags.map((g) => (
                <span key={g.label} className={`tag ${g.tone ? "text-attention" : ""}`} data-tip={g.tip} tabIndex={g.tip ? tab : undefined}>
                  {g.label}
                  {g.tip && <span className="sr-only"> {g.tip}</span>}
                </span>
              ))}
            </div>
          )}
          <Usage id={i.id} />
          {/* Clear of the size handle in the corner. */}
          <div className="flex gap-1.5 pe-3.5">
            <Button size="md" icon="eye" className="flex-1 min-w-0" disabled={noWindow} tabIndex={tab} onClick={show}>
              {t("home.show")}
            </Button>
            {again ? (
              // With auto-rejoin off, only this and the menu's item bring it back.
              <Button size="md" icon="play" className="flex-1 min-w-0" disabled={step !== null} tabIndex={tab} onClick={() => void launchOne(again, i.target, i.fpsCap)}>
                <span className="truncate">{step ?? t("home.playAgain")}</span>
              </Button>
            ) : (
              <Button size="md" icon={hidden ? "external" : "minimize"} className="flex-1 min-w-0" disabled={noWindow} tabIndex={tab} onClick={minimize}>
                {hidden ? t("dashboard.restore") : t("dashboard.minimize")}
              </Button>
            )}
            {more}
          </div>
        </div>
      ) : (
        <div className="flex flex-col gap-0.5 min-w-0 ps-1 pe-5 pb-0.5">
          <span className="font-medium truncate text-[13px]">{a?.username ?? name}</span>
          {i.note && !rejoining && <span className="alert text-[12px]">{i.note}</span>}
        </div>
      )}
      <SizeHandle name={name} detailed={detailed} tab={tab} onSize={onSize} onSizing={onSizing} />
    </div>
  );
}
