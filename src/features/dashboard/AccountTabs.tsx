import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api, asError, type AccountSummary, type BulkProgress, type Instance } from "../../api";
import { publicBlock, qk, useBulk, useInstances, useMultiStatus, useNetwork, useSettings, useUpdateAccount } from "../../data";
import { isRtl, t } from "../../i18n";
import { NumberField, Switch, TextField } from "../../ui/controls";
import { Icon } from "../../ui/Icon";
import { menuKeys } from "../../ui/menu";
import { toast, toastError } from "../../ui/Toast";
import { accountName, Avatar } from "../accounts/shared";
import { FavouritesRow } from "../launch/Favourites";
import { launchBlocked, launched, launchFailed, MARK } from "../launch/LaunchView";
import { isLive, isRunning, MultiNotice, STATES } from "../launch/Running";
import { draftFor, readDraft, resolveTarget, type Draft } from "../launch/target";
import { storedText, type HomeTab, type HomeTabs } from "./homeLayout";

const tabId = (id: number | null) => `home-tab-${id ?? "overview"}`;
const inFlight = (tab: HomeTab) => tab.status === "queued" || tab.status === "launching";

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

/** One account's tab: its game, its options, and its launch or its window. */
function TabBody({ tab, account: a, accounts, instances, block, onDraft, onLaunch }: {
  tab: HomeTab;
  account: AccountSummary;
  accounts: AccountSummary[];
  instances: Instance[];
  /** Why no launch can start now, or null. */
  block: string | null;
  onDraft: (f: Draft | ((d: Draft) => Draft)) => void;
  onLaunch: () => void;
}) {
  const update = useUpdateAccount();
  const settings = useSettings().data;
  const { draft } = tab;
  const { parsed, place, look, usable, pending } = readDraft(draft);
  const mine = instances.filter((i) => i.accountId === a.id);
  const inst = mine.find(isLive) ?? mine.find(isRunning);
  const live = !!inst && isLive(inst);
  const busy = inFlight(tab);
  const name = accountName(a);
  const valid = a.status === "valid";
  const [state, color] = inst ? STATES[inst.state] : [null, undefined];
  // On screen only, because it can't be read back after a restart.
  const unsaved = usable && !pending && !storedText(draft);
  return (
    <>
      <section aria-labelledby="home-tab-where" className="enter panel flex flex-col gap-4 p-5">
        <div className="flex flex-wrap items-center gap-3">
          <Avatar account={a} size={36} />
          <div className="flex-[1_1_160px] min-w-0 flex flex-col">
            <h2 id="home-tab-where" className="section-title truncate">{name}</h2>
            <span className="mono text-[13px] text-dim truncate">@{a.username}</span>
          </div>
          {state && (
            <span className="state flex-none text-[13px]" style={{ color }}>
              <span aria-hidden="true" className="led" />
              {state}
            </span>
          )}
          {inst && <button className="btn" onClick={() => api.instanceShow(inst.id, "focus").catch(toastError)}>{t("home.show")}</button>}
          {!live && (
            <button className="btn btn-primary min-w-0" disabled={!valid || !(usable || pending) || busy || block !== null} title={valid ? block ?? undefined : t("launch.onlyValid")} onClick={onLaunch}>
              <Icon name="play" size={12} fill />
              <span className="truncate">{busy ? t("launch.launching") : usable || pending ? t("launch.launchAs", { name }) : t("play.chooseWhere")}</span>
            </button>
          )}
        </div>
        <p className="empty:hidden text-[13px]" role="status">
          {tab.status === "queued" ? <span className="text-dim">{t("dashboard.tabs.queued")}</span>
            : tab.status === "failed" && !live ? <span className="text-orange">{t("dashboard.tabs.failed")}</span>
            : null}
        </p>
        <div className="flex flex-col gap-2">
          <label className="label" htmlFor="home-tab-target">{t("launch.targetLabel")}</label>
          <input
            id="home-tab-target"
            className="field mono"
            placeholder={t("launch.targetPlaceholder")}
            spellCheck={false}
            autoComplete="off"
            disabled={busy}
            value={draft.text}
            onChange={(e) => {
              const v = e.target.value;
              onDraft((d) => ({ ...d, text: v, favId: null, note: null }));
            }}
          />
          {draft.text.trim() && (
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 min-h-11">
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
            </div>
          )}
          {unsaved && <p className="text-[13px] text-dim">{t("dashboard.tabs.notSaved")}</p>}
          {parsed?.kind === "jobIdOnly" && (
            <label className="flex flex-col gap-1.5 mt-2 max-w-[320px]">
              <span className="label">{t("launch.placeId")}</span>
              <input
                className="field mono"
                inputMode="numeric"
                spellCheck={false}
                placeholder={t("launch.placeIdPlaceholder")}
                value={draft.place}
                disabled={busy}
                aria-invalid={draft.place.trim() !== "" && place === null}
                onChange={(e) => {
                  const v = e.target.value;
                  onDraft((d) => ({ ...d, place: v, favId: null, note: null }));
                }}
              />
            </label>
          )}
        </div>
        <FavouritesRow accounts={accounts} active={draft.favId} onPick={(f) => !busy && onDraft(draftFor(f.target, f.id, f.name))} />
      </section>

      <section aria-labelledby="home-tab-options" className="panel flex flex-col gap-4 p-5">
        <h2 id="home-tab-options" className="section-title">{t("dashboard.tabs.options")}</h2>
        <div className="well px-[18px] py-4">
          <div className="setting">
            <div className="setting-text">
              <p className="font-medium">{t("accounts.detail.fpsCap")}</p>
              <p className="text-[13px] text-dim">{t("accounts.detail.fpsCapHint")}</p>
            </div>
            <NumberField unit={t("accounts.detail.fpsUnit")} short="FPS" min={0} max={360} value={a.fpsCap ?? 0} onCommit={(fpsCap) => update.mutate({ id: a.id, patch: { fpsCap } })} />
          </div>
          <div className="setting">
            <div className="setting-text">
              <p className="font-medium">{t("accounts.detail.launchArgs")}</p>
            </div>
            <TextField className="flex-[1_1_240px] w-auto" label={t("accounts.detail.launchArgs")} placeholder={t("accounts.detail.launchArgsDefault")} value={a.launchArgs ?? ""} onCommit={(launchArgs) => update.mutateAsync({ id: a.id, patch: { launchArgs } })} />
          </div>
          <div className="setting">
            <div className="setting-text">
              <p className="font-medium">{t("accounts.detail.fullQuality")}</p>
              <p className="text-[13px] text-dim">{t("accounts.detail.fullQualityHint")}</p>
            </div>
            <Switch label={t("accounts.detail.fullQuality")} checked={a.fullQuality} onChange={(fullQuality) => update.mutate({ id: a.id, patch: { fullQuality } })} />
          </div>
          <div className="setting">
            <div className="setting-text">
              <p className="font-medium">{t("accounts.detail.bringBack")}</p>
              <p className="text-[13px] text-dim">{settings && !settings.autoRejoin && !settings.autoRestartOnCrash ? t("accounts.detail.bringBackOff") : t("accounts.detail.bringBackHint")}</p>
            </div>
            <Switch label={t("accounts.detail.bringBack")} checked={!a.allowDisconnect} onChange={(v) => update.mutate({ id: a.id, patch: { allowDisconnect: !v } })} />
          </div>
        </div>
      </section>
    </>
  );
}

/** Home's tab strip: Overview (`children`, what Home always shows), then one tab per added account,
 * each with its own game and options, and a launch of all of them. */
export function AccountTabs({ accounts, home, onAdd, children }: { accounts: AccountSummary[]; home: HomeTabs; onAdd: () => void; children: ReactNode }) {
  const qc = useQueryClient();
  const instances = useInstances().data ?? [];
  const multi = useMultiStatus().data;
  const batch = useBulk().data;
  const settings = useSettings().data;
  const network = useNetwork().data;
  const strip = useRef<HTMLDivElement>(null);
  const [who, setWho] = useState("");
  const byId = new Map(accounts.map((a) => [a.id, a]));
  const { tabs, active } = home;
  const open = tabs.find((tab) => tab.accountId === active);
  const ready = tabs.filter((tab) => {
    const { usable, pending } = readDraft(tab.draft);
    return byId.get(tab.accountId)?.status === "valid" && (usable || pending) && !instances.some((i) => i.accountId === tab.accountId && isLive(i));
  });
  const launching = tabs.some(inFlight);
  // With multi-instance off, one more client ends the one that runs.
  const replaces = (tab: HomeTab) => instances.some((i) => isRunning(i) && i.accountId !== tab.accountId);
  const allReplace = ready.some(replaces);
  const blockFor = (several: boolean, replacing: boolean) => publicBlock(network, settings) ?? launchBlocked(several || (replacing && !!multi?.enabled), multi, batch);
  const otherBlock = publicBlock(network, settings) ?? (batch && !batch.done ? t("launch.batchRunning") : null);
  const free = accounts.filter((a) => !tabs.some((tab) => tab.accountId === a.id));
  const needle = who.trim().toLowerCase();
  const picks = needle ? free.filter((a) => `${a.alias}\n${a.displayName}\n${a.username}`.toLowerCase().includes(needle)) : free;
  /** After a tab was added or closed, once that is on screen: what had the focus is gone. */
  const focusSelected = () => requestAnimationFrame(() => strip.current?.querySelector<HTMLElement>("[role=tab][aria-selected=true]")?.focus());

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
  }, []);

  // Coming back to Home, the strip starts at its left end.
  useEffect(() => {
    strip.current?.querySelector("[role=tab][aria-selected=true]")?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [active]);

  /** One after another: `BulkRequest` has a single target, and each tab has its own. The backend
   * keeps the launch delay between them. */
  const launch = async (list: HomeTab[]) => {
    if (launching) return;
    home.setStatus(list.map((tab) => tab.accountId), "queued");
    for (const [n, tab] of list.entries()) {
      const id = tab.accountId;
      const a = byId.get(id);
      const rest = () => home.setStatus(list.slice(n).map((x) => x.accountId), undefined);
      // A batch can have started meanwhile; launches go one at a time.
      const running = qc.getQueryData<BulkProgress | null>(qk.bulk);
      if (!a || (running && !running.done)) {
        rest();
        if (a) toast(t("launch.batchRunning"), "error");
        return;
      }
      home.setStatus([id], "launching");
      let asked = false;
      try {
        const target = await resolveTarget(tab.draft, (f) => home.setDraft(id, f));
        asked = true;
        launched(a, await api.launch(a.id, target, tab.draft.favId ?? undefined));
        home.setStatus([id], undefined);
        if (tab.draft.favId !== null) qc.invalidateQueries({ queryKey: qk.favourites });
      } catch (e) {
        launchFailed(e);
        const { kind } = asError(e);
        // The backend refusing: mostly Kill all, or an auto-rejoin or another launch under way. They
        // have no kind of their own, and the rest must not go on starting. A dead share link reads
        // the same, so it stops the rest too; its toast says why.
        const refused = asked && kind === "invalid";
        home.setStatus([id], refused ? undefined : "failed");
        if (refused || kind === "cancelled" || kind === "locked") {
          home.setStatus(list.slice(n + 1).map((x) => x.accountId), undefined);
          return;
        }
      }
    }
  };

  const allBlock = blockFor(ready.length > 1, allReplace);
  const openAccount = open && byId.get(open.accountId);
  return (
    <>
      <div className="flex items-center gap-2.5">
        <div ref={strip} role="tablist" aria-label={t("dashboard.tabs.label")} className="tab-strip" onKeyDown={tabKeys}>
          <div className="tab">
            <button type="button" role="tab" id={tabId(null)} aria-selected={active === null} aria-controls="home-tab-panel" tabIndex={active === null ? 0 : -1} onClick={() => home.setActive(null)}>
              {t("dashboard.tabs.overview")}
            </button>
          </div>
          {tabs.map((tab) => {
            const a = byId.get(tab.accountId);
            if (!a) return null;
            const inst = instances.find((i) => i.accountId === a.id && isLive(i));
            const mark = inFlight(tab) ? "var(--dim)" : inst ? STATES[inst.state][1] : tab.status === "failed" ? "var(--orange)" : null;
            const selected = active === a.id;
            const name = accountName(a);
            return (
              <div key={a.id} className="tab">
                <button type="button" role="tab" id={tabId(a.id)} aria-selected={selected} aria-controls="home-tab-panel" tabIndex={selected ? 0 : -1} title={name} onClick={() => home.setActive(a.id)}>
                  {mark && <span aria-hidden="true" className={`led ${inFlight(tab) ? "pulse" : ""}`} style={{ color: mark }} />}
                  <span className="truncate">{name}</span>
                </button>
                <button
                  type="button"
                  className="wc w-8 h-8 me-0.5"
                  aria-label={t("dashboard.tabs.close", { name })}
                  title={t("dashboard.tabs.close", { name })}
                  disabled={inFlight(tab)}
                  onClick={() => {
                    home.close(a.id);
                    focusSelected();
                  }}
                >
                  <Icon name="x" size={12} />
                </button>
              </div>
            );
          })}
        </div>
        <button
          type="button"
          className="btn btn-icon flex-none"
          // Every account has its tab, or there are none: the only way on is another account.
          popoverTarget={free.length ? "home-tabs-add" : undefined}
          onClick={free.length ? undefined : onAdd}
          style={{ anchorName: "--home-tabs-add" }}
          aria-label={free.length ? t("dashboard.tabs.add") : t("accounts.add.title")}
          title={free.length ? t("dashboard.tabs.add") : t("accounts.add.title")}
        >
          <Icon name="plus" size={14} />
        </button>
        <div
          id="home-tabs-add"
          popover="auto"
          className="menu scroll"
          onKeyDown={menuKeys}
          onToggle={(e) => {
            if (e.newState === "open") e.currentTarget.querySelector<HTMLElement>("input, .menu-item")?.focus();
            else setWho("");
          }}
          style={{ positionAnchor: "--home-tabs-add", positionArea: "block-end span-inline-end", margin: "6px 0 0" }}
        >
          {free.length > 12 && (
            <input className="field mb-1" aria-label={t("accounts.filterLabel")} placeholder={t("accounts.filterPlaceholder")} spellCheck={false} value={who} onChange={(e) => setWho(e.target.value)} />
          )}
          {picks.map((a) => (
            <button
              key={a.id}
              className="menu-item"
              onClick={(e) => {
                // Now: the item is gone once the tab is added, and its own hide would never run.
                e.currentTarget.closest<HTMLElement>("[popover]")?.hidePopover();
                home.add(a.id);
                focusSelected();
              }}
            >
              <Avatar account={a} size={24} />
              <span className="truncate">{accountName(a)}</span>
            </button>
          ))}
          <div role="separator" className="h-px my-1 bg-line" />
          <button
            className="menu-item"
            onClick={(e) => {
              e.currentTarget.closest<HTMLElement>("[popover]")?.hidePopover();
              onAdd();
            }}
          >
            <Icon name="plus" size={12} />
            {t("accounts.add.title")}
          </button>
        </div>
        {ready.length > 0 && (
          <button className="btn btn-primary flex-none ms-auto" disabled={launching || allBlock !== null} title={allBlock ?? undefined} onClick={() => void launch(ready)}>
            <Icon name="play" size={12} fill />
            {launching ? t("launch.launching") : t("launch.launchMany", { count: ready.length })}
          </button>
        )}
      </div>
      {(open || ready.length > 1 || allReplace) && <MultiNotice wanted={ready.length > 1} replaces={allReplace || (!!open && replaces(open))} />}
      {open && otherBlock && <p className="alert">{otherBlock}</p>}
      <div role="tabpanel" id="home-tab-panel" aria-labelledby={tabId(active)} className="flex flex-col gap-6 min-w-0">
        {open && openAccount ? (
          <TabBody
            key={open.accountId}
            tab={open}
            account={openAccount}
            accounts={accounts}
            instances={instances}
            block={blockFor(false, replaces(open)) ?? (launching ? t("launch.launching") : null)}
            onDraft={(f) => home.setDraft(open.accountId, f)}
            onLaunch={() => void launch([open])}
          />
        ) : (
          children
        )}
      </div>
    </>
  );
}
