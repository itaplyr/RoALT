import { useEffect, useLayoutEffect, useRef, useState, type Dispatch, type ReactNode, type RefObject, type SetStateAction } from "react";
import { flushSync } from "react-dom";
import { AnimatePresence, motion } from "motion/react";
import { api, type AccountSummary, type Favourite, type JoinCandidate, type SameServer } from "../../api";
import { publicBlock, useBulk, useFavourites, useInstances, useMultiStatus, useNetwork, useRobloxClient, useSetSettings, useSettings } from "../../data";
import { fmtNum, t } from "../../i18n";
import { announce, overlayOpen } from "../../ui/a11y";
import { Disclosure } from "../../ui/Bits";
import { Button, IconButton } from "../../ui/Button";
import { NumberField, OffOrNumberField, Segmented, Switch } from "../../ui/controls";
import { Icon } from "../../ui/Icon";
import { Collapse, Morph, SPRING, Swap, TWEEN } from "../../ui/motion";
import { RANGES } from "../../ui/ranges";
import { toastError } from "../../ui/Toast";
import { accountName, Avatar } from "../accounts/shared";
import { AccountPicker } from "./AccountPicker";
import { FavouriteDialog, FavouritesRow } from "./Favourites";
import { JoinDialog } from "./JoinDialog";
import { LaunchButton } from "./LaunchButton";
import { launchBlocked, launchOne, launchSame, launchStep, sessionFps, useLaunchActivity, useSessionFps } from "./launchQueue";
import { LaunchProgress } from "./LaunchProgress";
import { MultiNotice } from "./MultiNotice";
import { useMoveConfirm } from "./MoveDialog";
import { isRunning } from "./Running";
import { ServerDrawer } from "./ServerDrawer";
import { TargetField } from "./TargetField";
import { draftFor, draftForUser, readDraft, resolveTarget, type Draft } from "./target";
import "./launch.css";


/** How long the layout preview stays on screen (`PREVIEW_FOR` in src-tauri/src/commands.rs). */
const PREVIEW_MS = 6000;
/** The composer's width that fits Who beside Where and Ready. */
const WIDE = 880;

/** Whether the composer has room for its two columns (its own width: the side layout's rail takes some). */
function useWide(ref: RefObject<HTMLElement | null>) {
  const [wide, setWide] = useState(true);
  useLayoutEffect(() => {
    const el = ref.current!;
    const check = () => setWide(el.clientWidth >= WIDE);
    check();
    const ro = new ResizeObserver(check);
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return wide;
}

/** A step's number; once it is done it turns into a check (a shape, not only a fill). */
function Step({ n, done = false }: { n: number; done?: boolean }) {
  return (
    <span aria-hidden="true" className={`num ${done ? "num-done" : ""}`}>
      <Swap k={done ? "done" : "n"} variant="blur">
        {done ? <Icon name="check" size={14} /> : n}
      </Swap>
    </span>
  );
}

/** One option: its name with a one-line hint (or a longer "More about" behind a toggle), and its controls. */
function Option({ id, label, hint, more, children, below }: { id: string; label: string; hint?: string; more?: string; children: ReactNode; below?: ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <div id={id} className="play-opt">
      <div className="play-opt-line">
        <div className="play-opt-text">
          <p className="flex items-center gap-1 min-h-7 font-medium">
            {label}
            {more && (
              <IconButton icon="info" size="sm" label={t("settings.moreAbout", { label })} aria-expanded={open} aria-controls={open ? `${id}-more` : undefined} onClick={() => setOpen(!open)} />
            )}
          </p>
          {hint && <p className="text-[13px] text-fg-3">{hint}</p>}
        </div>
        <div className="flex flex-wrap items-center gap-2.5">{children}</div>
      </div>
      {more && (
        <Collapse open={open}>
          <p id={`${id}-more`} className="play-opt-more">{more}</p>
        </Collapse>
      )}
      {below}
    </div>
  );
}

type Chip = { key: string; text: string; edit?: string };

/** Play: who plays (left), where and the launch (right). Two columns when there is room, else stacked. */
export function LaunchView({ accounts, picked, setPicked, sameServer, setSameServer, draft, setDraft, onAdd, onServers, onPickFavourite, onHome, onAccounts, onSettings }: {
  accounts: AccountSummary[];
  picked: number[];
  setPicked: Dispatch<SetStateAction<number[]>>;
  sameServer: SameServer;
  setSameServer: (s: SameServer) => void;
  draft: Draft;
  setDraft: Dispatch<SetStateAction<Draft>>;
  onAdd: () => void;
  /** The whole Servers view (the drawer's way there). */
  onServers: () => void;
  onPickFavourite: (f: Favourite) => void;
  onHome: () => void;
  onAccounts: () => void;
  onSettings: () => void;
}) {
  const favourites = useFavourites();
  const multi = useMultiStatus().data;
  const batch = useBulk().data;
  const client = useRobloxClient().data;
  const settings = useSettings().data;
  const network = useNetwork().data;
  const delayMs = settings?.launchDelayMs ?? 6000;
  const layout = settings?.windowLayout ?? "off";
  const setSettings = useSetSettings();
  const activity = useLaunchActivity();
  const [joinOpen, setJoinOpen] = useState(false);
  const [saveOpen, setSaveOpen] = useState(false);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [fps, setFps] = useSessionFps();
  // Open when one of them is set: it applies to the next launch, so it stays in sight.
  const [optionsOpen, setOptionsOpen] = useState(() => fps > 0 || sameServer !== "off");
  // One choice, two switches: all in one server, or each in a different one.
  const together = sameServer === "pickFirst" || sameServer === "followFirst";
  const spread = sameServer !== "off" && !together;
  const [delayError, setDelayError] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const options = useRef<HTMLDivElement>(null);
  const foot = useRef<HTMLDivElement>(null);
  const wide = useWide(root);

  const instances = useInstances().data ?? [];
  // In list order, which is also the bulk launch order. A playing account is only ever picked on its
  // own (AccountPicker): launching it moves its client, asked first.
  const has = new Set(picked);
  const chosen = accounts.filter((a) => a.status === "valid" && has.has(a.id));
  const account = chosen[0] ?? null;
  const move = useMoveConfirm();
  // Who asks Roblox (friends to join, the signed-in sorts, private servers): the first one picked, else any signed in.
  const asker = account ?? accounts.find((a) => a.status === "valid") ?? null;
  const bulk = chosen.length > 1;
  // With multi-instance off, one more client ends the one that runs.
  const replaces = !bulk && !!account && instances.some((i) => isRunning(i) && i.accountId !== account.id);
  // Stacked, a Who with picks folds into one line until opened, so Where and Play are in reach.
  const [whoOpen, setWhoOpen] = useState(() => chosen.length === 0);
  const folded = !wide && !whoOpen && chosen.length > 0;
  const openWho = () => {
    flushSync(() => setWhoOpen(true));
    // The summary goes: the focus moves on into the picker it opened.
    root.current?.querySelector<HTMLElement>(".play-picker input")?.focus();
  };
  // Folded, the picker's search isn't there for Ctrl+F (the shell would go to Accounts): it opens.
  useEffect(() => {
    if (!folded) return;
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey || e.key.toLowerCase() !== "f" || overlayOpen()) return;
      e.preventDefault();
      e.stopPropagation();
      openWho();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [folded]); // eslint-disable-line react-hooks/exhaustive-deps

  // A parse that hasn't caught up yet is launchable.
  const { parsed, look, usable: usableTarget, pending } = readDraft(draft);
  const fav = draft.favId === null ? undefined : favourites.data?.find((f) => f.id === draft.favId);
  const pub = publicBlock(network, settings);
  // Turned on but held up by a Roblox already running: another client would still replace it.
  const blocked = pub ?? launchBlocked(bulk || (replaces && !!multi?.enabled), multi, batch);
  // The button's own line says what no notice beside it already does (multi-instance and playing
  // accounts have their notices, with the way out); pressing it says that too.
  const reason = pub ?? (batch && !batch.done ? t("launch.batchRunning") : null);
  const noted = !reason && blocked ? blocked : null;
  const canLaunch = !!account && (usableTarget || pending) && !blocked;
  // A launch of several under way has the button's place; one account's shows under it.
  const inPlace = !!activity && !activity.done && activity.rows.length > 1;

  // While the layout preview is on screen (it goes by itself after PREVIEW_MS, as the backend's timer).
  const [previewing, setPreviewing] = useState(false);
  const previewTimer = useRef(0);
  useEffect(() => () => clearTimeout(previewTimer.current), []);
  /** The picked accounts' spots, in launch (list) order, drawn over the screen for a few seconds. */
  const previewLayout = () =>
    void api.layoutPreview(chosen.map((a) => a.id), t("launch.layoutPreviewRunning")).then(() => {
      setPreviewing(true);
      clearTimeout(previewTimer.current);
      previewTimer.current = window.setTimeout(() => setPreviewing(false), PREVIEW_MS);
    }, toastError);
  const stopPreview = () => {
    clearTimeout(previewTimer.current);
    setPreviewing(false);
    void api.layoutPreviewStop().catch(toastError);
  };

  // The FPS cap is read now: a value committed by this click's blur counts.
  const launch = () => {
    if (!account) return;
    const where = { draft, setDraft };
    if (bulk) void launchSame(chosen, where, { sameServer, fpsCap: sessionFps() || null });
    else if (move.liveOf(account)) move.ask(account, (from) => void launchOne(account, where, sessionFps(), from));
    else void launchOne(account, where, sessionFps());
  };

  // While the progress has the button's place, Ctrl+Enter still says why nothing starts.
  useEffect(() => {
    if (!inPlace) return;
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key === "Enter" && !overlayOpen()) {
        e.preventDefault();
        announce(launchStep() ?? t("launch.batchRunning"), "assertive");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [inPlace]);

  // The button and the launch's progress take turns at the foot, and the progress folds and goes: the
  // focus goes with them (Cancel, else the button), unless it is somewhere else.
  const turn = inPlace ? "progress" : activity ? `${activity.done}` : "none";
  const lastTurn = useRef(turn);
  useEffect(() => {
    if (lastTurn.current === turn) return;
    lastTurn.current = turn;
    const el = document.activeElement;
    const box = foot.current;
    const lost = !el || el === document.body || el.matches("main") || (!!box?.contains(el) && !!el.closest(".launch-slot, .launch-progress"));
    if (lost) box?.querySelector<HTMLElement>(inPlace ? ".launch-cancel" : ".launch-btn")?.focus();
  }, [turn]); // eslint-disable-line react-hooks/exhaustive-deps

  const join = (c: JoinCandidate) => {
    setJoinOpen(false);
    const p = c.presence;
    setDraft(
      p.kind === "inGame" && p.gameId && p.placeId !== null
        ? draftFor({ kind: "server", placeId: p.placeId, jobId: p.gameId }, null, t("launch.withUser", { name: c.username }))
        : draftForUser({ userId: c.userId, username: c.username, displayName: c.displayName }),
    );
  };

  // What this launch will do, each chip a way to the option it reads.
  const layoutOn = layout !== "off" && (bulk || !account?.fullQuality);
  const chips = [
    usableTarget && look ? { key: "target", text: `${look.kind} · ${look.detail}`, edit: "#launch-target" } : null,
    bulk ? { key: "delay", text: t("play.apart", { s: fmtNum(delayMs / 1000) }), edit: "#play-opt-delay input" } : null,
    fps > 0 ? { key: "fps", text: t("dashboard.fps", { fps }), edit: "#play-opt-fps input" } : null,
    bulk && together ? { key: "same", text: t("launch.sameServer"), edit: "#play-opt-same [role=switch]" } : null,
    bulk && spread ? { key: "spread", text: t("launch.spread"), edit: "#play-opt-spread [role=switch]" } : null,
    layoutOn ? { key: "layout", text: `${t("launch.layout")} · ${t(layout === "fit" ? "launch.layoutFit" : "launch.layoutTiny")}`, edit: "#play-opt-layout [aria-pressed=true]" } : null,
    settings?.lowGraphics && !account?.fullQuality ? { key: "low", text: t("settings.lowGraphics") } : null,
  ].filter((c): c is Chip => c !== null);
  const edit = (selector: string) => {
    const details = options.current?.querySelector("details");
    if (details && selector !== "#launch-target") details.open = true;
    document.querySelector<HTMLElement>(selector)?.focus();
  };
  const seed = parsed?.kind === "target" && "placeId" in parsed.target ? parsed.target.placeId : null;
  const names = chosen.slice(0, 2).map(accountName).join(", ") + (chosen.length > 2 ? ` +${chosen.length - 2}` : "");

  return (
    <main className="scroll flex-1 min-w-0 page pb-0">
      <div ref={root} className="play" data-wide={wide || undefined}>
        <h1 className="page-title">{t("app.nav.launch")}</h1>
        <div className="play-grid">
          <section aria-labelledby="play-who" className="panel play-col play-who">
            <div className="play-head">
              <Step n={1} done={chosen.length > 0} />
              <h2 id="play-who" className="section-title">{t("play.who")}</h2>
            </div>
            <Collapse open={folded}>
              <button
                type="button"
                className="play-who-sum"
                aria-expanded={false}
                onClick={openWho}
              >
                <span className="play-faces">
                  {chosen.slice(0, 3).map((a) => (
                    <Avatar key={a.id} account={a} size={26} />
                  ))}
                </span>
                <span className="truncate">{names}</span>
                <Icon name="chevron" size={14} className="ms-auto text-fg-3" />
              </button>
            </Collapse>
            <Collapse open={!folded} className="flex flex-col min-h-0">
              <AccountPicker variant="tiles" accounts={accounts} picked={picked} onPicked={setPicked} label={t("play.who")} onAdd={onAdd} onAccounts={onAccounts} className="play-picker min-h-0" />
            </Collapse>
          </section>

          <div className="panel play-col">
            <div className="play-scroll">
              <section aria-labelledby="play-where" className="flex flex-col gap-4">
                <div className="play-head">
                  <Step n={2} done={usableTarget} />
                  <h2 id="play-where" className="section-title flex-[1_1_120px]">{t("play.where")}</h2>
                  <span className="flex flex-wrap gap-1">
                    <Button variant="ghost" size="md" icon="accounts" onClick={() => setJoinOpen(true)}>{t("launch.joinSomeone")}</Button>
                    <Button variant="ghost" size="md" icon="servers" onClick={() => setDrawerOpen(true)}>{t("play.browse")}</Button>
                  </span>
                </div>
                <FavouritesRow accounts={accounts} active={draft.favId} onPick={onPickFavourite} />
                <TargetField
                  draft={draft}
                  setDraft={setDraft}
                  autoFocus
                  action={
                    <Button size="md" className="flex-none" disabled={!usableTarget} onClick={() => setSaveOpen(true)}>
                      <Icon name="star" size={14} fill={!!fav} />
                      {fav ? t("launch.editFavourite") : t("launch.saveAsFavourite")}
                    </Button>
                  }
                />
              </section>

              <section aria-labelledby="play-ready" className="play-ready">
                <div className="play-head">
                  <Step n={3} />
                  <h2 id="play-ready" className="section-title">{t("play.ready")}</h2>
                </div>
                <Collapse open={chips.length > 0}>
                  <div className="play-chips">
                    <AnimatePresence mode="popLayout" initial={false}>
                      {chips.map((c) => (
                        <motion.span
                          key={c.key}
                          layout="position"
                          className="inline-flex max-w-full"
                          initial={{ opacity: 0, scale: 0.9 }}
                          animate={{ opacity: 1, scale: 1 }}
                          exit={{ opacity: 0, scale: 0.9, transition: TWEEN.exit }}
                          transition={SPRING.snappy}
                        >
                          {c.edit ? (
                            <button type="button" className="chip chip-sm max-w-full" onClick={() => edit(c.edit!)}>
                              <span className="truncate">{c.text}</span>
                            </button>
                          ) : (
                            <span className="tag max-w-full min-h-[var(--h-sm)] px-2.5">
                              <span className="truncate">{c.text}</span>
                            </span>
                          )}
                        </motion.span>
                      ))}
                    </AnimatePresence>
                  </div>
                </Collapse>
                <div ref={options}>
                  <Disclosure className="play-options" summary={optionsOpen ? t("play.fewerOptions") : t("play.moreOptions")} defaultOpen={optionsOpen} onToggle={setOptionsOpen}>
                    <div role="group" aria-labelledby="play-this" className="play-opt-group">
                      <h3 id="play-this" className="play-opt-head">{t("play.thisLaunch")}</h3>
                      <Option id="play-opt-fps" label={t("launch.fpsCap")} hint={t("launch.fpsHint")}>
                        <OffOrNumberField label={t("launch.fpsCap")} value={fps} range={RANGES.fpsCap} unit="FPS" short="FPS" onCommit={setFps} />
                      </Option>
                      <Option
                        id="play-opt-same"
                        label={t("launch.sameServer")}
                        hint={t("play.sameServerHint")}
                        below={
                          <Collapse open={together}>
                            <div className="pt-2.5">
                              <Segmented
                                label={t("launch.findServer")}
                                value={together ? sameServer : "pickFirst"}
                                options={[{ value: "pickFirst", label: t("launch.pickServer") }, { value: "followFirst", label: t("launch.followFirst") }]}
                                onChange={setSameServer}
                              />
                            </div>
                          </Collapse>
                        }
                      >
                        <Switch label={t("launch.sameServer")} checked={together} onChange={(on) => setSameServer(on ? "pickFirst" : "off")} />
                      </Option>
                      <Option
                        id="play-opt-spread"
                        label={t("launch.spread")}
                        hint={t("play.spreadHint")}
                        below={
                          <Collapse open={spread}>
                            <div className="pt-2.5">
                              <Segmented
                                label={t("launch.findServer")}
                                value={spread ? sameServer : "randomPublic"}
                                options={[
                                  { value: "ownPrivate", label: t("launch.spreadOwn") },
                                  { value: "randomPublic", label: t("launch.spreadRandom") },
                                  { value: "closest", label: t("launch.spreadClosest") },
                                  { value: "mostFull", label: t("servers.orderMostFull") },
                                  { value: "mostEmpty", label: t("servers.orderMostEmpty") },
                                ]}
                                onChange={setSameServer}
                              />
                            </div>
                          </Collapse>
                        }
                      >
                        <Switch label={t("launch.spread")} checked={spread} onChange={(on) => setSameServer(on ? "randomPublic" : "off")} />
                      </Option>
                    </div>
                    <div role="group" aria-labelledby="play-every" className="play-opt-group">
                      <h3 id="play-every" className="play-opt-head">{t("play.everyLaunch")}</h3>
                      <Option
                        id="play-opt-delay"
                        label={t("launch.stagger")}
                        hint={t("play.staggerHint")}
                        below={
                          delayError && (
                            <p id="play-opt-delay-error" role="alert" className="pt-1.5 text-xs text-attention">
                              {delayError}
                            </p>
                          )
                        }
                      >
                        <NumberField
                          name={t("launch.stagger")}
                          onInvalid={setDelayError}
                          describedBy={delayError ? "play-opt-delay-error" : undefined}
                          unit={t("launch.seconds")}
                          short={t("launch.secondsShort")}
                          min={2}
                          max={60}
                          step={0.5}
                          value={delayMs / 1000}
                          onCommit={(s) => setSettings.mutate({ launchDelayMs: Math.round(s * 1000) })}
                        />
                      </Option>
                      <Option id="play-opt-layout" label={t("launch.layout")} more={t("launch.layoutHint")}>
                        <Segmented
                          label={t("launch.layout")}
                          value={layout}
                          options={[
                            { value: "off", label: t("launch.layoutOff") },
                            { value: "fit", label: t("launch.layoutFit") },
                            { value: "tiny", label: t("launch.layoutTiny") },
                          ]}
                          // Picking Fit or Tiny shows where the windows will go right away.
                          onChange={(windowLayout) => setSettings.mutate({ windowLayout }, { onSuccess: () => windowLayout !== "off" && chosen.length > 0 && previewLayout() })}
                        />
                        {layout !== "off" && (
                          <Button size="md" className="enter" icon={previewing ? "eyeOff" : "eye"} data-tip={previewing ? undefined : t("launch.layoutPreviewHint")} onClick={previewing ? stopPreview : previewLayout}>
                            {previewing ? t("launch.layoutPreviewStop") : t("launch.layoutPreview")}
                          </Button>
                        )}
                      </Option>
                      <Option id="play-opt-rejoin" label={t("launch.autoRejoin")} hint={t("play.autoRejoinHint")}>
                        <Switch label={t("launch.autoRejoin")} checked={settings?.autoRejoin ?? false} onChange={(autoRejoin) => setSettings.mutate({ autoRejoin })} />
                      </Option>
                    </div>
                  </Disclosure>
                </div>
              </section>
            </div>

            <div ref={foot} className="play-go">
              {/* Whatever stands in the launch's way, with the way out, right above the button. */}
              <Morph axis="height">
                <div className="flex flex-col gap-2.5 has-[.notice,.alert]:pb-1">
                  <MultiNotice wanted={bulk} replaces={replaces} />
                  {client && !client.path && (
                    <div className="notice enter" data-tone="attention">
                      <p className="alert flex-[1_1_240px] min-w-0">{t("settings.robloxMissing")}</p>
                      <Button size="md" onClick={onSettings}>{t("settings.installRoblox")}</Button>
                    </div>
                  )}
                  {!bulk && account && move.liveOf(account) && (
                    <div className="notice enter">
                      <p className="flex-[1_1_240px] min-w-0 text-fg-2">{t("move.notice", { name: accountName(account) })}</p>
                      <Button size="md" icon="home" onClick={onHome}>{t("app.nav.dashboard")}</Button>
                    </div>
                  )}
                  {canLaunch && bulk && client?.outdated && (
                    // Not blocking: Roblox updates itself on the first launch, which can end the clients after it.
                    <p className="alert enter text-[13px]" data-tip={t("launch.robloxVersions", { installed: client.version ?? "", latest: client.latest ?? "" })}>
                      {t("launch.robloxOutdated")}
                    </p>
                  )}
                </div>
              </Morph>
              <Collapse open={!inPlace} className="-mx-2 px-2">
                <LaunchButton
                  big
                  shortcut
                  label={!account ? t("play.pickFirst") : !usableTarget && !pending ? t("play.chooseWhere") : bulk ? t("launch.launchMany", { count: chosen.length }) : t("launch.launchAs", { name: accountName(account) })}
                  disabled={!canLaunch}
                  reason={reason}
                  noted={noted}
                  onLaunch={launch}
                />
              </Collapse>
              <LaunchProgress onPick={setPicked} />
            </div>
          </div>
        </div>
      </div>

      {move.dialog}
      <JoinDialog open={joinOpen} onClose={() => setJoinOpen(false)} account={asker} accounts={accounts} onPick={join} />
      <ServerDrawer
        open={drawerOpen}
        onClose={() => setDrawerOpen(false)}
        accounts={accounts}
        asker={asker}
        count={chosen.length}
        seed={seed}
        onUse={setDraft}
        onServers={onServers}
      />
      <FavouriteDialog
        open={saveOpen}
        onClose={() => setSaveOpen(false)}
        accounts={accounts}
        initial={
          fav ?? {
            name: look ? `${look.kind} · ${look.detail}`.replace(/ · [^·]*••••/, "").slice(0, 60) : "",
            accountIds: accounts.filter((a) => a.status === "valid" && picked.includes(a.id)).map((a) => a.id),
            inTray: false,
          }
        }
        resolve={() => (fav ? Promise.resolve(fav.target) : resolveTarget(draft, setDraft))}
        onSaved={(f) => setDraft((d) => (d.text === draft.text ? { ...d, favId: f.id, note: f.name } : d))}
      />
    </main>
  );
}
