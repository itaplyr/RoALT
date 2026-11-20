import "./browse.css";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type AccountSummary, type BrowseBlocked, type BrowsePage, type BrowsePlay } from "../../api";
import { playTarget, requestNavigation, type BrowseRequest } from "../../app/navigation";
import { useGame, useSettings } from "../../data";
import { t, type Key } from "../../i18n";
import { Button, IconButton } from "../../ui/Button";
import { Segmented } from "../../ui/controls";
import { Input, Select } from "../../ui/Field";
import { readStored, writeStored } from "../../ui/stored";
import { Tabs } from "../../ui/Tabs";
import { toast, toastError } from "../../ui/Toast";
import { accountName, Avatar } from "../accounts/shared";
import { FavouriteDialog } from "../launch/Favourites";
import { launchOne, launchStep } from "../launch/launchQueue";
import { useMoveConfirm } from "../launch/MoveDialog";
import { copyRoaltLink } from "../launch/target";
import { useGameDetails } from "./GameDetails";
import { LimitedsTab } from "./Market";
import { PlaytimeTab, PopularTab } from "./Native";
import { usePageCard } from "./pageCard";
import { usePlayerDetails } from "./PlayerDetails";
import { RobloxPane } from "./RobloxPane";

const LAST_KEY = "roalt.browseAccount";
const lastAccount = () => Number(readStored(LAST_KEY)) || null;
const remember = (id: number) => writeStored(LAST_KEY, String(id));

/** The page Browse last showed: coming back to Browse goes on from there, not from its home. */
let lastPath = "home";
/** The last "Open on roblox.com" Browse followed: coming back to Browse later doesn't follow it again. */
let askDone = 0;
/** A page that starts a game as it loads (a private server or share link): never loaded again. */
const launches = (path: string) => /^(share|games\/start)\b|[?&]privateserverlinkcode=/i.test(path);
/** The place of a game's page ("games/920587237/Adopt-Me", maybe under a language: "de/games/..."). */
const placeOf = (path: string) => Number(/^(?:[a-z]{2}(?:-[a-z]{2,4})?\/)?games\/(\d{1,15})(?:[/?#]|$)/i.exec(path)?.[1]) || null;
/** The player of a profile page ("users/156/profile", maybe under a language). */
const userOf = (path: string) => Number(/^(?:[a-z]{2}(?:-[a-z]{2,4})?\/)?users\/(\d{1,15})\/profile(?:[/?#]|$)/i.exec(path)?.[1]) || null;

// Only what roblox.com lacks gets a tab; the "roblox" tab shows its page where it was, and Roblox's own
// top bar there reaches Home, Charts, Marketplace, Avatar and Friends.
type Native = "popular" | "playtime" | "limiteds";
const BLOCKED: Record<BrowseBlocked, Key> = {
  create: "browse.blocked.create",
  robux: "browse.blocked.robux",
  money: "browse.blocked.money",
  signOut: "browse.blocked.signOut",
};

/** Browse: roblox.com inside RoALT as one of the accounts (its own session), with RoALT's own tabs
 * over it, back / forward / reload and a search. Create isn't there. `ask`: an account's "Open on
 * roblox.com" (each one new). */
export function BrowseView({ accounts, ask }: { accounts: AccountSummary[]; ask: (BrowseRequest & { n: number }) | null }) {
  const pick = (id: number | null | undefined) => (id != null && accounts.some((a) => a.id === id) ? id : null);
  // An ask not followed yet (Browse opened by it); otherwise Browse goes on where it was.
  const fresh = ask && ask.n > askDone ? ask : null;
  const [accountId, setAccountId] = useState<number | null>(() => pick(fresh?.accountId) ?? pick(lastAccount()) ?? accounts.find((a) => a.status === "valid")?.id ?? accounts[0]?.id ?? null);
  // What to load; `n` loads it again (the same search, a game opened again).
  const [page, setPage] = useState({ path: fresh ? (fresh.path ?? "home") : lastPath, n: 0 });
  // Opened by Play's "Browse games": a Play or Join here fills Play. Leaving Browse ends it.
  const [picking, setPicking] = useState(!!fresh?.pick);
  // What the page shows now (it navigates by itself too).
  const [shown, setShown] = useState(page.path);
  const [error, setError] = useState<string | null>(null);
  const [kind, setKind] = useState<"people" | "games">("people");
  const [query, setQuery] = useState("");
  // One of RoALT's own tabs, over Roblox's page (which stays where it was, hidden).
  const [native, setNative] = useState<Native | null>(null);
  // Counts Browse's page reports: a reload of the same page needs the profile card again.
  const [seq, setSeq] = useState(0);
  // The game being saved as a favourite; it stays set while its dialog closes.
  const [saving, setSavingPlace] = useState<number | null>(null);
  const [saveOpen, setSaveOpen] = useState(false);
  const savingGame = useGame(saving);
  const setSaving = (placeId: number) => {
    setSavingPlace(placeId);
    setSaveOpen(true);
  };

  const go = (path: string) => {
    setNative(null);
    setPage((p) => ({ path, n: p.n + 1 }));
  };
  // Asked again while on screen (another account's "Open on roblox.com").
  const [askSeen, setAskSeen] = useState(ask?.n);
  if (ask && ask.n !== askSeen) {
    setAskSeen(ask.n);
    if (pick(ask.accountId) !== null) setAccountId(ask.accountId);
    setPage((p) => ({ path: ask.path ?? "home", n: p.n + 1 }));
    setPicking(!!ask.pick);
  }
  // The account was removed: the first one left.
  if (accountId !== null && pick(accountId) === null) setAccountId(accounts[0]?.id ?? null);

  useEffect(() => {
    if (ask) askDone = Math.max(askDone, ask.n);
  }, [ask]);
  useEffect(() => {
    if (accountId !== null) remember(accountId);
  }, [accountId]);
  useEffect(() => {
    const un = listen<BrowsePage>("browse://page", (e) => {
      if (e.payload.accountId !== accountId) return;
      if (!launches(e.payload.path)) lastPath = e.payload.path;
      setShown(e.payload.path);
      setSeq((n) => n + 1);
    });
    return () => void un.then((f) => f());
  }, [accountId]);

  // Play or Join on Roblox's page: RoALT launches the account (asking first when it already plays),
  // or hands the game or server to Play when Browse was opened to pick one.
  const move = useMoveConfirm();
  const play = useRef<(p: BrowsePlay) => void>(() => {});
  play.current = ({ accountId: id, target }) => {
    if (picking) return playTarget(target);
    const a = accounts.find((x) => x.id === id);
    if (!a) return;
    // One launch at a time: launchOne would do nothing.
    if (launchStep() !== null) return toast(t("browse.busy"), "error");
    if (move.liveOf(a)) move.ask(a, (from) => void launchOne(a, target, undefined, from));
    else void launchOne(a, target);
  };
  useEffect(() => {
    const un = listen<BrowsePlay>("browse://play", (e) => play.current(e.payload));
    return () => void un.then((f) => f());
  }, []);
  // What Browse refused: Create, and spending while purchases are locked.
  useEffect(() => {
    const un = listen<BrowseBlocked>("browse://blocked", (e) => toast(t(BLOCKED[e.payload]), "error"));
    return () => void un.then((f) => f());
  }, []);

  const search = (e: FormEvent) => {
    e.preventDefault();
    const q = query.trim();
    if (!q) return;
    go(kind === "people" ? `search/users?keyword=${encodeURIComponent(q)}` : `discover?Keyword=${encodeURIComponent(q)}`);
  };
  const nav = (action: "back" | "forward" | "reload") => {
    setNative(null);
    void api.browseNav(action).catch(toastError);
  };
  const account = accounts.find((a) => a.id === accountId);
  const details = useGameDetails();
  const player = usePlayerDetails(account?.id ?? 0);
  const gamePage = native ? null : placeOf(shown);
  const profilePage = native ? null : userOf(shown);
  usePageCard(account?.id ?? null, userOf(shown), seq, useSettings().data?.rolimonsValues ?? false);
  // A game card's buttons: its page here, or a launch as the account browsing.
  const open = (placeId: number) => go(`games/${placeId}`);
  const playPlace = (placeId: number) => account && play.current({ accountId: account.id, target: { kind: "place", placeId, launchData: null } });

  if (!account)
    return (
      <main className="flex-1 min-w-0 page grid place-items-center">
        <p className="text-fg-2">{t("browse.none")}</p>
      </main>
    );

  return (
    <main className="flex-1 min-w-0 min-h-0 flex flex-col browse">
      <div className="browse-bar">
        <span className="flex items-center gap-2 min-w-0">
          <Avatar account={account} size={28} />
          <Select
            aria-label={t("browse.as")}
            className="browse-account"
            value={String(account.id)}
            onChange={(v) => {
              setAccountId(Number(v));
              go("home");
            }}
            options={accounts.map((a) => ({ value: String(a.id), label: accountName(a) }))}
          />
        </span>
        <span className="flex items-center gap-1">
          <IconButton icon="back" label={t("browse.back")} data-tip-side="top" onClick={() => nav("back")} />
          <IconButton icon="back" className="browse-forward" label={t("browse.forward")} data-tip-side="top" onClick={() => nav("forward")} />
          <IconButton icon="refresh" label={t("browse.reload")} data-tip-side="top" onClick={() => nav("reload")} />
          {/* Labelled, not an icon alone: what this page offers should be seen. */}
          {gamePage && (
            <>
              <Button size="sm" variant="ghost" icon="info" onClick={() => details.open(gamePage)}>
                {t("game.detailsTip")}
              </Button>
              <Button size="sm" variant="ghost" icon="link" onClick={() => copyRoaltLink({ kind: "place", placeId: gamePage, launchData: null })}>
                {t("share.copyLink")}
              </Button>
              {/* The page's own Play starts only the account browsing: these take the game to Play, or keep it. */}
              <Button size="sm" variant="ghost" icon="play" onClick={() => playTarget({ kind: "place", placeId: gamePage, launchData: null })}>
                {t("common.playEllipsis")}
              </Button>
              <Button size="sm" variant="ghost" icon="star" aria-haspopup="dialog" onClick={() => setSaving(gamePage)}>
                {t("launch.saveAsFavourite")}
              </Button>
            </>
          )}
          {profilePage && (
            <Button size="sm" variant="ghost" icon="info" onClick={() => player.open(profilePage)}>
              {t("player.detailsTip")}
            </Button>
          )}
        </span>
        <Tabs
          label={t("browse.pages")}
          value={native ?? "roblox"}
          onChange={(id) => setNative(id === "roblox" ? null : (id as Native))}
          items={[
            { value: "roblox", label: t("browse.tab.roblox"), icon: "globe" as const },
            { value: "popular", label: t("browse.tab.popular"), icon: "bolt" as const },
            { value: "playtime", label: t("browse.tab.playtime"), icon: "clock" as const },
            { value: "limiteds", label: t("browse.tab.limiteds"), icon: "gauge" as const },
          ]}
        />
        <form className="browse-search" onSubmit={search}>
          <Segmented
            label={t("browse.searchFor")}
            value={kind}
            onChange={setKind}
            options={[
              { value: "people", label: t("browse.people") },
              { value: "games", label: t("browse.games") },
            ]}
          />
          <Input data-find leading="search" aria-label={t("browse.search")} placeholder={t("browse.search")} value={query} onChange={(e) => setQuery(e.target.value)} />
        </form>
      </div>
      {picking && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 px-1 text-[13px] text-fg-2">
          <span role="status">{t("browse.picking")}</span>
          <Button size="sm" variant="ghost" icon="back" onClick={() => requestNavigation("launch")}>
            {t("browse.backToPlay")}
          </Button>
        </div>
      )}
      {error && !native && <p role="alert" className="alert px-1 pb-2">{error}</p>}
      <div className="browse-stage">
        <RobloxPane accountId={account.id} path={page.path} n={page.n} hidden={!!native} resume={lastPath} onError={setError} />
        {native && (
          <div className="browse-native scroll">
            {native === "popular" ? (
              <PopularTab onOpen={open} onPlay={playPlace} onDetails={details.open} />
            ) : native === "limiteds" ? (
              <LimitedsTab onOpen={go} />
            ) : (
              <PlaytimeTab accounts={accounts} onOpen={open} onPlay={playPlace} onDetails={details.open} />
            )}
          </div>
        )}
      </div>
      {move.dialog}
      {details.dialog}
      {player.dialog}
      {saving !== null && (
        <FavouriteDialog
          // Once the game's name is known (or can't be): the dialog takes its name as it opens.
          open={saveOpen && !savingGame.isLoading}
          onClose={() => setSaveOpen(false)}
          accounts={accounts}
          initial={{ name: savingGame.data?.name ?? t("dashboard.place", { id: saving }), accountIds: [account.id], inTray: false }}
          resolve={() => Promise.resolve({ kind: "place", placeId: saving, launchData: null })}
        />
      )}
    </main>
  );
}
