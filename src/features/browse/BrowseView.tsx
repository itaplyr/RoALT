import "./browse.css";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type AccountSummary, type BrowseBlocked, type BrowsePage, type BrowsePlay } from "../../api";
import type { BrowseRequest } from "../../app/navigation";
import { useSettings } from "../../data";
import { t, type Key } from "../../i18n";
import { Button, IconButton } from "../../ui/Button";
import { Segmented } from "../../ui/controls";
import { Input, Select } from "../../ui/Field";
import type { IconName } from "../../ui/Icon";
import { readStored, writeStored } from "../../ui/stored";
import { Tabs } from "../../ui/Tabs";
import { toast, toastError } from "../../ui/Toast";
import { accountName, Avatar } from "../accounts/shared";
import { launchOne, launchStep } from "../launch/launchQueue";
import { useMoveConfirm } from "../launch/MoveDialog";
import { useGameDetails } from "./GameDetails";
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

/** Roblox's own pages, as tabs. */
const PAGES: { id: string; path: string; label: Key; icon: IconName }[] = [
  { id: "home", path: "home", label: "browse.tab.home", icon: "home" },
  { id: "games", path: "charts", label: "browse.tab.games", icon: "grid" },
  { id: "market", path: "catalog", label: "browse.tab.market", icon: "star" },
  { id: "avatar", path: "my/avatar", label: "browse.tab.avatar", icon: "palette" },
  { id: "friends", path: "users/friends", label: "browse.tab.friends", icon: "accounts" },
];
type Native = "popular" | "playtime";
const BLOCKED: Record<BrowseBlocked, Key> = {
  create: "browse.blocked.create",
  robux: "browse.blocked.robux",
  money: "browse.blocked.money",
  signOut: "browse.blocked.signOut",
};

/** The tab a page belongs to; "" for none of them (a game, a profile). */
const tabOf = (path: string) => PAGES.find((p) => path === p.path || path.startsWith(`${p.path}/`) || path.startsWith(`${p.path}?`))?.id ?? "";

/** Browse: roblox.com inside RoALT as one of the accounts (its own session), with Roblox's main pages
 * as tabs, back / forward / reload and a search. Create isn't there. `ask`: an account's "Open on
 * roblox.com" (each one new). */
export function BrowseView({ accounts, ask }: { accounts: AccountSummary[]; ask: (BrowseRequest & { n: number }) | null }) {
  const pick = (id: number | null | undefined) => (id != null && accounts.some((a) => a.id === id) ? id : null);
  // An ask not followed yet (Browse opened by it); otherwise Browse goes on where it was.
  const fresh = ask && ask.n > askDone ? ask : null;
  const [accountId, setAccountId] = useState<number | null>(() => pick(fresh?.accountId) ?? pick(lastAccount()) ?? accounts.find((a) => a.status === "valid")?.id ?? accounts[0]?.id ?? null);
  // What to load; `n` loads it again (the same search, a game opened again).
  const [page, setPage] = useState({ path: fresh ? (fresh.path ?? "home") : lastPath, n: 0 });
  // What the page shows now (it navigates by itself too).
  const [shown, setShown] = useState(page.path);
  const [error, setError] = useState<string | null>(null);
  const [kind, setKind] = useState<"people" | "games">("people");
  const [query, setQuery] = useState("");
  // One of RoALT's own tabs, over Roblox's page (which stays where it was, hidden).
  const [native, setNative] = useState<Native | null>(null);
  // Counts Browse's page reports: a reload of the same page needs the profile card again.
  const [seq, setSeq] = useState(0);

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

  // Play or Join on Roblox's page: RoALT launches the account (asking first when it already plays).
  const move = useMoveConfirm();
  const play = useRef<(p: BrowsePlay) => void>(() => {});
  play.current = ({ accountId: id, target }) => {
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
            <Button size="sm" variant="ghost" icon="info" onClick={() => details.open(gamePage)}>
              {t("game.detailsTip")}
            </Button>
          )}
          {profilePage && (
            <Button size="sm" variant="ghost" icon="info" onClick={() => player.open(profilePage)}>
              {t("player.detailsTip")}
            </Button>
          )}
        </span>
        <Tabs
          label={t("browse.pages")}
          value={native ?? tabOf(shown)}
          onChange={(id) => (id === "popular" || id === "playtime" ? setNative(id) : go(PAGES.find((p) => p.id === id)!.path))}
          items={[
            ...PAGES.map((p) => ({ value: p.id, label: t(p.label), icon: p.icon })),
            { value: "popular", label: t("browse.tab.popular"), icon: "bolt" as const },
            { value: "playtime", label: t("browse.tab.playtime"), icon: "clock" as const },
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
      {error && !native && <p role="alert" className="alert px-1 pb-2">{error}</p>}
      <div className="browse-stage">
        <RobloxPane accountId={account.id} path={page.path} n={page.n} hidden={!!native} resume={lastPath} onError={setError} />
        {native && (
          <div className="browse-native scroll">
            {native === "popular" ? (
              <PopularTab onOpen={open} onPlay={playPlace} onDetails={details.open} />
            ) : (
              <PlaytimeTab accounts={accounts} onOpen={open} onPlay={playPlace} onDetails={details.open} />
            )}
          </div>
        )}
      </div>
      {move.dialog}
      {details.dialog}
      {player.dialog}
    </main>
  );
}
