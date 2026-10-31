import { useEffect, useMemo, useRef, useState, type CSSProperties, type Dispatch, type SetStateAction } from "react";
import { useQueryClient } from "@tanstack/react-query";
import {
  api, asError, type AccountSummary, type BulkProgress, type Favourite, type JoinCandidate, type LaunchInfo,
  type MultiInstanceStatus, type SameServer, type Settings,
} from "../../api";
import { publicBlock, qk, useBulk, useFavourites, useInstances, useMultiStatus, useNetwork, usePresence, useRobloxClient, useSetSettings, useSettings } from "../../data";
import { fmtNum, t, type Key } from "../../i18n";
import { NumberField, Segmented, Switch } from "../../ui/controls";
import { Icon } from "../../ui/Icon";
import { friendly, toast } from "../../ui/Toast";
import { accountName, Avatar, Status } from "../accounts/shared";
import { BatchSection } from "./Batch";
import { FavouriteDialog, FavouritesRow } from "./Favourites";
import { JoinDialog } from "./JoinDialog";
import { isLive, isRunning, MultiNotice } from "./Running";
import { draftFor, draftForUser, readDraft, resolveTarget, type Draft, type Tone } from "./target";

const LAST_KEY = "roalt.lastLaunchAccount";
export const MARK: Record<Tone, CSSProperties> = {
  ok: { background: "var(--accent-text)" },
  info: { background: "var(--blue)", borderRadius: 2 },
  bad: { background: "var(--orange)", transform: "rotate(45deg) scale(.85)" },
};
const LAUNCH_ERRORS: Record<string, Key> = {
  cookieInvalid: "launch.err.cookieInvalid",
  challenge: "launch.err.challenge",
  notFound: "launch.err.notFound",
};
/** One launch at a time, also across remounts of the view. */
let busy = false;
/** The FPS cap typed for the next launches (0: each account's own or the default), across remounts;
 * read when launching, so a value committed by the Launch click's blur counts. */
let lastFps = 0;

export function lastLaunchAccount() {
  try {
    return Number(localStorage.getItem(LAST_KEY)) || null;
  } catch {
    return null;
  }
}
function remember(id: number) {
  try {
    localStorage.setItem(LAST_KEY, String(id));
  } catch {
    // Only a convenience.
  }
}

/** Why several accounts can't launch right now, or null. */
/** Why a launch can't start now: launches go one at a time (a running batch blocks single
 * launches too), and several accounts need multi-instance. */
export function launchBlocked(several: boolean, multi: MultiInstanceStatus | undefined, batch: BulkProgress | null | undefined) {
  if (batch && !batch.done) return t("launch.batchRunning");
  if (several && !multi?.active) return (multi?.enabled && multi.message) || t("launch.needMulti");
  return null;
}

/** Toasts a started client and remembers its account for the next start. */
export function launched(a: AccountSummary, info: LaunchInfo) {
  remember(a.id);
  toast(t(info.viaBootstrapper ? "launch.launchedViaBootstrapper" : "launch.launched", { name: accountName(a) }));
  if (info.warning) toast(info.warning, "error");
}

export function launchFailed(e: unknown) {
  const err = asError(e);
  if (err.kind !== "cancelled") toast(LAUNCH_ERRORS[err.kind] ? t(LAUNCH_ERRORS[err.kind]) : friendly(err), "error");
}

export function LaunchView({ accounts, picked, setPicked, sameServer, setSameServer, draft, setDraft, step, setStep, onAdd, onServers, onPickFavourite, onHome, onAccounts, onSettings }: {
  accounts: AccountSummary[];
  picked: number[];
  setPicked: Dispatch<SetStateAction<number[]>>;
  sameServer: SameServer;
  setSameServer: (s: SameServer) => void;
  draft: Draft;
  setDraft: Dispatch<SetStateAction<Draft>>;
  step: string | null;
  setStep: (step: string | null) => void;
  onAdd: () => void;
  onServers: () => void;
  onPickFavourite: (f: Favourite) => void;
  onHome: () => void;
  onAccounts: () => void;
  onSettings: () => void;
}) {
  const qc = useQueryClient();
  const favourites = useFavourites();
  const multi = useMultiStatus().data;
  const batch = useBulk().data;
  const client = useRobloxClient().data;
  const settings = useSettings().data;
  const delayMs = settings?.launchDelayMs ?? 6000;
  const setSettings = useSetSettings();
  const pasted = useRef(false);
  const [joinOpen, setJoinOpen] = useState(false);
  const [saveOpen, setSaveOpen] = useState(false);
  const targetField = useRef<HTMLInputElement>(null);
  // Open when one of them is set: it applies to the next launch, so it stays in sight.
  const [who, setWho] = useState("");
  const [optionsOpen, setOptionsOpen] = useState(() => lastFps > 0 || sameServer !== "off");
  const [fps, setFpsState] = useState(lastFps);
  const setFps = (v: number) => setFpsState((lastFps = v));

  // An account with a client on its way or in a game can't start another.
  const instances = useInstances().data ?? [];
  const liveAccounts = new Set(instances.filter(isLive).map((i) => i.accountId));
  // In list order, which is also the bulk launch order.
  const chosen = accounts.filter((a) => a.status === "valid" && !liveAccounts.has(a.id) && picked.includes(a.id));
  const account = chosen[0] ?? null;
  const bulk = chosen.length > 1;
  // With multi-instance off, one more client ends the one that runs.
  const replaces = !bulk && !!account && instances.some((i) => isRunning(i) && i.accountId !== account.id);
  const validIds = useMemo(
    () => accounts.filter((a) => a.status === "valid" && !liveAccounts.has(a.id)).map((a) => a.id).sort((x, y) => x - y),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [accounts.map((a) => `${a.id}:${a.status}:${liveAccounts.has(a.id)}`).join()],
  );
  // ponytail: the first 100 only; one request per account each minute. Past that, tiles show Ready.
  const presence = usePresence(useMemo(() => validIds.slice(0, 100), [validIds])).data;
  const needle = who.trim().toLowerCase();
  const tiles = needle ? accounts.filter((a) => `${a.alias}\n${a.displayName}\n${a.username}`.toLowerCase().includes(needle)) : accounts;
  // Picked accounts that are all playing already: that's what the button can't do.
  const allPlaying = !chosen.length && accounts.some((a) => picked.includes(a.id) && liveAccounts.has(a.id));
  const signedOut = accounts.length > 0 && accounts.every((a) => a.status !== "valid");
  if (joinOpen && !account) setJoinOpen(false);

  // Debounced local parse; the result is kept with the text it belongs to.
  const { text } = draft;
  // Straight to pasting a link. The field can be below the fold: no jump.
  useEffect(() => {
    if (!text.trim()) targetField.current?.focus({ preventScroll: true });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (!text.trim() || draft.parsed?.text === text) return;
    const t = setTimeout(() => {
      api.targetParse(text).then(
        (value) => setDraft((d) => (d.text === text ? { ...d, parsed: { text, value } } : d)),
        () => {}, // launching parses again and shows the error
      );
    }, 150);
    return () => clearTimeout(t);
  }, [text]); // eslint-disable-line react-hooks/exhaustive-deps

  // A parse that hasn't caught up yet is launchable.
  const { parsed, place, look, usable: usableTarget, pending } = readDraft(draft);
  const fav = draft.favId === null ? undefined : favourites.data?.find((f) => f.id === draft.favId);
  const network = useNetwork().data;
  // Turned on but held up by a Roblox already running: another client would still replace it.
  const blocked = publicBlock(network, settings) ?? launchBlocked(bulk || (replaces && !!multi?.enabled), multi, batch);
  // Multi-instance has its own notice, with the way out.
  const otherBlock = publicBlock(network, settings) ?? (batch && !batch.done ? t("launch.batchRunning") : null);
  const canLaunch = !!account && (usableTarget || pending) && !step && !blocked;

  async function launch() {
    if (!canLaunch || !account || busy) return;
    const a = account;
    const ids = chosen.map((x) => x.id);
    const first = ids.length > 1 ? t("launch.step.batch") : t("launch.step.ticket");
    busy = true;
    setStep(first);
    let timer = 0;
    try {
      const target = await resolveTarget(draft, setDraft, (name) => setStep(t("launch.step.lookup", { name })));
      setStep(first);
      if (ids.length > 1) {
        // From the cache: a stagger committed by this click's blur hasn't re-rendered yet.
        const delay = qc.getQueryData<Settings>(qk.settings)?.launchDelayMs ?? delayMs;
        await api.bulkLaunch({ accountIds: ids, target, delayMs: delay, sameServer, favouriteId: draft.favId, fpsCap: lastFps || null });
      } else {
        timer = window.setTimeout(() => setStep(t("launch.step.starting")), 800);
        launched(a, await api.launch(a.id, target, draft.favId ?? undefined, lastFps));
      }
      if (draft.favId !== null) qc.invalidateQueries({ queryKey: qk.favourites });
    } catch (e) {
      launchFailed(e);
    } finally {
      clearTimeout(timer);
      busy = false;
      setStep(null);
    }
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "Enter" && !document.querySelector("dialog[open]")) {
        e.preventDefault();
        void launch();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const join = (c: JoinCandidate) => {
    setJoinOpen(false);
    const p = c.presence;
    setDraft(
      p.kind === "inGame" && p.gameId && p.placeId !== null
        ? draftFor({ kind: "server", placeId: p.placeId, jobId: p.gameId }, null, t("launch.withUser", { name: c.username }))
        : draftForUser({ userId: c.userId, username: c.username, displayName: c.displayName }),
    );
  };

  return (
    <main className="scroll flex-1 min-w-0 px-8 pt-5 pb-12">
      <div className="enter flex flex-col gap-6 max-w-[1000px]">
        <header>
          <h1 className="page-title">{t("app.nav.launch")}</h1>
          <p className="page-sub">{t("launch.subtitle")}</p>
        </header>

        <section aria-labelledby="play-who" className="panel flex flex-col gap-4 p-5">
          <div className="flex flex-wrap items-center gap-3">
            <span aria-hidden="true" className="num">1</span>
            <h2 id="play-who" className="section-title flex-[1_1_160px]">{t("play.who")}</h2>
            <span className="text-[13px] text-dim">{t("launch.pickedOfValid", { picked: chosen.length, count: validIds.length })}</span>
            {validIds.length > 1 && (
              <button className="btn btn-ghost" onClick={() => setPicked(chosen.length === validIds.length ? [] : validIds)}>
                {chosen.length === validIds.length ? t("launch.clear") : t("launch.selectAll")}
              </button>
            )}
          </div>
          {signedOut && (
            <div className="well flex flex-wrap items-center gap-3 px-4 py-3">
              <p className="flex-[1_1_240px] text-dim">{t("home.signedOutLead")}</p>
              <button className="btn" onClick={onAccounts}>{t("app.nav.accounts")}</button>
            </div>
          )}
          {accounts.length > 12 && (
            <input
              className="field"
              aria-label={t("accounts.filterLabel")}
              placeholder={t("accounts.filterPlaceholder")}
              spellCheck={false}
              value={who}
              onChange={(e) => setWho(e.target.value)}
            />
          )}
          {accounts.length === 0 ? (
            <div className="flex flex-wrap items-center gap-3.5">
              <p className="flex-[1_1_240px] text-dim">{t("launch.noAccounts")}</p>
              <button className="btn btn-primary" onClick={onAdd}>
                <Icon name="plus" size={14} />
                {t("accounts.add.title")}
              </button>
            </div>
          ) : (
            <div role="group" aria-labelledby="play-who" className="scroll grid gap-3.5 max-h-[440px] p-2 -m-2" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(min(250px, 100%), 1fr))" }}>
              {tiles.map((a) => {
                const valid = a.status === "valid";
                const playing = liveAccounts.has(a.id);
                return (
                  <button
                    key={a.id}
                    className="btn pick"
                    aria-pressed={valid && !playing && picked.includes(a.id)}
                    disabled={!valid || playing}
                    title={valid ? undefined : t("launch.onlyValid")}
                    // A pick that is playing goes with this one, or it would come back by itself when its client ends.
                    onClick={(e) => e.detail < 2 && setPicked((p) => (p.includes(a.id) ? p.filter((id) => id !== a.id) : [...p.filter((id) => !liveAccounts.has(id)), a.id]))}
                  >
                    <span aria-hidden="true" className="tick"><Icon name="check" size={14} /></span>
                    <Avatar account={a} size={36} />
                    <span className="flex-1 min-w-0 flex flex-col">
                      <span className="truncate">{accountName(a)}</span>
                      <span className="flex text-[13px] font-normal text-dim min-w-0">
                        {playing ? t("play.already") : <Status status={a.status} presence={presence?.get(a.id)} />}
                      </span>
                    </span>
                  </button>
                );
              })}
            </div>
          )}
        </section>

        <section aria-labelledby="play-where" className="panel flex flex-col gap-4 p-5">
          <div className="flex flex-wrap items-center gap-3">
            <span aria-hidden="true" className="num">2</span>
            <h2 id="play-where" className="section-title flex-[1_1_160px]">{t("play.where")}</h2>
            <button className="btn btn-ghost" disabled={!account} title={account ? t("launch.joinHint") : t("launch.pickAccountFirst")} onClick={() => setJoinOpen(true)}>
              <Icon name="accounts" size={14} />
              {t("launch.joinSomeone")}
            </button>
            <button className="btn btn-ghost" onClick={onServers}>
              <Icon name="servers" size={14} />
              {t("play.browse")}
            </button>
          </div>
          <div className="flex flex-col gap-2">
            <label className="label" htmlFor="launch-target">{t("launch.targetLabel")}</label>
            <input
              id="launch-target"
              ref={targetField}
              className="field mono"
              placeholder={t("launch.targetPlaceholder")}
              spellCheck={false}
              autoComplete="off"
              value={text}
              onPaste={() => {
                pasted.current = true;
              }}
              onKeyDown={() => {
                pasted.current = false;
              }}
              onChange={(e) => {
                const v = e.target.value;
                setDraft((d) => ({ ...d, text: v, favId: null, note: null }));
              }}
            />
            {text.trim() && (
              <div className="enter flex flex-wrap items-center gap-x-3 gap-y-1.5 min-h-11">
                <span aria-hidden="true" className={`w-2 h-2 flex-none rounded-full ${look ? "" : "bg-dim pulse"}`} style={look ? MARK[look.tone] : undefined} />
                <span className="flex items-baseline gap-3 min-w-0 flex-1" aria-live="polite">
                  {look ? (
                    <>
                      <span className="font-semibold flex-none">{look.kind}</span>
                      <span className={`mono text-[13px] truncate ${look.tone === "bad" ? "text-orange" : "text-dim"}`} title={look.detail}>
                        {look.detail}
                      </span>
                    </>
                  ) : (
                    <span className="text-dim">{t("launch.reading")}</span>
                  )}
                </span>
                {draft.note && <span className="tag flex-none max-w-[30%] truncate">{draft.note}</span>}
                <button
                  className="btn btn-sm flex-none"
                  disabled={!usableTarget}
                  style={fav ? { color: "var(--accent-text)" } : undefined}
                  onClick={() => setSaveOpen(true)}
                >
                  <Icon name="star" size={12} fill={!!fav} />
                  {fav ? t("launch.editFavourite") : t("launch.saveAsFavourite")}
                </button>
              </div>
            )}
            {parsed?.kind === "jobIdOnly" && (
              <label className="enter flex flex-col gap-1.5 mt-2 max-w-[320px]">
                <span className="label">{t("launch.placeId")}</span>
                <input
                  className="field mono"
                  ref={(el) => {
                    // Jump here after a pasted job ID, not while it is being typed.
                    if (el && pasted.current) {
                      pasted.current = false;
                      el.focus();
                    }
                  }}
                  inputMode="numeric"
                  spellCheck={false}
                  placeholder={t("launch.placeIdPlaceholder")}
                  value={draft.place}
                  aria-invalid={draft.place.trim() !== "" && place === null}
                  onChange={(e) => {
                    const v = e.target.value;
                    setDraft((d) => ({ ...d, place: v, favId: null, note: null }));
                  }}
                />
              </label>
            )}
          </div>
          <FavouritesRow accounts={accounts} active={draft.favId} onPick={onPickFavourite} />
        </section>

        <section aria-labelledby="play-ready" className="panel flex flex-col gap-4 p-5">
          <BatchSection accounts={accounts} onPick={setPicked} />
          <div className="flex items-center gap-3">
            <span aria-hidden="true" className="num">3</span>
            <h2 id="play-ready" className="section-title">{t("play.ready")}</h2>
          </div>
          <MultiNotice wanted={bulk} replaces={replaces} />
          {client && !client.path && (
            <div className="well flex flex-wrap items-center gap-3 px-4 py-3">
              <p className="alert flex-[1_1_240px]">{t("settings.robloxMissing")}</p>
              <button className="btn" onClick={onSettings}>{t("settings.installRoblox")}</button>
            </div>
          )}
          {allPlaying && (
            <div className="well flex flex-wrap items-center gap-3 px-4 py-3">
              <p className="flex-[1_1_240px] text-dim">{t("play.allPlaying")}</p>
              <button className="btn" onClick={onHome}>{t("app.nav.dashboard")}</button>
            </div>
          )}
          <button className="btn btn-primary btn-play" disabled={!canLaunch} title={t("launch.launchHint")} onClick={() => void launch()}>
            <Icon name="play" size={22} fill />
            <span className="truncate">
              {step
                ? t("launch.launching")
                : !account
                  ? t("play.pickFirst")
                  : !usableTarget && !pending
                    ? t("play.chooseWhere")
                    : bulk
                      ? t("launch.launchMany", { count: chosen.length })
                      : t("launch.launchAs", { name: accountName(account) })}
            </span>
          </button>
          <div className={step ? "live-bar" : "h-1"} />
          <div className="flex justify-center text-center text-[13px] text-dim" aria-live="polite">
            {step ? (
              <span className="text-text">{step}</span>
            ) : otherBlock ? (
              <span className="alert text-start">{otherBlock}</span>
            ) : bulk && client?.outdated ? (
              // Not blocking: Roblox updates itself on the first launch, which can end the clients after it.
              <span className="alert text-start" title={t("launch.robloxVersions", { installed: client.version ?? "", latest: client.latest ?? "" })}>
                {t("launch.robloxOutdated")}
              </span>
            ) : canLaunch && look ? (
              <span>
                {t("play.into", { target: look.kind })} {look.detail}
                {bulk && <> · {t("play.apart", { s: fmtNum(delayMs / 1000) })}</>}
                {fps > 0 && <> · {t("dashboard.fps", { fps })}</>}
                {bulk && sameServer !== "off" && <> · {t("launch.sameServer")}</>}
                {settings?.lowGraphics && !account?.fullQuality && <> · {t("settings.lowGraphics")}</>}
              </span>
            ) : (
              t("play.hint")
            )}
          </div>
          <button className="btn btn-ghost self-center" aria-expanded={optionsOpen} onClick={() => setOptionsOpen(!optionsOpen)}>
            {optionsOpen ? t("play.fewerOptions") : t("play.moreOptions")}
            <Icon name="chevron" size={12} className={optionsOpen ? "rotate-180" : ""} />
          </button>
          {optionsOpen && (
            <div className="enter well px-[18px] py-4">
              <div className="setting">
                <div className="setting-text">
                  <p className="font-medium">{t("launch.fpsCap")}</p>
                  <p className="text-[13px] text-dim">{t("launch.fpsHint")}</p>
                </div>
                <NumberField unit={t("launch.fpsUnit")} short="FPS" min={0} max={360} value={fps} onCommit={setFps} />
              </div>
              <div className="setting">
                <div className="setting-text">
                  <p className="font-medium">{t("launch.stagger")}</p>
                  <p className="text-[13px] text-dim">{t("play.staggerHint")}</p>
                </div>
                <NumberField
                  unit={t("launch.seconds")}
                  short={t("launch.secondsShort")}
                  min={2}
                  max={60}
                  step={0.5}
                  value={delayMs / 1000}
                  onCommit={(s) => setSettings.mutate({ launchDelayMs: Math.round(s * 1000) })}
                />
              </div>
              <div className="setting">
                <div className="setting-text">
                  <p className="font-medium">{t("launch.sameServer")}</p>
                  <p className="text-[13px] text-dim">{t("play.sameServerHint")}</p>
                </div>
                {sameServer !== "off" && (
                  <Segmented
                    label={t("launch.findServer")}
                    value={sameServer}
                    options={[{ value: "pickFirst", label: t("launch.pickServer") }, { value: "followFirst", label: t("launch.followFirst") }]}
                    onChange={setSameServer}
                  />
                )}
                <Switch label={t("launch.sameServer")} checked={sameServer !== "off"} onChange={(on) => setSameServer(on ? "pickFirst" : "off")} />
              </div>
              <div className="setting">
                <div className="setting-text">
                  <p className="font-medium">{t("launch.autoRejoin")}</p>
                  <p className="text-[13px] text-dim">{t("play.autoRejoinHint")}</p>
                </div>
                <Switch label={t("launch.autoRejoin")} checked={settings?.autoRejoin ?? false} onChange={(autoRejoin) => setSettings.mutate({ autoRejoin })} />
              </div>
            </div>
          )}
        </section>
      </div>

      <JoinDialog open={joinOpen} onClose={() => setJoinOpen(false)} account={account} accounts={accounts} onPick={join} />
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
        onSaved={(f) => setDraft((d) => (d.text === text ? { ...d, favId: f.id, note: f.name } : d))}
      />
    </main>
  );
}
