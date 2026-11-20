// The card on a player's profile in Browse's page (src-tauri/src/browse_card.js): the same numbers
// as Player details, sent as text in the UI's language.
import { useEffect } from "react";
import { api, type PlayerStats, type PlayerValue } from "../../api";
import type { PageCard } from "../../bindings/PageCard";
import { qk, queryClient } from "../../data";
import { fmtNum, t } from "../../i18n";
import { playerFacts } from "./PlayerDetails";

function cardOf(p: PlayerStats, v: PlayerValue | null, rolimons: boolean): PageCard {
  const rows: [string, string][] = playerFacts(p).map(([k, value]) => [t(k), value ?? "–"]);
  const worth = v?.value;
  if (worth) {
    rows.push([t("value.rap"), fmtNum(worth.rap)], [t("value.items"), fmtNum(worth.count)]);
    if (rolimons && worth.value !== null) rows.push([t("value.value"), fmtNum(worth.value)]);
  }
  const note = !v ? t("player.counting") : v.busy ? t("player.busy") : v.private || !worth ? t("player.private") : worth.partial ? t("value.partial") : null;
  return {
    userId: p.userId,
    title: p.displayName,
    toggle: t("player.detailsTip"),
    rows,
    note,
    topTitle: t("value.top"),
    top: (worth?.top ?? []).slice(0, 5).map((i) => [i.name, fmtNum(i.rolimons?.value ?? i.rap)]),
  };
}

/** Fills the card while Browse shows profile `userId` as `accountId`; `seq` changes with each page
 *  Browse reports (a reload starts the card afresh). The limiteds are counted as the drawer counts. */
export function usePageCard(accountId: number | null, userId: number | null, seq: number, rolimons: boolean) {
  useEffect(() => {
    if (accountId === null || userId === null) {
      void api.browseCard(null).catch(() => {});
      return;
    }
    let live = true;
    const send = (c: PageCard) => live && void api.browseCard(c).catch(() => {});
    void (async () => {
      const p = await queryClient.fetchQuery({ queryKey: qk.player(userId), queryFn: () => api.playerStats(accountId, userId), staleTime: 60_000 }).catch(() => null);
      if (!p || !live) return;
      send(cardOf(p, null, rolimons));
      const v = await queryClient.fetchQuery({ queryKey: qk.playerValue(accountId, userId), queryFn: () => api.playerValue(accountId, userId), staleTime: Infinity }).catch(() => null);
      if (v) send(cardOf(p, v, rolimons));
    })();
    return () => {
      live = false;
    };
  }, [accountId, userId, seq, rolimons]);
}
