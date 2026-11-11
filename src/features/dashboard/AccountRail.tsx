import { memo, useMemo, useRef, useState } from "react";
import { motion } from "motion/react";
import type { AccountSummary, Instance } from "../../api";
import { useSettings } from "../../data";
import { t } from "../../i18n";
import { rovingKeys } from "../../ui/a11y";
import { IconButton } from "../../ui/Button";
import { Input } from "../../ui/Field";
import { Icon } from "../../ui/Icon";
import { lid, Num, SPRING, STAGGER } from "../../ui/motion";
import { accountName, Avatar } from "../accounts/shared";
import { useLaunchActivity } from "../launch/launchQueue";
import { isLive, STATES } from "../launch/Running";
import { useNewsFeed } from "../news/news";
import { LaunchMark, ReadyRing } from "./AccountTabs";
import { byOrder, readyTabs, tabLaunch, type HomeTab, type HomeTabs } from "./homeLayout";
import { Sortable, type DragProps } from "./Sortable";
import { useSteadyFlip } from "./useSteadyFlip";

const ON = lid("home-rail", "on");
const ROW = ".rail-item";

type RowActs = { open: (id: number) => void; close: (id: number) => void; focus: (k: string) => void };

/** One account's row. Memoized on what it shows: a long rail re-renders only the rows that changed. */
const RailRow = memo(function RailRow({ id, account: a, open, state, status, ready, current, tabIndex, compact, still, drag, act }: {
  id: number;
  account: AccountSummary;
  /** It has a tab on Home. */
  open: boolean;
  /** Its live client's state. */
  state: Instance["state"] | undefined;
  status: HomeTab["status"];
  ready: boolean;
  current: boolean;
  tabIndex: number;
  compact: boolean;
  still: boolean;
  drag: DragProps;
  act: RowActs;
}) {
  const k = String(id);
  const busy = status === "queued" || status === "launching";
  const name = accountName(a);
  const close = () => act.close(id);
  return (
    <div className="home-rail-row" data-flip={!still || undefined}>
      <button
        type="button"
        className="rail-item"
        tabIndex={tabIndex}
        aria-current={current ? "page" : undefined}
        {...drag}
        aria-keyshortcuts={open ? `${drag["aria-keyshortcuts"]} Delete` : drag["aria-keyshortcuts"]}
        onKeyDown={(e) => {
          drag.onKeyDown(e);
          if (e.key === "Delete" && open && !busy) {
            e.preventDefault();
            close();
          }
        }}
        onFocus={() => act.focus(k)}
        onClick={() => act.open(id)}
      >
        {current && <motion.span layoutId={ON} className="home-rail-on" transition={SPRING.smooth} />}
        <span className="home-rail-face">
          <Avatar account={a} size={compact ? 24 : 30} />
          <span className="home-rail-mark">
            <LaunchMark inst={state && { state }} status={status} />
          </span>
        </span>
        <span className="flex-1 min-w-0 flex flex-col">
          <span className="truncate">{name}</span>
          {!compact && (
            <span className="truncate text-xs font-normal text-fg-3">
              {busy ? t(status === "queued" ? "dashboard.tabs.queued" : "launch.launching") : state ? STATES[state][0] : status === "failed" ? t("dashboard.tabs.failed") : `@${a.username}`}
            </span>
          )}
        </span>
        <ReadyRing on={ready} still={still} />
      </button>
      {open && <IconButton icon="x" size="sm" iconSize={12} className="home-rail-x" tabIndex={-1} label={t("dashboard.tabs.close", { name })} disabled={busy} onClick={close} />}
    </div>
  );
});

/** The "accounts on the side" layout's left part: Overview and every account, each opening on Home.
 * An account's row opens its tab there, as the strip's + does; × (or Delete) closes it, and a ring
 * says "Play with N" will start it. One Tab stop: the arrows move, Alt and the arrows reorder.
 * `onHome`: Home is on screen, the only place a row is the current one. */
export function AccountRail({ width, accounts, instances, home, onHome, onOpen, onAdd }: {
  width: number;
  accounts: AccountSummary[];
  instances: Instance[];
  home: HomeTabs;
  onHome: boolean;
  onOpen: () => void;
  onAdd: () => void;
}) {
  const compact = useSettings().data?.density === "compact";
  const activity = useLaunchActivity();
  const [who, setWho] = useState("");
  const [focused, setFocused] = useState<string | null>(null);
  const list = useRef<HTMLDivElement>(null);
  // Sortable's own column holds the rows: that is what glides.
  const col = useMemo(() => ({ get current() { return list.current?.querySelector<HTMLElement>(":scope > .sort-col") ?? null; } }), []);
  const needle = who.trim().toLowerCase();
  const shown = needle ? accounts.filter((a) => `${a.alias}\n${a.displayName}\n${a.username}`.toLowerCase().includes(needle)) : accounts;
  const ids = byOrder(shown.map((a) => a.id), home.order);
  const hold = useSteadyFlip(col, ids.join());
  // A long list's rows don't move: no glides, no ring popping in.
  const still = ids.length > STAGGER.listLimit;
  const running = instances.filter(isLive).length;
  const byId = new Map(accounts.map((a) => [a.id, a]));
  const ready = new Set(readyTabs(home.tabs, byId, instances).map((tab) => tab.accountId));
  const news = useNewsFeed().unread;
  const here = onHome ? (home.active === null ? "overview" : String(home.active)) : null;
  // The row that takes the Tab key: the one last focused, else the current one, while it is shown.
  const keys = [...(needle ? [] : ["news", "overview"]), ...ids.map(String), "add"];
  const stop = [focused, here].find((k) => k !== null && keys.includes(k)) ?? keys[0];
  const tab = (k: string) => (k === stop ? 0 : -1);
  const on = (k: string) => k === here && <motion.span layoutId={ON} className="home-rail-on" transition={SPRING.smooth} />;
  // Per row only what it shows, so a poll re-renders just the rows that changed.
  const liveState = new Map<number, Instance["state"]>();
  for (const i of instances) if (isLive(i) && !liveState.has(i.accountId)) liveState.set(i.accountId, i.state);
  const openTabs = new Set(home.tabs.map((x) => x.accountId));
  const latest = useRef({ home, onOpen });
  latest.current = { home, onOpen };
  const [act] = useState<RowActs>(() => ({
    open: (id) => (latest.current.home.add(id), latest.current.onOpen()),
    close: (id) => latest.current.home.close(id),
    focus: setFocused,
  }));
  // Each row's drag keys, steady across renders; they call the newest ones Sortable handed out.
  const drags = useRef(new Map<number, DragProps>());
  const steady = useRef(new Map<number, DragProps>());
  const steadyDrag = (id: number, drag: DragProps) => {
    drags.current.set(id, drag);
    let s = steady.current.get(id);
    if (!s) {
      const call = (k: string) => (e: unknown) => (drags.current.get(id)![k as keyof DragProps] as (e: unknown) => void)(e);
      s = Object.fromEntries(Object.entries(drag).map(([k, v]) => [k, typeof v === "function" ? call(k) : v])) as DragProps;
      steady.current.set(id, s);
    }
    return s;
  };

  return (
    <nav aria-label={t("dashboard.tabs.label")} style={{ width }} className="rail home-rail" data-compact={compact || undefined}>
      <div className="flex-none p-2">
        <Input
          leading="search"
          type="search"
          aria-label={t("accounts.filterLabel")}
          placeholder={t("accounts.filterPlaceholder")}
          autoComplete="off"
          value={who}
          onChange={(e) => setWho(e.target.value)}
          onKeyDown={(e) => {
            if (e.key !== "ArrowDown") return;
            e.preventDefault();
            list.current?.querySelector<HTMLElement>(ROW)?.focus();
          }}
        />
      </div>
      <div ref={list} className="scroll flex-1 min-h-0 flex flex-col gap-0.5 px-2 pb-2" style={{ scrollbarWidth: "none" }} onKeyDown={(e) => rovingKeys(e, ROW, "vertical")}>
        {!needle && (
          <button type="button" className="rail-item" tabIndex={tab("news")} aria-current={here === "news" ? "page" : undefined} onFocus={() => setFocused("news")} onClick={() => (home.setActive("news"), onOpen())}>
            {on("news")}
            <span className="home-rail-face">
              <Icon name="news" />
            </span>
            <span className="flex-1 min-w-0 truncate">{t("news.tab")}</span>
            {news > 0 && <span className="news-count" aria-label={t("news.unreadCount", { n: news })}>{news}</span>}
          </button>
        )}
        {!needle && (
          <button type="button" className="rail-item" tabIndex={tab("overview")} aria-current={here === "overview" ? "page" : undefined} onFocus={() => setFocused("overview")} onClick={() => (home.setActive(null), onOpen())}>
            {on("overview")}
            <span className="home-rail-face">
              <Icon name="home" />
            </span>
            <span className="flex-1 min-w-0 truncate">{t("dashboard.tabs.overview")}</span>
            {running > 0 && (
              <span className="rail-count" data-tip={t("app.running", { count: running })}>
                <Num value={running} />
              </span>
            )}
          </button>
        )}
        <Sortable
          column
          className="flex flex-col gap-0.5"
          ids={shown.map((a) => a.id)}
          order={home.order}
          onOrder={(next) => {
            hold();
            home.setOrder(next);
          }}
          name={(id) => accountName(byId.get(id)!)}
          render={(id, drag) => {
            const k = String(id);
            const state = liveState.get(id);
            return (
              <RailRow
                key={id}
                id={id}
                account={byId.get(id)!}
                open={openTabs.has(id)}
                state={state}
                status={tabLaunch(activity, id, !!state)}
                ready={ready.has(id)}
                current={here === k}
                tabIndex={tab(k)}
                compact={compact}
                still={still}
                drag={steadyDrag(id, drag)}
                act={act}
              />
            );
          }}
        />
        {needle && !shown.length && <p role="status" className="px-2.5 py-2 text-[13px] text-fg-2">{t("accounts.noMatch")}</p>}
        <button type="button" className="rail-item" tabIndex={tab("add")} onFocus={() => setFocused("add")} onClick={onAdd}>
          <span className="home-rail-face">
            <Icon name="plus" />
          </span>
          <span className="flex-1 min-w-0 truncate">{t("accounts.add.title")}</span>
        </button>
      </div>
    </nav>
  );
}
