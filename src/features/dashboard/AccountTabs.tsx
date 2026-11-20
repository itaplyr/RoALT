import { useEffect, useLayoutEffect, useMemo, useRef, useState, type Dispatch, type KeyboardEvent, type ReactNode, type SetStateAction } from "react";
import { AnimatePresence, motion } from "motion/react";
import { api, type AccountSummary, type Instance } from "../../api";
import { publicBlock, useBulk, useInstances, useMultiStatus, useNetwork, useSettings } from "../../data";
import { isRtl, t } from "../../i18n";
import { Disclosure } from "../../ui/Bits";
import { Button, IconButton } from "../../ui/Button";
import { riseIn, SPRING, TWEEN, useFlip, useFreshItems, useLiquid } from "../../ui/motion";
import { Popover, useAnchor } from "../../ui/Popover";
import { StatusDot, StatusPill } from "../../ui/Status";
import { announce } from "../../ui/a11y";
import { toastError } from "../../ui/Toast";
import { DetailOverrides } from "../accounts/DetailOverrides";
import { accountName, Avatar } from "../accounts/shared";
import { AccountPicker } from "../launch/AccountPicker";
import { FavouritesRow } from "../launch/Favourites";
import { LaunchButton } from "../launch/LaunchButton";
import { launchBlocked, launchEach, launchOne, useLaunchActivity } from "../launch/launchQueue";
import { MultiNotice } from "../launch/MultiNotice";
import { isLive, isRunning, STATE_TONE, STATES } from "../launch/Running";
import { TargetField } from "../launch/TargetField";
import { draftFor, readDraft, type Draft } from "../launch/target";
import { useNewsFeed } from "../news/news";
import { NewsView } from "../news/NewsView";
import { homeSorted, readyTabs, storedText, tabLaunch, type HomeActive, type HomeTab, type HomeTabs } from "./homeLayout";
import { AccountColumn } from "./AccountColumn";
import { Sortable } from "./Sortable";

const ADD = "home-tabs-add";
const tabId = (id: HomeActive) => `home-tab-${id ?? "overview"}`;

/** Arrows, Home and End move between the tabs and open them. */
function tabKeys(e: KeyboardEvent<HTMLElement>) {
  const all = [...e.currentTarget.querySelectorAll<HTMLElement>("[role=tab]")];
  const at = all.indexOf(document.activeElement as HTMLElement);
  const fore = isRtl() ? -1 : 1;
  const next = at < 0 ? null : e.key === "ArrowRight" ? at + fore : e.key === "ArrowLeft" ? at - fore : e.key === "Home" ? 0 : e.key === "End" ? all.length - 1 : null;
  if (next === null) return;
  e.preventDefault();
  const to = all[(next + all.length) % all.length];
  to.focus();
  to.click();
}

/** A tab's or rail row's state: its launch while one is under way or failed, else its client's. */
export function LaunchMark({ inst, status }: { inst: Pick<Instance, "state"> | undefined; status: HomeTab["status"] }) {
  if (status === "queued" || status === "launching") return <StatusDot tone="neutral" label={status === "queued" ? t("dashboard.tabs.queued") : t("launch.launching")} pulse />;
  if (inst) return <StatusDot tone={STATE_TONE[inst.state]} label={STATES[inst.state][0]} pulse={inst.state === "starting" || inst.state === "joining"} />;
  return status === "failed" ? <StatusDot tone="attention" label={t("dashboard.tabs.failed")} /> : null;
}

/** "Play with N" will start this tab: a ring pops in. `still`: one of a long list, which doesn't move. */
export function ReadyRing({ on, still }: { on: boolean; still?: boolean }) {
  const label = t("dashboard.tabs.ready");
  if (still) return on ? <span className="home-ready" data-tip={label}><span className="sr-only">{label}</span></span> : null;
  return (
    <AnimatePresence initial={false}>
      {on && (
        <motion.span
          key="ready"
          className="home-ready"
          data-tip={label}
          initial={{ opacity: 0, scale: 0.5 }}
          animate={{ opacity: 1, scale: 1, transition: SPRING.snappy }}
          exit={{ opacity: 0, scale: 0.5, transition: TWEEN.exit }}
        >
          <span className="sr-only">{label}</span>
        </motion.span>
      )}
    </AnimatePresence>
  );
}

/** One account's tab: where it plays, and its launch or its window; its own options fold away. */
function TabBody({ tab, account: a, accounts, instances, status, block, why, onDraft }: {
  tab: HomeTab;
  account: AccountSummary;
  accounts: AccountSummary[];
  instances: Instance[];
  status: HomeTab["status"];
  /** Why no launch can start now, or null; `why`: the same unless a notice above already says it. */
  block: string | null;
  why: string | null;
  onDraft: Dispatch<SetStateAction<Draft>>;
}) {
  const { draft } = tab;
  const { usable, pending } = readDraft(draft);
  const mine = instances.filter((i) => i.accountId === a.id);
  const inst = mine.find(isLive) ?? mine.find(isRunning);
  const live = !!inst && isLive(inst);
  const busy = status === "queued" || status === "launching";
  const name = accountName(a);
  const valid = a.status === "valid";
  const can = usable || pending;
  // On screen only, because it can't be read back after a restart.
  const unsaved = usable && !pending && !storedText(draft);
  // Only as it opens: Disclosure's `open` follows its prop, and clearing the last option would fold it
  // away under the focus.
  const [own] = useState(() => !!a.fpsCap || !!a.launchArgs || a.fullQuality || a.allowDisconnect || a.discordMuted || Object.keys(a.customFlags).length > 0);
  return (
    <div className="home-tab-cols">
      <section aria-labelledby="home-tab-where" className="panel home-tab-body">
        <div className="flex flex-wrap items-start gap-x-3 gap-y-2">
          <Avatar account={a} size={40} />
          <div className="flex-[1_1_160px] min-w-0 flex flex-col">
            <h2 id="home-tab-where" className="section-title truncate">{name}</h2>
            <span className="text-[13px] text-fg-2 truncate">@{a.username}</span>
          </div>
          {inst && (
            <span className="flex items-center gap-2 min-h-10">
              <StatusPill tone={STATE_TONE[inst.state]} label={STATES[inst.state][0]} pulse={inst.state === "starting" || inst.state === "joining"} />
              <Button size="md" icon="eye" onClick={() => api.instanceShow(inst.id, "focus").catch(toastError)}>
                {t("home.show")}
              </Button>
            </span>
          )}
          {!live && (
            <LaunchButton
              label={can ? t("launch.launchAs", { name }) : t("play.chooseWhere")}
              disabled={!valid || !can || block !== null}
              reason={!valid ? t("launch.onlyValid") : can ? why : null}
              shortcut
              onLaunch={() => void launchOne(a, { draft, setDraft: onDraft })}
            />
          )}
        </div>
        <p role="status" className="empty:hidden text-[13px]">
          {status === "queued" ? <span className="text-fg-2">{t("dashboard.tabs.queued")}</span> : status === "failed" ? <span className="alert">{t("dashboard.tabs.failed")}</span> : null}
        </p>
        <TargetField id="home-tab-target" draft={draft} setDraft={onDraft} disabled={busy} />
        {unsaved && <p className="-mt-2 text-[13px] text-fg-3">{t("dashboard.tabs.notSaved")}</p>}
        <FavouritesRow accounts={accounts} active={draft.favId} onPick={(f) => !busy && onDraft(draftFor(f.target, f.id, f.name))} />
        <Disclosure summary={t("dashboard.tabs.options")} defaultOpen={own} className="home-tab-opts">
          <DetailOverrides account={a} />
        </Disclosure>
      </section>
      <AccountColumn account={a} inst={inst} />
    </div>
  );
}

/** Home's tabs (in the side layout the rail stands in for the strip): News, Overview, one tab per
 * added account, each with its own game and options, "+", and a launch of every ready tab one after
 * another. `children` is Overview (what Home shows without a tab open); beside the rail it gets
 * "Play with N" for its header. */
export function AccountTabs({ accounts, home, onAdd, children }: { accounts: AccountSummary[]; home: HomeTabs; onAdd: () => void; children: (extra: ReactNode) => ReactNode }) {
  const instances = useInstances().data ?? [];
  const multi = useMultiStatus().data;
  const batch = useBulk().data;
  const settings = useSettings().data;
  const network = useNetwork().data;
  const activity = useLaunchActivity();
  const side = settings?.tabLayout === "side";
  const strip = useRef<HTMLDivElement>(null);
  // The selected tab's light: one drop that flows along the strip.
  const tabDrop = useRef<HTMLSpanElement>(null);
  useLiquid(strip, tabDrop, ".tab:has(> [role=tab][aria-selected=true])", undefined, side);
  // Sortable's own row holds the tabs: that is what glides.
  const row = useMemo(() => ({ get current() { return strip.current?.querySelector<HTMLElement>(":scope > .sort-row") ?? null; } }), []);
  const anchor = useAnchor();
  // Each opening of "+" starts from an empty search.
  const [opened, setOpened] = useState(0);
  const [shown, setShown] = useState(false);
  const byId = new Map(accounts.map((a) => [a.id, a]));
  const { active } = home;
  // In the order they were dragged to, which "Play with N" goes by too.
  const tabs = homeSorted(home.tabs, home.order).filter((tab) => byId.has(tab.accountId));
  const ids = tabs.map((tab) => tab.accountId);
  const hold = useFlip(row, { key: ids.join() });
  const fresh = useFreshItems(ids);
  const open = tabs.find((tab) => tab.accountId === active);
  const ready = readyTabs(tabs, byId, instances);
  const liveOf = (id: number) => instances.find((i) => i.accountId === id && isLive(i));
  const statusOf = (id: number) => tabLaunch(activity, id, !!liveOf(id));
  // With multi-instance off, one more client ends the one that runs.
  const replaces = (tab: HomeTab) => instances.some((i) => isRunning(i) && i.accountId !== tab.accountId);
  const allReplace = ready.some(replaces);
  const blockFor = (several: boolean, replacing: boolean) => publicBlock(network, settings) ?? launchBlocked(several || (replacing && !!multi?.enabled), multi, batch);
  const allBlock = blockFor(ready.length > 1, allReplace);
  // The multi-instance notice under the strip says that one, with its ways out: not again.
  const multiWhy = launchBlocked(true, multi, null);
  const said = (why: string | null) => (why === multiWhy ? null : why);
  const free = accounts.filter((a) => !ids.includes(a.id));
  const news = useNewsFeed().unread;
  // News is always there, so the strip is too.
  const showStrip = !side;
  /** After a tab was added or closed, once that is on screen: what had the focus is gone. */
  const focusSelected = () => requestAnimationFrame(() => strip.current?.querySelector<HTMLElement>("[role=tab][aria-selected=true]")?.focus());
  const hideAdd = () => document.getElementById(ADD)?.hidePopover();

  // The wheel runs along the tabs; at either end it scrolls the page as usual. Not onWheel: React's
  // is passive, and the page would scroll too.
  useEffect(() => {
    const el = strip.current;
    if (!el) return;
    const wheel = (e: WheelEvent) => {
      if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
      const end = el.scrollWidth - el.clientWidth;
      // Right to left, scrollLeft runs from 0 down to -end.
      const at = Math.abs(el.scrollLeft);
      if (end <= 0 || (e.deltaY < 0 && at <= 0) || (e.deltaY > 0 && at >= end - 1)) return;
      e.preventDefault();
      el.scrollLeft += isRtl() ? -e.deltaY : e.deltaY;
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => el.removeEventListener("wheel", wheel);
  }, [showStrip]);

  // Coming back to Home, the strip starts at its left end.
  useEffect(() => {
    strip.current?.querySelector("[role=tab][aria-selected=true]")?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [active]);

  // A new tab rises into the strip; its neighbours glide (useFlip).
  useLayoutEffect(() => {
    fresh.forEach((id, n) => {
      const el = row.current?.querySelector(`:scope > [data-id="${id}"]`);
      if (el) riseIn(el, n);
    });
  }, [fresh.join()]); // eslint-disable-line react-hooks/exhaustive-deps

  const plus = (
    <IconButton
      icon="plus"
      data-home-add
      label={free.length ? t("dashboard.tabs.add") : t("accounts.add.title")}
      // Every account has its tab, or there are none: the only way on is another account.
      popoverTarget={free.length ? ADD : undefined}
      onClick={free.length ? undefined : onAdd}
      style={anchor.style}
    />
  );
  const playAll = ready.length > 0 && (
    <LaunchButton
      label={t("launch.launchMany", { count: ready.length })}
      disabled={allBlock !== null}
      reason={said(allBlock)}
      shortcut={!open}
      onLaunch={() => void launchEach(ready.map((tab) => ({ account: byId.get(tab.accountId)!, where: { draft: tab.draft, setDraft: (f) => home.setDraft(tab.accountId, f) } })))}
    />
  );

  return (
    <>
      {/* Only in the top layout: a layout switch swaps the strip at once, it doesn't fold it away. */}
      {showStrip && (
        <div className="home-strip">
          <div ref={strip} role="tablist" aria-label={t("dashboard.tabs.label")} className="tab-strip" onKeyDown={tabKeys}>
            <span ref={tabDrop} aria-hidden="true" className="home-tab-on lq-drop" />
            <div className="tab">
              <button type="button" role="tab" id={tabId("news")} aria-selected={active === "news"} aria-controls="home-tab-panel" tabIndex={active === "news" ? 0 : -1} onClick={() => home.setActive("news")}>
                {t("news.tab")}
                {news > 0 && <span className="news-count" aria-label={t("news.unreadCount", { n: news })}>{news}</span>}
              </button>
            </div>
            <div className="tab">
              <button type="button" role="tab" id={tabId(null)} aria-selected={active === null} aria-controls="home-tab-panel" tabIndex={active === null ? 0 : -1} onClick={() => home.setActive(null)}>
                {t("dashboard.tabs.overview")}
              </button>
            </div>
            <Sortable
              className="flex flex-none items-center gap-1"
              ids={ids}
              order={home.order}
              onOrder={(next) => {
                hold();
                home.setOrder(next);
              }}
              name={(id) => accountName(byId.get(id)!)}
              render={(id, drag) => {
                const a = byId.get(id)!;
                const tab = tabs.find((x) => x.accountId === id)!;
                const status = statusOf(id);
                const selected = active === id;
                const name = accountName(a);
                // Not while it launches: a press says why.
                const busy = status === "queued" ? t("dashboard.tabs.queued") : status === "launching" ? t("launch.launching") : null;
                const close = () => {
                  if (busy) return announce(busy);
                  home.close(id);
                  focusSelected();
                };
                return (
                  <div key={id} className="tab" data-flip data-id={id}>
                    <button
                      type="button"
                      role="tab"
                      id={tabId(id)}
                      aria-selected={selected}
                      aria-controls="home-tab-panel"
                      tabIndex={selected ? 0 : -1}
                      onClick={() => home.setActive(id)}
                      {...drag}
                      aria-keyshortcuts={`${drag["aria-keyshortcuts"]} Delete`}
                      onKeyDown={(e) => {
                        drag.onKeyDown(e);
                        if (e.key !== "Delete") return;
                        e.preventDefault();
                        close();
                      }}
                    >
                      <LaunchMark inst={liveOf(id)} status={status} />
                      <span className="truncate">{name}</span>
                      <ReadyRing on={ready.includes(tab)} />
                    </button>
                    {/* Out of the Tab order, as the tabs are one stop: Delete closes from the keyboard. */}
                    <IconButton
                      icon="x"
                      size="sm"
                      iconSize={12}
                      className="home-tab-x"
                      tabIndex={-1}
                      label={t("dashboard.tabs.close", { name })}
                      aria-disabled={!!busy || undefined}
                      onClick={close}
                    />
                  </div>
                );
              }}
            />
          </div>
          {plus}
          {playAll && <div className="ms-auto">{playAll}</div>}
        </div>
      )}
      {side && open && playAll && <div className="flex justify-end">{playAll}</div>}
      {(open || ready.length > 1 || allReplace) && <MultiNotice wanted={ready.length > 1} replaces={allReplace || (!!open && replaces(open))} />}
      <div role={showStrip ? "tabpanel" : undefined} id="home-tab-panel" aria-labelledby={showStrip ? tabId(active) : undefined} className="flex flex-col gap-6 min-w-0">
        {active === "news" ? (
          <NewsView />
        ) : open ? (
          <TabBody
            key={open.accountId}
            tab={open}
            account={byId.get(open.accountId)!}
            accounts={accounts}
            instances={instances}
            status={statusOf(open.accountId)}
            block={blockFor(false, replaces(open))}
            why={said(blockFor(false, replaces(open)))}
            onDraft={(f) => home.setDraft(open.accountId, f)}
          />
        ) : (
          children(side ? playAll : null)
        )}
      </div>
      <Popover
        id={ADD}
        anchor={anchor.name}
        role="dialog"
        label={t("dashboard.tabs.add")}
        className="home-add-pop"
        onToggle={(on) => {
          setShown(on);
          if (!on) return;
          setOpened((n) => n + 1);
          requestAnimationFrame(() => document.getElementById(ADD)?.querySelector<HTMLElement>("input")?.focus());
        }}
      >
        {/* Only while open: a closed one would still render every account on each poll. */}
        {shown && <AccountPicker
          key={opened}
          variant="menu"
          accounts={free}
          label={t("dashboard.tabs.add")}
          onPick={(id) => {
            hideAdd();
            home.add(id);
            focusSelected();
          }}
          onAdd={() => {
            hideAdd();
            onAdd();
          }}
        />}
      </Popover>
    </>
  );
}
