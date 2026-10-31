import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, type AccountSummary, type ClientMetrics, type GameInfo, type Instance, type Limits, type WindowShow } from "../../api";
import { qk, queryClient, useInstances, useMetrics, useSetSettings, useSettings, useSystemUsage } from "../../data";
import { Segmented, Switch } from "../../ui/controls";
import { ConfirmDialog } from "../../ui/Dialog";
import { Icon, type IconName } from "../../ui/Icon";
import { toast, toastError } from "../../ui/Toast";
import { fmtList, isRtl, t } from "../../i18n";
import { accountName, Avatar } from "../accounts/shared";
import { EndedPanel, isRunning, LimitsDialog, STATES, uptime, useNow } from "../launch/Running";
import { launched, launchFailed } from "../launch/LaunchView";
import { mb, pct } from "../monitor/MonitorView";
import { menuKeys } from "../../ui/menu";
import { AccountTabs } from "./AccountTabs";
import { homeSorted, reordered, useHomeDetailed, useHomeOrder, useHomeSizes, type HomeTabs } from "./homeLayout";
import { usePreviewFps, type PreviewFps } from "./previewFps";
import { useGridDrag } from "./useGridDrag";

const CAPS = [0, 50, 25, 10, 5];
/** How often a hidden RoALT window looks again whether it is shown. */
const HIDDEN_MS = 500;

/** All cards together ask for at most this many pictures a second.
 * ponytail: a guess, tune it from measurements with many clients. */
const PICTURES_PER_SECOND = 60;
/** A picture counts as old from this age, and only after asking this long without a break. */
const STALE_MS = 3000;
const STATUSES = ["live", "minimized", "noWindow", "pending", "unsupported"] as const;
/** The cards scrolled into view, sharing `PICTURES_PER_SECOND`. */
let cardsInView = 0;

/** Draws the client's window on `canvas` up to `fps` times a second while the RoALT window is
 * visible, one request in flight at a time; a request that gets no new picture changes nothing on
 * the canvas. `status`: what the client's window is doing (`window::Status`), null before the first
 * answer; `painted`: the canvas holds a picture, maybe an old one; `ghost`: RoALT hid it, and it
 * draws while asked for; `ageS`: seconds since its picture, when it is live and that has gone on too long. */
function useThumbnail(id: string, canvas: React.RefObject<HTMLCanvasElement | null>, fps: PreviewFps) {
  const [status, setStatus] = useState<(typeof STATUSES)[number] | null>(null);
  const [painted, setPainted] = useState(false);
  const [ghost, setGhost] = useState(false);
  const [ageS, setAgeS] = useState<number | null>(null);
  useEffect(() => {
    let stop = false;
    let timer: number | undefined;
    let known = 0;
    // Nothing was asked for while out of sight: a picture isn't old until asked for a while again.
    let wasVisible = false;
    let askingSince = 0;
    let wasLive = false;
    // A card scrolled out of sight costs no pictures.
    let inView = true;
    cardsInView++;
    const seen = new IntersectionObserver((entries) => {
      const now = entries[entries.length - 1].isIntersecting;
      if (now !== inView) cardsInView += now ? 1 : -1;
      inView = now;
    });
    if (canvas.current) seen.observe(canvas.current);
    const tick = async () => {
      const started = performance.now();
      const el = canvas.current;
      const visible = document.visibilityState === "visible" && inView;
      if (visible && !wasVisible) askingSince = started;
      wasVisible = visible;
      if (el && visible) {
        try {
          const buf = await api.instanceThumbnail(id, Math.min(960, Math.max(320, Math.round(el.clientWidth * devicePixelRatio))), known);
          if (stop) return;
          const head = new DataView(buf);
          const [w, h, code, age] = [head.getUint32(0, true), head.getUint32(4, true), head.getUint32(8, true), head.getUint32(20, true)];
          if (buf.byteLength > 24) {
            // Resizing clears the canvas and costs a reallocation: only when the size changes.
            if (el.width !== w) el.width = w;
            if (el.height !== h) el.height = h;
            el.getContext("2d")?.putImageData(new ImageData(new Uint8ClampedArray(buf, 24, w * h * 4), w, h), 0, 0);
            known = head.getUint32(16, true);
            setPainted(true);
          }
          setStatus(STATUSES[code]);
          setGhost(head.getUint32(12, true) === 1);
          // Back from minimized or no window: no picture came meanwhile, as when out of sight.
          const live = STATUSES[code] === "live";
          if (live && !wasLive) askingSince = performance.now();
          wasLive = live;
          setAgeS(live && age >= STALE_MS && performance.now() - askingSince >= STALE_MS ? Math.floor(age / 1000) : null);
        } catch {
          // Ended or not started: the card goes with the next instances list.
        }
      }
      const delay = Math.max(1000 / fps, (cardsInView * 1000) / PICTURES_PER_SECOND);
      if (!stop) timer = window.setTimeout(tick, visible ? Math.max(0, delay - (performance.now() - started)) : HIDDEN_MS);
    };
    void tick();
    return () => {
      stop = true;
      clearTimeout(timer);
      seen.disconnect();
      if (inView) cardsInView--;
    };
  }, [id, canvas, fps]);
  return { status, painted, ghost, ageS };
}

/** `beside`: the left edge of the key that opened it, `x` its right edge. The menu opens on the
 * side the card's text starts from (left of the key, or right of it right to left), not over it,
 * where a second click would land on an item. */
type MenuAt = { inst: Instance; hidden: boolean; x: number; y: number; from: HTMLElement; beside?: number };

const closeKey = (i: Instance) => `${i.id}:${i.pid}`;
const capitalized = (s: string) => s.charAt(0).toLocaleUpperCase() + s.slice(1);
const playAgain = (a: AccountSummary, i: Instance) => api.launch(a.id, i.target, undefined, i.fpsCap).then((info) => launched(a, info), launchFailed);

/** The game's name for a card, from the shared game cache. */
function GameName({ placeId }: { placeId: number | null }) {
  const g = useQuery({ queryKey: qk.game(placeId), queryFn: () => api.gameInfo(placeId!), enabled: placeId !== null }).data;
  return <span className="truncate">{placeId === null ? t("dashboard.noGameYet") : g?.name ?? t("dashboard.place", { id: placeId })}</span>;
}

/** A card's flex width: unsized cards share a row, up to a quarter over their base, so one left alone
 * on a row looks like the rest; a sized one keeps its width unless the window is narrower. */
const cardFlex = (width: number | undefined, detailed: boolean) => {
  const base = detailed ? 280 : 180;
  const minWidth = `min(${detailed ? 260 : 160}px, 100%)`;
  return width ? { flex: `0 1 ${width}px`, minWidth } : { flex: `1 1 ${base}px`, maxWidth: base * 1.25, minWidth };
};

/** The corner that sizes a card: drag it, arrow keys step, double-click or Home goes back to the
 * shared width. Drags change only the element, so the picture and the other cards don't re-render. */
function SizeHandle({ name, detailed, onSize }: { name: string; detailed: boolean; onSize: (width: number | null) => void }) {
  const from = useRef<{ x: number; y: number; w: number; moved?: boolean } | null>(null);
  // A click, or the second of a double-click, sizes nothing. What a drag set stays, so React knows it.
  const end = (e: { currentTarget: Element }) => {
    if (from.current?.moved) onSize(parseFloat(tile(e).style.flexBasis));
    from.current = null;
  };
  const tile = (e: { currentTarget: Element }) => e.currentTarget.closest<HTMLElement>(".tile")!;
  const fit = (el: HTMLElement, w: number) => Math.round(Math.max(detailed ? 260 : 160, Math.min(el.parentElement!.clientWidth, w)));
  return (
    <button
      type="button"
      className="size-handle"
      aria-label={t("dashboard.resizeCard", { name })}
      title={t("dashboard.resizeHint")}
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

/** One running client: its live picture, who, where, how long, what it uses, and its keys; with
 * `detailed` off only the picture and the username, the rest being in the right-click menu. */
function ClientCard({ inst: i, account: a, metrics: m, now, afkOn, defaultFps, fps, closing, detailed, width, onSize, grip, onMenu }: {
  inst: Instance;
  account: AccountSummary | undefined;
  metrics: ClientMetrics | undefined;
  now: number;
  afkOn: boolean;
  /** Performance's FPS cap. */
  defaultFps: number;
  fps: PreviewFps;
  closing: boolean;
  detailed: boolean;
  width: number | undefined;
  onSize: (width: number | null) => void;
  grip: ReturnType<typeof useGridDrag>["grip"];
  onMenu: (at: MenuAt) => void;
}) {
  const card = useRef<HTMLDivElement>(null);
  // Rises in. Done with WAAPI because a CSS animation restarts whenever React moves the card.
  useLayoutEffect(() => {
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    card.current?.animate([{ opacity: 0, transform: "translateY(8px)" }, { opacity: 1, transform: "none" }], { duration: 220, easing: "cubic-bezier(.2,.8,.2,1)" });
  }, []);
  const canvas = useRef<HTMLCanvasElement>(null);
  const { status, painted, ghost, ageS } = useThumbnail(i.id, canvas, fps);
  // No window to show or minimize yet.
  const noWindow = status === "noWindow" || (status === null && i.state === "starting");
  const hidden = status === "minimized" || ghost;
  const name = a ? accountName(a) : t("dashboard.account", { id: i.accountId });
  const rejoining = i.relaunching && i.state === "disconnected";
  const [label, color] = closing ? [t("launch.running.closing"), "var(--dim)"] : rejoining ? [t("launch.state.rejoining"), "var(--orange)"] : STATES[i.state];
  const moving = i.state === "starting" || i.state === "joining" || rejoining || closing;
  const focus = () => api.instanceShow(i.id, "focus").catch(toastError);
  const throttled = t("dashboard.throttled");
  const pictureNote = status === "minimized" ? t("dashboard.minimized")
    : status === "noWindow" ? (i.state === "starting" ? t("dashboard.starting") : t("dashboard.noWindow"))
    : status === "pending" ? t("dashboard.waitingPicture")
    : status === "unsupported" ? t("dashboard.noPreview")
    : "";
  const badges:[string, string?][] = [
    ...(i.throttled ? [[throttled, t("dashboard.throttledHint")] as [string, string]] : []),
    ...(i.limits.cpuCap !== null ? [[t("dashboard.cpuPct", { pct: i.limits.cpuCap })] as [string]] : []),
    ...(i.limits.eco ? [[t("dashboard.efficiency")] as [string]] : []),
    ...(i.limits.trimEveryMin !== null ? [[capitalized(t("launch.limits.trimItem"))] as [string]] : []),
    ...(i.limits.affinity !== null ? [[capitalized(t("launch.limits.coresItem"))] as [string]] : []),
    ...(afkOn && i.antiAfk ? [[t("dashboard.antiAfk"), i.afkAt !== null ? t("dashboard.lastKeyPress", { time: uptime(i.afkAt, now) }) : t("dashboard.noKeyPress")] as [string, string]] : []),
    ...(i.micro ? [[t("dashboard.micro")] as [string]] : []),
    ...(i.fpsCap ? [[t("dashboard.fpsCap", { fps: i.fpsCap }), t("dashboard.launchFpsCap")] as [string, string]]
      : a?.fpsCap ? [[t("dashboard.fpsCap", { fps: a.fpsCap }), t("dashboard.accountFpsCap")] as [string, string]]
      : defaultFps ? [[t("dashboard.fpsCap", { fps: defaultFps }), t("dashboard.defaultFpsCap")] as [string, string]] : []),
  ];
  return (
    <div
      ref={card}
      role="group"
      tabIndex={0}
      aria-label={t("dashboard.cardLabel", { name, state: label })}
      data-group={i.accountId}
      className={`tile panel relative flex flex-col outline-offset-2 ${detailed ? "gap-3 p-3.5" : "gap-2 p-2.5"}`}
      style={cardFlex(width, detailed)}
      title={`PID ${i.pid}`}
      onDoubleClick={(e) => {
        if (!(e.target as Element).closest("button")) void focus();
      }}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter") void focus();
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
      <div className="bezel">
        <div className="relative aspect-video overflow-hidden rounded-[9px]">
          <canvas ref={canvas} aria-hidden="true" className={`w-full h-full object-contain ${painted ? "" : "invisible"} ${pictureNote || ageS !== null ? "opacity-50" : ""}`} />
          {!painted && <span className="absolute inset-0 grid place-items-center text-xs text-dim">{pictureNote}</span>}
        </div>
        {painted && pictureNote ? <span className="badge" title={status === "minimized" ? t("dashboard.lastPictureHint") : undefined}>{pictureNote}</span>
          : ageS !== null ? <span className="badge" title={t("dashboard.pictureAgeHint")}>{t("dashboard.pictureAge", { s: ageS })}</span>
          : ghost && <span className="badge" title={t("dashboard.hiddenHint")}>{t("dashboard.hidden")}</span>}
        <button type="button" className="grip" aria-label={t("accounts.reorder", { name })} {...grip(name)}>
          <Icon name="grip" size={14} />
        </button>
      </div>
      {detailed ? (
        <>
          <div className="flex items-center gap-3 min-w-0">
            {a && <Avatar account={a} size={36} />}
            <div className="flex-1 min-w-0 flex flex-col">
              <span className="font-medium truncate">{name}</span>
              <span className="flex gap-1 text-[13px] text-dim min-w-0">
                <GameName placeId={i.placeId} />
                <span className="mono flex-none">· {uptime(i.startedAt, now)}</span>
              </span>
            </div>
            <span className="state flex-none text-[13px]" style={{ color }}>
              <span aria-hidden="true" className={`led ${moving ? "pulse" : ""}`} />
              {label}
            </span>
          </div>
          {i.note && !rejoining && <span className="text-[13px] text-orange">{i.note}</span>}
          <div className="flex items-center justify-between gap-2 text-xs text-dim min-w-0">
            <span className="flex flex-wrap gap-1 min-w-0">
              {badges.map(([b, tip]) => (
                <span key={b} className={`tag ${b === throttled ? "text-orange" : ""}`} title={tip}>{b}</span>
              ))}
            </span>
            <span className="mono flex-none ms-auto" title={m ? t("monitor.commit", { mb: mb(m.privateMb) }) : undefined} style={{ fontVariantNumeric: "tabular-nums" }}>
              {m ? `${pct(m.cpuPct)} · ${mb(m.memoryMb)}` : "—"}
            </span>
          </div>
          <div className="flex gap-2.5">
            <button className="btn flex-1 min-w-0 px-2 py-1 whitespace-normal leading-tight" disabled={noWindow} onClick={focus}>
              {t("home.show")}
            </button>
            {i.state === "disconnected" && !i.relaunching && a ? (
              // With auto-rejoin off, only this and the menu's item bring it back.
              <button className="btn btn-primary flex-1 min-w-0 px-2 py-1 whitespace-normal leading-tight" onClick={() => playAgain(a, i)}>
                {t("home.playAgain")}
              </button>
            ) : (
              <button className="btn flex-1 min-w-0 px-2 py-1 whitespace-normal leading-tight" disabled={noWindow} onClick={() => api.instanceShow(i.id, hidden ? "restore" : "minimize").catch(toastError)}>
                {hidden ? t("dashboard.restore") : t("dashboard.minimize")}
              </button>
            )}
            <button
              className="btn btn-icon"
              aria-label={t("home.moreFor", { name })}
              title={t("dashboard.clientActions")}
              aria-haspopup="menu"
              onClick={(e) => {
                const r = e.currentTarget.getBoundingClientRect();
                onMenu({ inst: i, hidden, x: r.right, y: r.top - 8, from: e.currentTarget, beside: r.left });
              }}
            >
              <Icon name="dots" />
            </button>
          </div>
        </>
      ) : (
        <>
          <div className="flex items-center gap-2.5 min-w-0 px-1">
            {/* Only what needs a look; the card's label always says the state. */}
            {(closing || i.state !== "inGame") && <span aria-hidden="true" title={label} className={`led ${moving ? "pulse" : ""}`} style={{ color }} />}
            <span className="font-medium truncate">{a?.username ?? name}</span>
          </div>
          {i.note && !rejoining && <span className="px-1 text-[13px] text-orange">{i.note}</span>}
        </>
      )}
      <SizeHandle name={name} detailed={detailed} onSize={onSize} />
    </div>
  );
}

type Item =
  | { icon?: IconName; label: string; run: () => unknown; danger?: boolean; checked?: boolean; disabled?: boolean }
  | { caps: true }
  | null;

/** A card's right-click menu: a popover, so Esc and a click outside close it; arrows move. */
function ClientMenu({ at, account, onClose, afkOn, closing, onLimits, onCloseClient }: {
  at: MenuAt | null;
  account: AccountSummary | undefined;
  onClose: () => void;
  afkOn: boolean;
  closing: ReadonlySet<string>;
  onLimits: (id: string) => void;
  onCloseClient: (i: Instance, force: boolean) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: 0, top: 0 });

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !at) return;
    el.showPopover();
    const r = el.getBoundingClientRect();
    setPos({
      left: at.beside === undefined ? Math.max(4, Math.min(at.x, innerWidth - r.width - 4)) : isRtl() ? Math.min(at.x + 6, innerWidth - r.width - 4) : Math.max(4, at.beside - r.width - 6),
      top: Math.max(4, Math.min(at.y, innerHeight - r.height - 4)),
    });
    el.querySelector<HTMLElement>("[role^=menuitem]")?.focus();
  }, [at]);

  useEffect(() => {
    const el = ref.current;
    const toggled = (e: Event) => {
      if ((e as ToggleEvent).newState !== "closed") return;
      // Focus goes back to the card unless the click went somewhere else.
      if (!document.activeElement || document.activeElement === document.body || el?.contains(document.activeElement)) at?.from.focus();
      onClose();
    };
    el?.addEventListener("toggle", toggled);
    return () => el?.removeEventListener("toggle", toggled);
  }, [onClose, at]);

  const i = at?.inst;
  const settings = useSettings().data;
  const tiny = { w: settings?.microWidth ?? 320, h: settings?.microHeight ?? 240 };
  const run = (f: () => unknown) => () => {
    ref.current?.hidePopover();
    void f();
  };
  const show = (what: WindowShow) => i && api.instanceShow(i.id, what).catch(toastError);
  const limits = (patch: Partial<Limits>) => i && api.instanceSetLimits(i.id, { ...i.limits, ...patch }).catch(toastError);
  const micro = () =>
    i &&
    api.instanceMicro(i.id, !i.micro).then(([w, h]) => {
      if (!i.micro && (w > tiny.w || h > tiny.h)) toast(t("dashboard.microKept", { w, h }));
    }, toastError);
  const trim = () => i && api.instanceTrim(i.id).then(() => toast(t("dashboard.trimmed")), toastError);

  const items: Item[] = i
    ? [
        ...(i.state === "disconnected" && !i.relaunching && account ? [{ icon: "play", label: t("home.playAgain"), run: () => playAgain(account, i) } as Item] : []),
        { icon: "external", label: t("dashboard.focus"), run: () => show("focus") },
        at.hidden ? { label: t("dashboard.restore"), run: () => show("restore") } : { label: t("dashboard.minimize"), run: () => show("minimize") },
        { icon: "grid", label: i.micro ? t("dashboard.normalSize") : t("dashboard.microWindow", tiny), run: micro },
        null,
        { icon: "refresh", label: t("dashboard.trimMemory"), run: trim },
        { caps: true },
        { label: t("dashboard.efficiencyMode"), checked: i.limits.eco, run: () => limits({ eco: !i.limits.eco }) },
        { label: afkOn ? t("dashboard.antiAfk") : t("dashboard.antiAfkOff"), checked: i.antiAfk, run: () => api.instanceSetAntiAfk(i.id, !i.antiAfk).catch(toastError) },
        { icon: "sliders", label: t("home.limits"), run: () => onLimits(i.id) },
        null,
        { icon: "x", label: t("common.close"), run: () => onCloseClient(i, false), disabled: closing.has(closeKey(i)) },
        { icon: "x", label: t("dashboard.forceClose"), run: () => onCloseClient(i, true), danger: true },
      ]
    : [];

  return (
    <div
      ref={ref}
      popover="auto"
      role="menu"
      aria-label={t("dashboard.clientActions")}
      className="menu"
      onKeyDown={menuKeys}
      style={{ position: "fixed", positionArea: "none", inset: "auto", left: pos.left, top: pos.top, margin: 0, maxHeight: "calc(100vh - 8px)" }}
    >
      {items.map((it, n) =>
        it === null ? (
          <div key={n} role="separator" className="h-px my-1 bg-line" />
        ) : "caps" in it ? (
          <div key="caps" role="group" aria-label={t("dashboard.cpuLimit")} className="flex items-center gap-1 min-h-10 px-2.5">
            <span className="flex-1 text-dim">{t("dashboard.cpuLimit")}</span>
            {CAPS.map((c) => (
              <button
                key={c}
                role="menuitemradio"
                aria-checked={(i?.limits.cpuCap ?? 0) === c}
                className="seg mono min-h-7 px-1.5 text-xs"
                aria-label={c ? t("dashboard.cpuLimitPct", { pct: c }) : t("dashboard.noCpuLimit")}
                onClick={run(() => limits({ cpuCap: c || null }))}
              >
                {c ? `${c}` : t("common.off")}
              </button>
            ))}
          </div>
        ) : (
          <button
            key={it.label}
            role={it.checked === undefined ? "menuitem" : "menuitemcheckbox"}
            aria-checked={it.checked}
            className={`menu-item ${it.danger ? "text-orange" : ""}`}
            disabled={it.disabled}
            onClick={run(it.run)}
          >
            {it.checked !== undefined ? (
              <Icon name="check" size={12} className={it.checked ? "" : "invisible"} />
            ) : it.icon ? (
              <Icon name={it.icon} size={12} />
            ) : (
              <span className="w-3" />
            )}
            {it.label}
          </button>
        ),
      )}
    </div>
  );
}

/** A share of something, as a sunk dial. Null: Windows doesn't say. */
function Dial({ value, label, hint, text }: { value: number | null; label: string; hint: string; text?: string }) {
  const circumference = 188.5;
  const shown = Math.max(0, Math.min(100, value ?? 0));
  return (
    <div className="flex-[1_1_170px] flex items-center gap-3.5 min-w-0">
      <div className="dial" style={{ color: "var(--accent-text)" }}>
        <svg viewBox="0 0 76 76" aria-hidden="true">
          {value !== null && <circle cx="38" cy="38" r="30" strokeDasharray={`${(shown / 100) * circumference} ${circumference}`} />}
        </svg>
        <span className="mono relative text-[15px] font-medium text-text">{text ?? (value === null ? "—" : `${Math.round(shown)}%`)}</span>
      </div>
      <div className="min-w-0">
        <p className="font-medium">{label}</p>
        <p className="text-[13px] text-dim">{hint}</p>
      </div>
    </div>
  );
}

/** What all Roblox clients use together, in dials; the words say how much room the whole PC has. */
function PcPanel({ metrics }: { metrics: ClientMetrics[] }) {
  const sys = useSystemUsage(true).data ?? null;
  const total = (of: (m: ClientMetrics) => number | null) => (metrics.some((m) => of(m) !== null) ? metrics.reduce((n, m) => n + (of(m) ?? 0), 0) : null);
  const memTotal = sys ? Math.max(1, sys.memTotalMb) : null;
  const memUsed = total((m) => m.memoryMb);
  const pcMem = sys && memTotal ? (sys.memUsedMb / memTotal) * 100 : null;
  // Each client's own busiest engine: on one GPU the sum comes close, with clients on different
  // GPUs it says too much.
  const gpu = total((m) => m.gpuPct);
  const worst = Math.max(sys?.cpuPct ?? 0, pcMem ?? 0);
  const [verdict, color] = worst < 60
    ? [t("home.plenty"), "var(--accent-text)"]
    : worst < 85
      ? [t("home.busy"), "var(--orange)"]
      : [t("home.flatOut"), "var(--err)"];
  return (
    <section aria-labelledby="pc-title" className="panel flex flex-wrap items-center gap-x-8 gap-y-5 p-5">
      <div className="flex-[1_1_170px] min-w-0">
        <h2 id="pc-title" className="section-title">{t("home.robloxClients")}</h2>
        {/* Without the PC's numbers there is nothing to say about its room. */}
        {sys && (
          <p className="state mt-2" style={{ color }} title={t("home.verdictHint", { cpu: pct(sys.cpuPct), memory: pct(pcMem) })}>
            <span aria-hidden="true" className="led" />
            {verdict}
          </p>
        )}
      </div>
      <Dial value={total((m) => m.cpuPct)} label={t("home.cpu")} hint={t("home.allClients")} />
      <Dial value={memUsed !== null && memTotal ? (memUsed / memTotal) * 100 : null} label={t("home.memory")} hint={memUsed !== null && sys ? t("home.memOf", { used: mb(memUsed), total: mb(sys.memTotalMb) }) : t("home.allClients")} />
      <Dial value={gpu} label={t("home.gpu")} hint={t("home.allClients")} />
    </section>
  );
}

/** Anti-AFK where it's seen, running clients or not; how often it presses stays in Settings. */
function AfkPanel() {
  const afk = useSettings().data?.antiAfk;
  const set = useSetSettings();
  if (!afk) return null;
  return (
    <section aria-labelledby="afk-title" className="panel flex flex-wrap items-center gap-x-5 gap-y-3 px-5 py-4">
      <div className="flex-[1_1_320px] min-w-0">
        <h2 id="afk-title" className="section-title">{t("settings.antiAfk")}</h2>
        <p className="text-dim text-[13px] mt-1">{afk.enabled ? (afk.mode === "focus" ? t("settings.antiAfkFocusHint") : t("settings.antiAfkBackgroundHint")) : t("settings.antiAfkHint")}</p>
      </div>
      {afk.enabled && (
        <Segmented
          label={t("settings.antiAfkMode")}
          value={afk.mode}
          options={[
            { value: "focus", label: t("settings.antiAfkFocus") },
            { value: "background", label: t("settings.inBackground") },
          ]}
          onChange={(mode) => set.mutate({ antiAfk: { ...afk, mode } })}
        />
      )}
      <Switch label={t("settings.antiAfk")} checked={afk.enabled} onChange={(enabled) => set.mutate({ antiAfk: { ...afk, enabled } })} />
    </section>
  );
}

/** What a client's relaunch is waiting for, with the ways out. */
function Trouble({ inst: i, account: a }: { inst: Instance; account: AccountSummary | undefined }) {
  const name = a ? accountName(a) : t("dashboard.account", { id: i.accountId });
  return (
    <section role="status" className="enter panel flex flex-wrap items-center gap-x-5 gap-y-3 px-5 py-4">
      <span aria-hidden="true" className="led pulse" style={{ color: "var(--orange)" }} />
      <div className="flex-[1_1_320px] min-w-0">
        <p className="font-medium">{t("home.lost", { name })}</p>
        <p className="text-dim text-[13px]">{t("home.retryLine", { why: i.relaunchProblem ?? "" })}</p>
      </div>
      <div className="flex gap-2.5">
        <button className="btn" onClick={() => api.instancesRetryRelaunch().catch(toastError)}>{t("home.tryNow")}</button>
        <button className="btn btn-ghost" onClick={() => api.instanceStopRelaunch(i.id).catch(toastError)}>{t("home.stopTrying")}</button>
      </div>
    </section>
  );
}

/** Three steps to a first game, or what to do while nothing runs. */
function Welcome({ hasAccounts, signedOut, onAdd, onImport, onPlay, onAccounts }: { hasAccounts: boolean; signedOut: boolean; onAdd: () => void; onImport: () => void; onPlay: () => void; onAccounts: () => void }) {
  return (
    <>
      <header className="max-w-[640px]">
        <h1 className="page-title">{signedOut ? t("home.signedOutTitle") : hasAccounts ? t("home.idle") : t("home.welcome")}</h1>
        <p className="page-sub">{signedOut ? t("home.signedOutLead") : hasAccounts ? t("home.idleLead") : t("home.welcomeLead")}</p>
        {signedOut && <button className="btn btn-primary mt-3" onClick={onAccounts}>{t("app.nav.accounts")}</button>}
      </header>
      <section className="grid gap-6" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(min(250px, 100%), 1fr))" }}>
        <article className="panel flex flex-col gap-3.5 p-5">
          <span className={`num ${hasAccounts ? "num-done" : ""}`}>{hasAccounts ? <Icon name="check" size={18} /> : "1"}</span>
          <h2 className="section-title">{t("home.step1")}</h2>
          <p className="flex-1 text-dim">{t("home.step1Text")}</p>
          <button className={`btn ${hasAccounts ? "" : "btn-primary"}`} onClick={onAdd}>
            <Icon name="plus" size={14} />
            {hasAccounts ? t("home.addAnother") : t("accounts.add.title")}
          </button>
        </article>
        <article className="panel flex flex-col gap-3.5 p-5">
          <span className="num">2</span>
          <h2 className="section-title">{t("home.step2")}</h2>
          <p className="flex-1 text-dim">{t("home.step2Text")}</p>
          <button className={`btn ${hasAccounts && !signedOut ? "btn-primary" : ""}`} disabled={!hasAccounts || signedOut} onClick={onPlay}>{t("home.chooseGame")}</button>
        </article>
        <article className="panel flex flex-col gap-3.5 p-5">
          <span className="num">3</span>
          <h2 className="section-title">{t("home.step3")}</h2>
          <p className="flex-1 text-dim">{t("home.step3Text")}</p>
          <p className="min-h-11 flex items-center text-[13px] text-dim">{t("home.noLimit")}</p>
        </article>
      </section>
      {!hasAccounts && (
        <p className="flex flex-wrap items-center gap-x-1 text-dim">
          {t("home.importLead")}
          <button className="btn btn-ghost px-2" onClick={onImport}>{t("home.import")}</button>
        </p>
      )}
    </>
  );
}

/** Every running client at a glance, with a picture of its window, and the window tools; with
 * nothing running, the way to the first game. */
export function DashboardView({ accounts, home, onLaunch, onAdd, onImport, onAccounts }: { accounts: AccountSummary[]; home: HomeTabs; onLaunch: () => void; onAdd: () => void; onImport: () => void; onAccounts: () => void }) {
  const setSettings = useSetSettings();
  const qc = useQueryClient();
  const list = useInstances().data ?? [];
  const running = list.filter(isRunning);
  const [order, setOrder] = useHomeOrder();
  const [detailed, setDetailed] = useHomeDetailed();
  const [sizes, setSize] = useHomeSizes();
  const shown = homeSorted(running, order);
  const drag = useGridDrag((from, over) => setOrder(reordered(order, shown.map((i) => i.accountId), from, over)));
  const metrics = useMetrics().data ?? [];
  const settings = useSettings().data;
  const afkOn = settings?.antiAfk.enabled ?? false;
  const now = useNow(30_000);
  const [menu, setMenu] = useState<MenuAt | null>(null);
  const closeMenu = useCallback(() => setMenu(null), []);
  // The key that opened the menu closes it again: the click already dismissed the popover.
  const openMenu = (at: MenuAt) => setMenu((m) => (at.beside !== undefined && m?.from === at.from ? null : at));
  const [limitsFor, setLimitsFor] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmClose, setConfirmClose] = useState(false);
  const [closing, setClosing] = useState(false);
  const [closingOne, setClosingOne] = useState<ReadonlySet<string>>(new Set());
  const [fps] = usePreviewFps();
  const byId = new Map(accounts.map((a) => [a.id, a]));
  const byInst = new Map(metrics.map((m) => [m.instanceId, m]));
  const inGame = running.filter((i) => i.state === "inGame").length;
  const troubles = list.filter((i) => i.relaunching && i.relaunchProblem);
  const coming = running.some((i) => i.state === "starting" || i.state === "joining" || (i.relaunching && i.state === "disconnected"));
  const limited = list.find((i) => i.id === limitsFor);
  const limitedAccount = limited && byId.get(limited.accountId);
  const limitedName = limited ? (limitedAccount ? accountName(limitedAccount) : t("dashboard.account", { id: limited.accountId })) : "";

  const games = [...new Set(running.flatMap((i) => (i.state !== "inGame" || i.placeId === null ? [] : [i.placeId])))].map((id) => qc.getQueryData<GameInfo>(qk.game(id))?.name).filter((n): n is string => !!n);

  const act = (f: () => Promise<unknown>) => () => {
    setBusy(true);
    f().catch(toastError).finally(() => setBusy(false));
  };
  const tile = (micro: boolean) =>
    act(() =>
      api.instancesTile(micro).then((n) => {
        if (n < running.length) toast(t("dashboard.laidOut", { count: n }));
      }),
    );
  /** Keyed by pid too: a close that reached only a bootstrapper ends with the client adopted
   * under a new pid, which can then be closed again. */
  const closeOne = (i: Instance, force: boolean) => {
    const key = closeKey(i);
    setClosingOne((s) => new Set(s).add(key));
    api.instanceClose(i.id, force).catch((e) => {
      setClosingOne((s) => {
        const next = new Set(s);
        next.delete(key);
        return next;
      });
      toastError(e);
    });
  };
  const closeAll = () => {
    setClosing(true);
    api
      .instancesCloseAll(false)
      .catch(toastError)
      .finally(() => {
        setClosing(false);
        setConfirmClose(false);
      });
  };

  return (
    <main className="scroll flex-1 min-w-0 px-8 pt-5 pb-12">
      <div className="enter flex flex-col gap-6">
        <AccountTabs accounts={accounts} home={home} onAdd={onAdd}>
          {running.length === 0 ? (
            <Welcome hasAccounts={accounts.length > 0} signedOut={accounts.length > 0 && accounts.every((a) => a.status !== "valid")} onAdd={onAdd} onImport={onImport} onPlay={onLaunch} onAccounts={onAccounts} />
          ) : (
            <>
              <header className="flex flex-wrap items-center justify-between gap-4">
                <div className="min-w-0">
                  <h1 className="page-title">{inGame ? t("home.playing", { count: inGame }) : coming ? t("home.gettingIn") : t("home.noneInGame")}</h1>
                  <p className="page-sub">
                    {games.length > 0 && <>{t("home.in", { games: fmtList(games) })} </>}
                    {settings?.autoRejoin ? t("home.autoRejoinOn") : t("home.autoRejoinOff")}
                    {settings && !settings.autoRejoin && (
                      <button className="btn btn-ghost btn-sm ms-1" onClick={() => setSettings.mutate({ autoRejoin: true })}>{t("launch.multi.turnOn")}</button>
                    )}
                  </p>
                </div>
                <div className="flex flex-wrap items-center gap-3">
                  <label className="flex items-center gap-2 text-dim">
                    {t("dashboard.detailedView")}
                    <Switch label={t("dashboard.detailedView")} checked={detailed} onChange={setDetailed} />
                  </label>
                  <button className="btn" disabled={busy} onClick={tile(false)} title={t("dashboard.tileHint")}>{t("dashboard.tile")}</button>
                  <button className="btn" disabled={busy} onClick={act(() => api.instancesShowAll("minimize"))}>{t("dashboard.minimizeAll")}</button>
                  <button className="btn btn-icon" popoverTarget="home-more" style={{ anchorName: "--home-more" }} aria-label={t("home.more")} title={t("home.more")}>
                    <Icon name="dots" />
                  </button>
                  <div id="home-more" popover="auto" className="menu" onKeyDown={menuKeys} style={{ positionAnchor: "--home-more", positionArea: "block-end span-inline-start", margin: "6px 0 0" }}>
                    <button className="menu-item" popoverTarget="home-more" popoverTargetAction="hide" disabled={busy} onClick={act(() => api.instancesShowAll("restore"))}>{t("dashboard.restoreAll")}</button>
                    <button className="menu-item" popoverTarget="home-more" popoverTargetAction="hide" disabled={busy} title={t("dashboard.tileMicroHint", { w: settings?.microWidth ?? 320, h: settings?.microHeight ?? 240 })} onClick={tile(true)}>{t("dashboard.tileMicro")}</button>
                    <button
                      className="menu-item"
                      popoverTarget="home-more"
                      popoverTargetAction="hide"
                      title={t("dashboard.refreshHint")}
                      onClick={() => {
                        for (const queryKey of [qk.instances, qk.metrics, qk.multi]) void queryClient.invalidateQueries({ queryKey });
                      }}
                    >
                      {t("dashboard.refresh")}
                    </button>
                    <div role="separator" className="h-px my-1 bg-line" />
                    <button className="menu-item text-orange" popoverTarget="home-more" popoverTargetAction="hide" onClick={() => setConfirmClose(true)}>{t("launch.running.closeAll")}</button>
                  </div>
                  <button className="btn btn-primary" onClick={onLaunch}>
                    <Icon name="play" size={12} fill />
                    {t("home.playMore")}
                  </button>
                </div>
              </header>
              {troubles.map((i) => <Trouble key={i.id} inst={i} account={byId.get(i.accountId)} />)}
              <PcPanel metrics={metrics} />
              <AfkPanel />
              <span className="sr-only" aria-live="polite">{drag.moved}</span>
              <section ref={drag.grid} aria-label={t("launch.running.title")} className="flex flex-wrap items-start gap-6">
                {shown.map((i) => (
                  <ClientCard key={i.id} inst={i} account={byId.get(i.accountId)} metrics={byInst.get(i.id)} now={now} afkOn={afkOn} defaultFps={settings?.fpsCap ?? 0} fps={fps} closing={closingOne.has(closeKey(i))} detailed={detailed} width={sizes[i.accountId]} onSize={(w) => setSize(i.accountId, w)} grip={drag.grip} onMenu={openMenu} />
                ))}
              </section>
            </>
          )}
          {running.length === 0 && accounts.length > 0 && <AfkPanel />}
          {running.length === 0 && troubles.map((i) => <Trouble key={i.id} inst={i} account={byId.get(i.accountId)} />)}
          <EndedPanel accounts={accounts} />
        </AccountTabs>
      </div>
      <ClientMenu at={menu} account={menu ? byId.get(menu.inst.accountId) : undefined} onClose={closeMenu} afkOn={afkOn} closing={closingOne} onLimits={setLimitsFor} onCloseClient={closeOne} />
      <LimitsDialog instanceId={limitsFor} name={limitedName} onClose={() => setLimitsFor(null)} />
      <ConfirmDialog
        open={confirmClose}
        onClose={() => !closing && setConfirmClose(false)}
        onConfirm={closeAll}
        busy={closing}
        title={t("launch.running.closeAllTitle")}
        text={t("launch.running.closeAllText", { count: running.length })}
        confirmLabel={t("launch.running.closeAll")}
      />
    </main>
  );
}
