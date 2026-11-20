import "./settings.css";
import { memo, useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState, type ComponentType, type ReactNode, type RefObject } from "react";
import { AnimatePresence, motion } from "motion/react";
import { SECTION_ICON, SECTION_VIEW, useNavigationRequests, VIEW_NAME, type Section, type View } from "../../app/navigation";
import { t } from "../../i18n";
import { rovingKeys } from "../../ui/a11y";
import { Segmented } from "../../ui/controls";
import { Input } from "../../ui/Field";
import { Icon, type IconName } from "../../ui/Icon";
import { Collapse, ListPresence, prefersReducedMotion, RISE, SPRING, TWEEN } from "../../ui/motion";
import { useStored } from "../../ui/stored";
import { BenchPanel, PERFORMANCE_RENDERERS, PresetsPanel, ReviewProvider, SharePanel } from "../performance/panels";
import { AREAS, type Panel, type SettingsArea } from "./areas";
import { GOALS } from "./goals";
import { jumpTo, requestJump } from "./jump";
import { EntryRow, Renderers, SearchHits, useCtx, type RowRenderers } from "./kit/SettingRow";
import { HeadlessResolutionRow, LinuxLaunchRows } from "./LinuxRows";
import { MocktailNotice } from "./MocktailNotice";
import { SettingSection } from "./kit/SettingSection";
import { dyn, entry, shown, type Entry } from "./registry";
import { keyOf, placeNames, search, type HitGroup } from "./search";
import { ReplayTourRow, SelfUpdateRow, SupportRow, VersionRow } from "./rows/about";
import { AccentRow, BackgroundRow, LookRow, LookShareRow, MotionRow, SoundRow, ThemeRow, ThemesRow } from "./rows/appearance";
import { BackupRow, BrowserDataRow, CleanLogsRow, DataFolderRow } from "./rows/data";
import { BreakerRow, ClientCard, LauncherRow, MultiRow, ShutdownRow } from "./rows/launching";
import { EncryptionRow, MasterPasswordRow, PublicNetworkRow, RecoveryCodeRow } from "./rows/security";
import { ShortcutsTable } from "./rows/shortcuts";

/** Settings' rows that draw their own controls, by entry id. */
const RENDERERS: RowRenderers = {
  ...PERFORMANCE_RENDERERS,
  multi: MultiRow,
  breaker: BreakerRow,
  shutdown: ShutdownRow,
  defaultLauncher: LauncherRow,
  client: ClientCard,
  theme: ThemeRow,
  themes: ThemesRow,
  look: LookRow,
  lookShare: LookShareRow,
  accent: AccentRow,
  background: BackgroundRow,
  sounds: SoundRow,
  motion: MotionRow,
  encryption: EncryptionRow,
  masterPassword: MasterPasswordRow,
  recoveryCode: RecoveryCodeRow,
  publicNetworks: PublicNetworkRow,
  dataFolder: DataFolderRow,
  backup: BackupRow,
  cleanLogs: CleanLogsRow,
  browserData: BrowserDataRow,
  selfUpdate: SelfUpdateRow,
  version: VersionRow,
  support: SupportRow,
  replayTour: ReplayTourRow,
};

/** Whether an entry is drawn as a row of Settings. What a panel stands for (a preset card) has no
 * row, and another view's setting isn't here: search shows those as the way there. */
const isRow = (e: Entry) => e.place.view === "settings" && (e.control.kind !== "action" || e.id in RENDERERS);

/** Each view's island icon, for search's links into it. */
const VIEW_ICON = Object.fromEntries((Object.keys(SECTION_VIEW) as Section[]).map((s) => [SECTION_VIEW[s], SECTION_ICON[s]])) as Partial<Record<View, IconName>>;

/** What was typed, and what it found then. Found per keystroke, not per render: a row whose words a
 * flow changes (the vault's mode, a new recovery code) stays on screen, its dialog with it. */
type Find = { q: string; found: HitGroup[] | null };
const NO_FIND: Find = { q: "", found: null };
/** Rows whose backend exists only on Windows: Performance leaves them out on this fork, so search
 * must not offer a jump that lands on nothing. Mute is not here: the Linux fork has `pactl`. */
const HIDDEN_ON_LINUX = new Set(["bandwidth"]);

/** An area's groups as the page and the list both draw them: a group with no visible row is no
 * block (its entry in the list would land on nothing). */
const groupsOf = <G extends { ids: readonly string[]; panel?: Panel }>(a: { groups: readonly G[] }) =>
  a.groups.filter((g) => g.panel || g.ids.some((id) => !HIDDEN_ON_LINUX.has(id)));

/** How the page is grouped: by RoALT's own areas, or by what a person wants to do. */
type By = "areas" | "goals";

// A block is a titled stretch of the page that the list at the side can go to: an area's group
// ("launching/1"; group 0 is the area's own title) or a goal ("goal/online").
const FIRST: Record<By, string> = { areas: `${AREAS[0].id}/0`, goals: `goal/${GOALS[0].id}` };

/** RoALT's own keys: a table, not rows. */
function KeysPanel({ block }: { block?: string }) {
  return (
    <SettingSection sub label="settings.group.ownKeys" block={block}>
      <ShortcutsTable />
    </SettingSection>
  );
}
/** The groups that aren't rows (areas.ts), each drawn whole; `block`: its name on the page. */
const PANELS: Record<Panel, ComponentType<{ block?: string }>> = { presets: PresetsPanel, bench: BenchPanel, share: SharePanel, keys: KeysPanel };
const panel = (name: Panel, block?: string) => {
  const Draw = PANELS[name];
  return <Draw key={name} block={block} />;
};

/** Where an area starts in either grouping: among goals, the one that holds its first row. */
function blockOf(area: SettingsArea, by: By) {
  if (by === "areas") return `${area}/0`;
  const row = AREAS.find((a) => a.id === area)!.groups.find((g) => !g.panel)?.ids[0];
  const goal = GOALS.find((g) => row && (g.start.includes(row) || g.tune.includes(row)));
  return goal ? `goal/${goal.id}` : FIRST.goals;
}

// The block the page was on when Settings was last on screen: it comes back there.
let lastBlock: string | null = null;

/** Settings: one page with every row, and a list of its blocks at the side. `area`: one asked for on
 * the way here (a link, the app menu); without one the page opens where it was. */
export function SettingsView({ area }: { area?: SettingsArea | null }) {
  const ctx = useCtx();
  const main = useRef<HTMLElement>(null);
  const [by, setBy] = useStored<By>("roalt.settingsBy", (s) => (s === "goals" ? "goals" : "areas"), (v) => v);
  const [find, setFind] = useState(NO_FIND);
  const q = find.q;
  // The field answers each key at once; the results follow when React has time. Ending a search
  // doesn't wait: the page shows at once.
  const deferred = useDeferredValue(find);
  const { q: foundQ, found } = find.found ? deferred : find;
  // Less the rows a change has hidden since (the master-only ones, once the password is removed).
  const groups = found?.map((g) => ({ ...g, hits: g.hits.filter((h) => shown(h.row, ctx) && !HIDDEN_ON_LINUX.has(h.row.id)) })).filter((g) => g.hits.length) ?? null;
  const hits = groups?.reduce((n, g) => n + g.hits.length, 0) ?? 0;
  const first = groups?.[0]?.hits[0]?.row;

  // While a pick glides the page to its block, the list waits: it would mark every block on the way.
  const gliding = useRef(false);
  const look = useRef(() => {});
  const column = useRef<HTMLDivElement>(null);
  /** Brings a block to the top, under the page's own top padding, and returns it; null when it
   * isn't on the page (during a search). */
  const show = (key: string, glide: boolean) => {
    const el = main.current;
    const col = column.current;
    const block = el?.querySelector<HTMLElement>(`[data-block="${key}"]`);
    if (!el || !col || !block) return null;
    // By layout offsets, not by where the block is drawn: the page may still be rising in.
    let y = 0;
    for (let n: HTMLElement | null = block; n && n !== col; n = n.offsetParent as HTMLElement | null) y += n.offsetTop;
    const top = col.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop + y - parseFloat(getComputedStyle(el).paddingTop);
    const smooth = glide && !prefersReducedMotion();
    el.scrollTo({ top, behavior: smooth ? "smooth" : "instant" });
    if (!smooth) return block;
    gliding.current = true;
    const landed = () => {
      clearTimeout(timer);
      el.removeEventListener("scrollend", landed);
      gliding.current = false;
      look.current();
    };
    // A block already in place scrolls nothing, so no scrollend comes.
    const timer = setTimeout(landed, 1500);
    el.addEventListener("scrollend", landed);
    return block;
  };
  /** A block someone asked for: shown, and the focus goes to it, so Tab goes on from there and not
   * from the top of the page. No ring: it is no control (base.css). */
  const reach = (key: string, glide: boolean) => {
    const block = show(key, glide);
    if (!block) return false;
    block.tabIndex = -1;
    block.focus({ preventScroll: true });
    return true;
  };

  // The block to reach once the page is back: a search hides it, and ends for it.
  const wanted = useRef<string | null>(null);
  const go = (key: string) => {
    if (reach(key, true)) return;
    wanted.current = key;
    setFind(NO_FIND);
  };
  useLayoutEffect(() => {
    if (wanted.current && reach(wanted.current, false)) wanted.current = null;
  });
  // On arrival: the area asked for; else the place it was left at, the focus left alone (the top
  // needs no scroll). Once: an area asked for later comes as a navigation request.
  useLayoutEffect(() => {
    if (area) reach(blockOf(area, by), false);
    else if (lastBlock && lastBlock !== FIRST[by]) show(lastBlock, false);
  }, []);
  useNavigationRequests((r) => {
    if (r.view !== "settings") return;
    if (r.area) go(blockOf(r.area, by));
    // Asked for by its tile while a search is on: the page itself is what was asked for.
    else if (find.found) setFind(NO_FIND);
  });

  const regroup = (next: By) => {
    setBy(next);
    setFind(NO_FIND);
    main.current?.scrollTo({ top: 0, behavior: "instant" });
  };

  return (
    <main ref={main} className="scroll flex-1 min-w-0 page">
      <Renderers value={RENDERERS}>
        <ReviewProvider>
          <div className="set-map" data-by={by}>
            <Side main={main} gliding={gliding} look={look} by={by} onBy={regroup} searching={!!groups} onGo={go}>
              <Input
                id="settings-search"
                type="search"
                leading="search"
                data-find
                aria-label={t("settings.searchLabel")}
                placeholder={t("settings.searchPlaceholder")}
                value={q}
                onChange={(e) => setFind({ q: e.target.value, found: e.target.value.trim() ? search(e.target.value, ctx) : null })}
                onKeyDown={(e) => {
                  if (e.key === "Escape" && q) {
                    e.stopPropagation();
                    setFind(NO_FIND);
                  } else if (e.key === "Enter" && first) {
                    e.preventDefault();
                    // A row among the results takes the jump where it is; anything else is gone to.
                    (isRow(first) ? requestJump : jumpTo)(first.id);
                  }
                }}
              />
              <p role="status" className="sr-only">
                {groups && (hits ? t("settings.searchCount", { count: hits }) : t("settings.searchNone", { q: foundQ.trim() }))}
              </p>
            </Side>
            <div ref={column} className="relative min-w-0">
              {/* Each area or goal is titled on the page; results come from every view, under the page's own name. */}
              <h1 className={groups ? "page-title set-area-title" : "sr-only"}>{t("settings.title")}</h1>
              <AnimatePresence mode="popLayout" initial={false}>
                <motion.div
                  key={groups ? "search" : by}
                  initial={{ opacity: 0, y: RISE.md }}
                  animate={{ opacity: 1, y: 0, transition: SPRING.smooth }}
                  exit={{ opacity: 0, transition: TWEEN.exit }}
                >
                  {groups ? <Results q={foundQ} groups={groups} /> : <Page by={by} />}
                </motion.div>
              </AnimatePresence>
            </div>
          </div>
        </ReviewProvider>
      </Renderers>
    </main>
  );
}

/** The list at the side: the search field (`children`), the grouping, and every block of the page.
 * It marks the block at the top as the page scrolls. That mark is its own state, so scrolling draws
 * the list again and never the rows. */
function Side({ main, gliding, look, by, onBy, searching, onGo, children }: {
  main: RefObject<HTMLElement | null>;
  gliding: RefObject<boolean>;
  /** Set here: reads the page's place again (the view calls it when a glide lands). */
  look: RefObject<() => void>;
  by: By;
  onBy: (by: By) => void;
  searching: boolean;
  onGo: (block: string) => void;
  children: ReactNode;
}) {
  const [at, setAt] = useState(lastBlock ?? FIRST[by]);
  useEffect(() => {
    const el = main.current;
    if (!el || searching) return;
    const read = () => {
      if (gliding.current) return;
      const line = el.getBoundingClientRect().top + 120;
      const marks = [...el.querySelectorAll<HTMLElement>("[data-block]")];
      // At the very end the last blocks can't reach the top: the last one counts.
      const ended = el.scrollTop + el.clientHeight >= el.scrollHeight - 2;
      const key = ((ended ? marks.at(-1) : marks.filter((m) => m.getBoundingClientRect().top <= line).at(-1)) ?? marks[0])?.dataset.block;
      if (!key) return;
      lastBlock = key;
      setAt(key);
    };
    look.current = read;
    read();
    el.addEventListener("scroll", read, { passive: true });
    return () => {
      el.removeEventListener("scroll", read);
      look.current = () => {};
    };
  }, [main, gliding, look, by, searching]);

  const pick = (key: string) => {
    setAt(key);
    onGo(key);
  };
  const area = at.split("/")[0];
  // One Tab stop, as the island has: the block the page is on (the first during a search); the
  // arrows move through the list.
  const stop = searching ? FIRST[by] : at;
  const tab = (key: string) => (key === stop ? 0 : -1);

  return (
    <nav className="set-side" aria-label={t("settings.title")}>
      {children}
      <Segmented
        label={t("settings.by.label")}
        className="set-by"
        value={by}
        onChange={onBy}
        options={[
          { value: "areas", label: t("settings.by.areas") },
          { value: "goals", label: t("settings.by.goals") },
        ]}
      />
      {by === "goals" && <p id="set-lead" className="set-lead">{t("settings.goals.lead")}</p>}
      <ul className="set-nav" aria-labelledby={by === "goals" ? "set-lead" : undefined} onKeyDown={(e) => void rovingKeys(e, "button", "vertical")}>
        {by === "goals"
          ? GOALS.map((g) => (
              <li key={g.id}>
                <button type="button" className="set-nav-item" tabIndex={tab(`goal/${g.id}`)} aria-current={!searching && at === `goal/${g.id}` ? "location" : undefined} onClick={() => pick(`goal/${g.id}`)}>
                  <Icon name={g.icon} size={16} />
                  {t(g.title)}
                </button>
              </li>
            ))
          : AREAS.map((a) => {
              const here = !searching && area === a.id;
              const groups = groupsOf(a);
              return (
                <li key={a.id}>
                  <button type="button" className="set-nav-item" tabIndex={tab(`${a.id}/0`)} aria-current={here ? "true" : undefined} onClick={() => pick(`${a.id}/0`)}>
                    <Icon name={a.icon} size={16} />
                    {t(a.label)}
                  </button>
                  {groups.length > 1 && (
                    <Collapse open={here}>
                      <ul className="set-nav-subs">
                        {groups.map((g, i) => (
                          <li key={i}>
                            {/* The first group starts where its area does: the area's row is its Tab stop. */}
                            <button type="button" className="set-nav-sub" tabIndex={i ? tab(`${a.id}/${i}`) : -1} aria-current={at === `${a.id}/${i}` ? "location" : undefined} onClick={() => pick(`${a.id}/${i}`)}>
                              {g.label && t(g.label)}
                            </button>
                          </li>
                        ))}
                      </ul>
                    </Collapse>
                  )}
                </li>
              );
            })}
      </ul>
    </nav>
  );
}

/** Every row, in one grouping. Drawn again only when the grouping changes: the rows read their own
 * values, and the list at the side keeps its own place. */
const Page = memo(function Page({ by }: { by: By }) {
  const { s } = useCtx();
  // Rows with no backend here are left out of the page, as search leaves them; a group left with
  // none of its own is no group.
  const here = (ids: readonly string[]) => ids.filter((id) => !HIDDEN_ON_LINUX.has(id));
  if (by === "goals")
    return GOALS.map((g) => (
      <section key={g.id} className="set-area" aria-labelledby={`set-goal-${g.id}`}>
        <header className="set-goal-head" data-block={`goal/${g.id}`}>
          <h2 id={`set-goal-${g.id}`} className="page-title">{t(g.title)}</h2>
          <p className="page-sub">{t(g.say)}</p>
        </header>
        {g.top?.map((p) => panel(p))}
        {here(g.start).length > 0 && <SettingSection sub label="settings.goals.start" ids={here(g.start)} />}
        {here(g.tune).length > 0 && <SettingSection sub label="settings.goals.tune" ids={here(g.tune)} />}
        {g.end?.map((p) => panel(p))}
        {g.links.length > 0 && (
          // With rows of its own, the links are an aside; with none, they are where the goal lives.
          <SettingSection sub label={g.start.length ? "settings.goals.elsewhere" : "settings.goals.where"}>
            {g.links.map((id) => (
              <div key={id} className="set-wrap">
                <LinkRow row={entry(id)} />
              </div>
            ))}
          </SettingSection>
        )}
      </section>
    ));
  return AREAS.map((a) => (
    <section key={a.id} className="set-area" aria-labelledby={`set-area-${a.id}`}>
      <h2 id={`set-area-${a.id}`} className="page-title set-area-title" data-block={`${a.id}/0`}>{t(a.label)}</h2>
      {groupsOf(a).map((g, i) => {
        // The first group starts where its area does: the area's title is its block.
        const block = i ? `${a.id}/${i}` : undefined;
        if (g.panel) return panel(g.panel, block);
        const ids = here(g.ids);
        if (!ids.length) return null;
        return (
          <SettingSection key={i} sub label={g.label} ids={ids} block={block}>
            {/* This fork's own rows, beside the ones they affect. */}
            {a.id === "launching" && i === 0 && <LinuxLaunchRows settings={s} />}
            {a.id === "performance" && g.label === "performance.section.roalt" && (
              <>
                <div className="set-row">
                  <MocktailNotice />
                </div>
                <HeadlessResolutionRow settings={s} />
              </>
            )}
          </SettingSection>
        );
      })}
    </section>
  ));
});

/** Hits from every screen, grouped where they live: Settings' as their own live rows (a match in
 * "More about" opens it), other views' as links that go there. */
function Results({ q, groups }: { q: string; groups: HitGroup[] }) {
  // The same set while the same rows open: a new one would re-render every row in the results.
  const moreIds = groups.flatMap((g) => g.hits.filter((h) => h.where === "more").map((h) => h.via.id)).join(" ");
  const more = useMemo(() => new Set(moreIds.split(" ")), [moreIds]);
  if (!groups.length) return <p className="empty enter">{t("settings.searchNone", { q: q.trim() })}</p>;
  return (
    <SearchHits value={more}>
      <ListPresence items={groups} getKey={(g) => keyOf(g.place)} className="set-results" render={(g) => <ResultGroup group={g} />} />
    </SearchHits>
  );
}

const hitIds = (g: HitGroup) => g.hits.map((h) => h.row.id).join(" ");

/** Drawn again only when its rows change, not on every key that finds the same ones. */
const ResultGroup = memo(function ResultGroup({ group }: { group: HitGroup }) {
  return (
    <section className="set-section">
      <h2 className="section-title set-heading">{placeNames(group.place).join(" · ")}</h2>
      <ListPresence
        items={group.hits}
        getKey={(h) => h.row.id}
        className="panel set-group"
        itemClassName="set-wrap"
        render={(h) => (isRow(h.row) ? <EntryRow id={h.row.id} /> : <LinkRow row={h.row} />)}
      />
    </section>
  );
}, (a, b) => keyOf(a.group.place) === keyOf(b.group.place) && hitIds(a.group) === hitIds(b.group));

/** A setting in another view, or a panel of Settings' own page: its words, and the way there (it
 * lands on it and flashes it). */
function LinkRow({ row }: { row: Entry }) {
  const ctx = useCtx();
  const place = row.place;
  const area = place.view === "settings" ? AREAS.find((a) => a.id === place.area) : undefined;
  return (
    <button type="button" className="set-row set-link" onClick={() => jumpTo(row.id)}>
      <span className="set-link-icon">
        <Icon name={area?.icon ?? VIEW_ICON[place.view] ?? "settings"} size={15} />
      </span>
      <span className="set-text">
        <span className="set-label block">{t(row.label)}</span>
        {row.hint && <span className="set-hint block">{dyn(row.hint, ctx)}</span>}
      </span>
      <span className="set-go">
        {t(area?.label ?? VIEW_NAME[place.view])}
        <Icon name="chevron" size={14} className="-rotate-90 rtl:rotate-90" />
      </span>
    </button>
  );
}
