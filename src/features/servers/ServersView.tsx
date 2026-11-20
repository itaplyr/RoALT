import { useId, useState, type Dispatch, type SetStateAction } from "react";
import { AnimatePresence, motion } from "motion/react";
import type { AccountSummary } from "../../api";
import { requestNavigation } from "../../app/navigation";
import { publicBlock, useBulk, useInstances, useMultiStatus, useNetwork, useRobloxClient, useSettings } from "../../data";
import { t } from "../../i18n";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import { Collapse, Morph, SPRING, Swap, TWEEN } from "../../ui/motion";
import { MenuLabel, MenuRadioItem, MenuSeparator, Popover, useAnchor } from "../../ui/Popover";
import { accountName } from "../accounts/shared";
import { AccountPicker } from "../launch/AccountPicker";
import { LaunchButton } from "../launch/LaunchButton";
import { launchBlocked, launchOne, launchSame } from "../launch/launchQueue";
import { useMoveConfirm } from "../launch/MoveDialog";
import { MultiNotice } from "../launch/MultiNotice";
import { isLive, isRunning } from "../launch/Running";
import { ServerBrowser, type ServerChoice } from "./ServerBrowser";

/** The Servers view: the server browser, then the bar that joins the picked accounts to the picked
 * server. A join stays here; the island shows its progress. */
export function ServersView({ accounts, picked, setPicked, place, setPlace }: {
  accounts: AccountSummary[];
  picked: number[];
  setPicked: Dispatch<SetStateAction<number[]>>;
  place: number | null;
  setPlace: (placeId: number) => void;
}) {
  const multi = useMultiStatus().data;
  const batch = useBulk().data;
  const instances = useInstances().data ?? [];
  const client = useRobloxClient().data;
  const network = useNetwork().data;
  const settings = useSettings().data;
  const [choice, setChoice] = useState<ServerChoice>(() => ({ target: null, free: null, summary: t("servers.noServerPicked"), restricted: false }));
  const [asId, setAsId] = useState<number | null>(null);
  const { target, free, summary, restricted } = choice;

  const has = new Set(picked);
  const live = new Set(instances.filter(isLive).map((i) => i.accountId));
  const valid = accounts.filter((a) => a.status === "valid");
  // In list order, which is also the batch's. A playing account is only ever picked on its own
  // (AccountPicker): joining moves its client, asked first.
  const mine = valid.filter((a) => has.has(a.id));
  const move = useMoveConfirm();
  const n = mine.length;
  // Who asks Roblox (the signed-in sorts, private servers, the access check). A pick that started
  // playing stays it, so a join doesn't reload the list under the user.
  const asker = mine.find((a) => a.id === asId) ?? mine[0];
  const askerName = asker ? accountName(asker) : "";
  const noJoin = t("servers.noJoin", { name: askerName });
  // Sent besides the asker: Roblox checked none of them.
  const others = mine.some((a) => a.id !== asker?.id);
  // With multi-instance off, one more client ends the one that runs.
  const replaces = n === 1 && instances.some((i) => isRunning(i) && i.accountId !== mine[0].id);
  // The private list told the asker it can't join, and only the asker goes.
  const refused = restricted && n > 0 && !others;
  const pub = publicBlock(network, settings);
  const multiBlock = launchBlocked(n > 1 || (replaces && !!multi?.enabled), multi, batch);
  const blocked = !target || !n || refused || !!(pub ?? multiBlock);
  // Multi-instance has its own notice, with the way out; no valid account, its own line.
  const reason =
    pub ??
    (batch && !batch.done ? t("launch.batchRunning") : null) ??
    // The button's label already says the first missing pick.
    (!target ? (n ? null : t("servers.pickServerFirst")) : refused ? noJoin : null);
  // Shown by the multi-instance notice or the no-valid-account line; pressing the button says it too.
  const noted = reason ? null : (multiBlock ?? (valid.length ? null : t("servers.noValidAccount")));
  const warnings = [
    restricted && others ? noJoin : null,
    target?.kind === "privateAccess" && others ? t("servers.accessCheckedFor", { name: askerName }) : null,
    free !== null && n > free ? t("servers.onlyFreeSlots", { count: free, accounts: t("servers.nAccounts", { count: n }) }) : null,
    n > 1 && client?.outdated ? t("servers.outdated") : null,
  ];

  const join = () => {
    if (!target) return;
    const one = mine[0];
    if (n > 1) void launchSame(mine, target);
    else if (one && move.liveOf(one)) move.ask(one, (from) => void launchOne(one, target, undefined, from));
    else if (one) void launchOne(one, target);
  };

  return (
    <main className="scroll flex-1 min-w-0 page pb-6 flex flex-col gap-4">
      <ServerBrowser accounts={accounts} asker={asker?.id} place={place} setPlace={setPlace} onChoice={setChoice} />
      <div className="flex-none flex flex-col">
        {/* The notice's room opens and closes with it. */}
        <Morph axis="height">
          <div className="has-[.notice]:pb-3">
            <MultiNotice wanted={n > 1} replaces={replaces} />
          </div>
        </Morph>
        <section className="panel srv-send px-4 py-3">
          <div className="srv-send-grid">
            <div className="srv-send-where flex items-center gap-3 min-w-0">
              <p className="relative flex items-center gap-2.5 flex-[0_1_13rem] min-w-0 text-[13px] font-medium">
                <span aria-hidden="true" className="srv-send-mark" data-on={target ? "" : undefined} />
                <Swap k={summary} variant="rise" className="min-w-0">
                  <span className={`truncate ${target?.kind === "server" ? "mono" : target ? "" : "text-fg-3"}`}>{summary}</span>
                </Swap>
              </p>
              {/* A choice only once several accounts are picked. */}
              <AnimatePresence initial={false}>
                {mine.length > 1 && asker && (
                  <motion.span key="as" className="flex min-w-0" initial={{ opacity: 0, scale: 0.94 }} animate={{ opacity: 1, scale: 1, transition: SPRING.snappy }} exit={{ opacity: 0, transition: TWEEN.exit }}>
                    <AsMenu accounts={mine} asker={asker} live={live} onPick={setAsId} />
                  </motion.span>
                )}
              </AnimatePresence>
            </div>
            <div className="srv-send-who min-w-0">
              {valid.length ? (
                <AccountPicker variant="chips" accounts={accounts} picked={picked} onPicked={setPicked} label={t("servers.accountsToSend")} />
              ) : (
                <p className="flex flex-wrap items-center gap-x-3 gap-y-2 min-h-7 text-[13px] text-fg-2">
                  {t("servers.noValidAccount")}
                  <Button size="sm" onClick={() => requestNavigation("accounts")}>{t("app.nav.accounts")}</Button>
                </p>
              )}
            </div>
            <LaunchButton label={!n ? t("play.pickFirst") : !target ? t("servers.pickServerFirst") : t("servers.joinWith", { count: n })} disabled={blocked} reason={reason} noted={noted} shortcut onLaunch={join} className="srv-send-go" />
          </div>
          <div aria-live="polite">
            {warnings.map((w, i) => (
              <Collapse key={i} open={!!w}>
                <p className="alert pt-2.5 text-[13px]">{w}</p>
              </Collapse>
            ))}
          </div>
        </section>
      </div>
      {move.dialog}
    </main>
  );
}

/** As whom Roblox is asked: one of the picked accounts (a playing one stays askable, and says so). */
function AsMenu({ accounts, asker, live, onPick }: { accounts: AccountSummary[]; asker: AccountSummary; live: Set<number>; onPick: (id: number) => void }) {
  const menu = `${useId()}as`;
  const anchor = useAnchor();
  const hint = t("servers.asHint");
  return (
    <>
      <button type="button" className="btn btn-xs max-w-full" popoverTarget={menu} style={anchor.style}>
        <Swap k={asker.id} variant="rise" className="min-w-0">
          <span className="truncate">{t("servers.asName", { name: accountName(asker) })}</span>
        </Swap>
        <Icon name="chevron" size={12} />
      </button>
      <Popover
        id={menu}
        anchor={anchor.name}
        placement="above-start"
        role="menu"
        label={hint}
      >
        {accounts.map((a) => (
          <MenuRadioItem key={a.id} checked={a.id === asker.id} onSelect={() => (onPick(a.id), document.getElementById(menu)?.hidePopover())}>
            {accountName(a)}
            {live.has(a.id) && <span className="text-fg-3"> · {t("play.already")}</span>}
          </MenuRadioItem>
        ))}
        <MenuSeparator />
        <MenuLabel>{hint}</MenuLabel>
      </Popover>
    </>
  );
}
