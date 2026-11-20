import { useId, useState } from "react";
import type { AccountSummary } from "../../api";
import { t } from "../../i18n";
import { announce } from "../../ui/a11y";
import { Button, IconButton } from "../../ui/Button";
import { Drawer } from "../../ui/Drawer";
import { Collapse, Swap } from "../../ui/motion";
import { accountName } from "../accounts/shared";
import { ServerBrowser, type ServerChoice } from "../servers/ServerBrowser";
import { draftFor, type Draft } from "./target";
import "./launch.css";

const FIELD = "servers-game-drawer";

/** Play's server browser, in a sheet from the end side. The server picked becomes the target; a
 * private one masked, so its access code never shows in the field. `asker`: the account that asks
 * Roblox; `count`: how many accounts would join (for the room warnings). */
export function ServerDrawer({ open, onClose, accounts, asker, count, seed, onUse, onServers }: {
  open: boolean;
  onClose: () => void;
  accounts: AccountSummary[];
  asker: AccountSummary | null;
  count: number;
  /** Play's game: the drawer opens on it, else on the game it showed last. */
  seed: number | null;
  onUse: (d: Draft) => void;
  /** The whole Servers view instead. */
  onServers: () => void;
}) {
  const titleId = useId();
  const summaryId = useId();
  const [place, setPlace] = useState(seed);
  const [choice, setChoice] = useState<ServerChoice | null>(null);
  const [nudge, setNudge] = useState(0);
  // Each opening starts afresh: no pick, and a new browser (reopened while the last one is still
  // sliding out, it would keep showing a pick the footer no longer has).
  const [opening, setOpening] = useState(0);
  const [was, setWas] = useState(open);
  if (open !== was) {
    setWas(open);
    if (open) {
      setChoice(null);
      setOpening((n) => n + 1);
      if (seed !== null) setPlace(seed);
    }
  }

  const target = choice?.target ?? null;
  const use = () => {
    if (!target) {
      announce(t("servers.pickServerFirst"), "assertive");
      setNudge((n) => n + 1);
      return;
    }
    onUse(draftFor(target));
    onClose();
  };
  const name = asker ? accountName(asker) : "";
  const warnings = [
    choice?.restricted ? t("servers.noJoin", { name }) : null,
    target?.kind === "privateAccess" && count > 1 ? t("servers.accessCheckedFor", { name }) : null,
    choice && choice.free !== null && count > choice.free ? t("servers.onlyFreeSlots", { count: choice.free, accounts: t("servers.nAccounts", { count }) }) : null,
  ];

  return (
    <Drawer open={open} onClose={onClose} labelledBy={titleId} focus={`#${FIELD}`}>
      <div className="dlg-head">
        <h2 id={titleId} className="dlg-title">{t("play.browse")}</h2>
        <IconButton icon="servers" label={t("app.nav.servers")} onClick={() => (onClose(), onServers())} />
        <IconButton icon="x" label={t("common.close")} onClick={onClose} />
      </div>
      <div className="drawer-main">
        <ServerBrowser key={opening} variant="drawer" accounts={accounts} asker={asker?.id} place={place} setPlace={setPlace} onChoice={setChoice} onSubmit={use} />
      </div>
      <div className="drawer-foot">
        <div className="flex-[1_1_220px] min-w-0">
          <p id={summaryId} className={`flex min-h-7 items-center text-[13px] font-medium ${target ? "" : "text-fg-3"} ${nudge ? (nudge % 2 ? "shake" : "shake2") : ""}`}>
            <Swap k={choice?.summary ?? ""} variant="rise" className="min-w-0">
              <span className={`truncate ${target?.kind === "server" ? "mono" : ""}`}>{choice?.summary}</span>
            </Swap>
          </p>
          <div aria-live="polite">
            {warnings.map((w, i) => (
              <Collapse key={i} open={!!w}>
                <p className="alert pt-1.5 text-[13px]">{w}</p>
              </Collapse>
            ))}
          </div>
        </div>
        <Button onClick={onClose}>{t("common.cancel")}</Button>
        <Button variant="primary" aria-disabled={!target || undefined} aria-describedby={target ? undefined : summaryId} onClick={use}>
          {t("servers.useServer")}
        </Button>
      </div>
    </Drawer>
  );
}
