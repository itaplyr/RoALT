import type { ReactNode } from "react";
import type { AccountSummary, Favourite } from "../../api";
import { requestNavigation } from "../../app/navigation";
import { publicBlock, useBulk, useFavourites, useGame, useInstances, useMultiStatus, useNetwork, useSettings } from "../../data";
import { fmtList, t, type Key } from "../../i18n";
import { Button } from "../../ui/Button";
import { ListPresence } from "../../ui/motion";
import { accountName, Avatar } from "../accounts/shared";
import { favouriteAccountIds, FavIcon } from "../launch/Favourites";
import { LaunchButton } from "../launch/LaunchButton";
import { launchBlocked, launchOne, launchSame, sessionFps } from "../launch/launchQueue";
import { isLive } from "../launch/Running";
import { describeTarget, draftFor, targetPlace } from "../launch/target";

const STEPS: [Key, Key][] = [
  ["home.step1", "home.step1Text"],
  ["home.step2", "home.step2Text"],
  ["home.step3", "home.step3Text"],
];

/** A favourite's icon, name and what it opens: the game by name once RoALT knows it. */
function FavHead({ f }: { f: Favourite }) {
  const look = describeTarget(f.target);
  const game = useGame(targetPlace(f.target)).data;
  return (
    <>
      <span className="flex-none text-fg-2 grid place-items-center w-7">
        <FavIcon target={f.target} size={28} />
      </span>
      <span className="flex-[1_1_180px] min-w-0 flex flex-col">
        <span className="font-medium truncate">{f.name}</span>
        <span className="text-[13px] text-fg-2 truncate">{look.kind} · {game?.name ?? look.detail}</span>
      </span>
    </>
  );
}

/** The favourites, each a click from playing with its default accounts (and Play's FPS cap for this
 * session), or from Play with it filled in. */
function QuickPlay({ accounts, onPick }: { accounts: AccountSummary[]; onPick?: (f: Favourite) => void }) {
  const favs = useFavourites();
  const instances = useInstances().data ?? [];
  const multi = useMultiStatus().data;
  const batch = useBulk().data;
  const settings = useSettings().data;
  const network = useNetwork().data;
  if (favs.isError) return <p className="text-[13px] text-fg-2">{t("launch.fav.loadFailed")}</p>;
  if (!favs.data?.length) return null;
  const live = new Set(instances.filter(isLive).map((i) => i.accountId));
  const byId = new Map(accounts.map((a) => [a.id, a]));
  return (
    <section aria-labelledby="home-favs" className="flex flex-col gap-2.5">
      <h2 id="home-favs" className="section-title">{t("launch.fav.title")}</h2>
      <ListPresence
        as="ul"
        items={favs.data}
        getKey={(f) => f.id}
        className="panel home-favs"
        itemClassName="home-fav"
        render={(f) => {
          const who = favouriteAccountIds(f, accounts).map((id) => byId.get(id)).filter((a): a is AccountSummary => !!a);
          const ready = who.filter((a) => a.status === "valid" && !live.has(a.id));
          const where = { draft: draftFor(f.target, f.id, f.name), setDraft: () => {} };
          const block = publicBlock(network, settings) ?? launchBlocked(ready.length > 1, multi, batch);
          return (
            <div className="home-fav-row">
              <FavHead f={f} />
              {who.length > 0 && (
                <span className="home-who" data-tip={fmtList(who.map(accountName))}>
                  {who.slice(0, 4).map((a) => (
                    <Avatar key={a.id} account={a} size={22} />
                  ))}
                  {who.length > 4 && <span className="home-who-more">+{who.length - 4}</span>}
                  <span className="sr-only">{fmtList(who.map(accountName))}</span>
                </span>
              )}
              <div className="flex flex-wrap items-start justify-end gap-1.5">
                {onPick && (
                  <Button variant="ghost" onClick={() => onPick(f)}>
                    {t("common.playEllipsis")}
                  </Button>
                )}
                {ready.length > 0 && (
                  <LaunchButton
                    label={t("launch.fav.playNow", { name: f.name })}
                    disabled={block !== null}
                    reason={block}
                    // Its own way of launching, where it has one; else Play's FPS cap for this session.
                    onLaunch={() => {
                      const fpsCap = f.options.fpsCap ?? sessionFps();
                      void (ready.length === 1 ? launchOne(ready[0], where, fpsCap) : launchSame(ready, where, { sameServer: f.options.sameServer ?? undefined, fpsCap }));
                    }}
                  />
                )}
              </div>
            </div>
          );
        }}
      />
    </section>
  );
}

/** Home with nothing running: three steps to a first game (no accounts), the way back in (all signed
 * out), or the way to a game and the favourites (idle). `toolbar` and `extra` go in the header,
 * `notice` under it. */
export function Welcome({ accounts, toolbar, extra, notice, onAdd, onImport, onAccounts, onPickFavourite }: {
  accounts: AccountSummary[];
  toolbar: ReactNode;
  extra: ReactNode;
  notice: ReactNode;
  onAdd: () => void;
  onImport: () => void;
  onAccounts: () => void;
  onPickFavourite?: (f: Favourite) => void;
}) {
  const head = (title: string, lead: string, end?: ReactNode) => (
    <>
      <header className="home-head">
        <div className="min-w-0 max-w-[640px]">
          <h1 className="page-title">{title}</h1>
          <p className="page-sub">{lead}</p>
        </div>
        {end && <div className="home-head-end">{end}</div>}
      </header>
      {notice}
    </>
  );

  if (!accounts.length)
    return (
      <>
        {head(t("home.welcome"), t("home.welcomeLead"))}
        <ol className="panel home-steps">
          {STEPS.map(([title, text], n) => (
            <li key={title} className="home-step">
              <span className="num">{n + 1}</span>
              <span className="flex-[1_1_260px] min-w-0 flex flex-col gap-0.5">
                <span className="font-semibold">{t(title)}</span>
                <span className="text-[13px] text-fg-2">{t(text)}</span>
              </span>
              {n === 0 && (
                <Button variant="primary" icon="plus" onClick={onAdd}>
                  {t("accounts.add.title")}
                </Button>
              )}
            </li>
          ))}
        </ol>
        <p className="flex flex-wrap items-center gap-x-1 text-fg-2">
          {t("home.importLead")}
          <Button variant="ghost" size="md" icon="download" onClick={onImport}>
            {t("home.import")}
          </Button>
        </p>
      </>
    );

  if (accounts.every((a) => a.status !== "valid"))
    return (
      <>
        {head(t("home.signedOutTitle"), t("home.signedOutLead"), <>{toolbar}{extra}</>)}
        <div>
          <Button variant="primary" icon="accounts" onClick={onAccounts}>
            {t("app.nav.accounts")}
          </Button>
        </div>
      </>
    );

  return (
    <>
      {head(t("home.idle"), t("home.idleLead"), <>{toolbar}{extra}</>)}
      <div>
        <Button variant="primary" icon="play" onClick={() => requestNavigation("launch")}>
          {t("home.chooseGame")}
        </Button>
      </div>
      <QuickPlay accounts={accounts} onPick={onPickFavourite} />
    </>
  );
}
