import { useEffect, useRef, useState, type ReactNode } from "react";
import type { AccountSummary, Instance } from "../../api";
import { isRtl, t } from "../../i18n";
import { Icon } from "../../ui/Icon";
import { accountName, Avatar } from "../accounts/shared";
import { isLive, STATES } from "../launch/Running";
import type { HomeTabs } from "./homeLayout";
import { Sortable } from "./Sortable";

/** The "accounts on the side" layout's left part: Overview and every account, each opening on Home.
 * An account's row opens its tab there, as the strip's + does. `onHome`: Home is on screen, the only
 * place a row is the current one. */
export function AccountRail({ width, accounts, instances, home, onHome, onOpen, onAdd, footer }: {
  width: number;
  accounts: AccountSummary[];
  instances: Instance[];
  home: HomeTabs;
  onHome: boolean;
  onOpen: () => void;
  onAdd: () => void;
  footer: ReactNode;
}) {
  const [who, setWho] = useState("");
  const needle = who.trim().toLowerCase();
  const shown = needle ? accounts.filter((a) => `${a.alias}\n${a.displayName}\n${a.username}`.toLowerCase().includes(needle)) : accounts;
  const running = instances.filter(isLive).length;
  const byId = new Map(accounts.map((a) => [a.id, a]));
  return (
    <nav aria-label={t("dashboard.tabs.label")} style={{ width }} className="flex-none flex flex-col min-h-0">
      <div className="flex-none px-3 pt-2.5 pb-2">
        <input className="field" type="search" aria-label={t("accounts.filterLabel")} placeholder={t("accounts.filterPlaceholder")} spellCheck={false} value={who} onChange={(e) => setWho(e.target.value)} />
      </div>
      <div className="scroll flex-1 min-h-0 flex flex-col gap-1 px-3 pb-3" style={{ scrollbarWidth: "none" }}>
        {!needle && (
          <button className="nav-item" aria-current={onHome && home.active === null ? "page" : undefined} onClick={() => { home.setActive(null); onOpen(); }}>
            <Icon name="home" />
            <span className="flex-1 min-w-0 truncate">{t("dashboard.tabs.overview")}</span>
            {running > 0 && <span className="nav-count mono" title={t("app.running", { count: running })}>{running}</span>}
          </button>
        )}
        <Sortable
          column
          className="flex flex-col gap-1"
          ids={shown.map((a) => a.id)}
          order={home.order}
          onOrder={home.setOrder}
          name={(id) => accountName(byId.get(id)!)}
          render={(id, drag) => {
          const a = byId.get(id)!;
          const tab = home.tabs.find((x) => x.accountId === a.id);
          const inst = instances.find((i) => i.accountId === a.id && isLive(i));
          const busy = tab?.status === "queued" || tab?.status === "launching";
          const mark = busy ? "var(--dim)" : inst ? STATES[inst.state][1] : tab?.status === "failed" ? "var(--orange)" : null;
          const name = accountName(a);
          return (
            <button key={a.id} className="nav-item" style={{ minHeight: 52 }} title={name} aria-current={onHome && home.active === a.id ? "page" : undefined} onClick={() => { home.add(a.id); onOpen(); }} {...drag}>
              <Avatar account={a} size={30} />
              <span className="flex-1 min-w-0 flex flex-col">
                <span className="truncate">{name}</span>
                <span className="truncate text-xs font-normal text-dim">{inst ? STATES[inst.state][0] : `@${a.username}`}</span>
              </span>
              {mark && <span aria-hidden="true" className={`led flex-none ${busy ? "pulse" : ""}`} style={{ color: mark }} />}
            </button>
          );
          }}
        />
        <button className="nav-item" onClick={onAdd}>
          <Icon name="plus" />
          <span className="flex-1 min-w-0 truncate">{t("accounts.add.title")}</span>
        </button>
      </div>
      {footer}
    </nav>
  );
}

/** That layout's sections, one row along the top. The wheel runs along it, as on Home's tab strip. */
export function SectionTabs({ children }: { children: ReactNode }) {
  const strip = useRef<HTMLElement>(null);
  useEffect(() => {
    const el = strip.current;
    if (!el) return;
    const wheel = (e: WheelEvent) => {
      if (Math.abs(e.deltaX) > Math.abs(e.deltaY) || el.scrollWidth <= el.clientWidth) return;
      e.preventDefault();
      el.scrollLeft += isRtl() ? -e.deltaY : e.deltaY;
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => el.removeEventListener("wheel", wheel);
  }, []);
  // A section picked by keyboard or shortcut stays in sight.
  useEffect(() => {
    strip.current?.querySelector("[aria-current=page]")?.scrollIntoView({ block: "nearest", inline: "nearest" });
  });
  // An edge fades while there is more past it. On the element: scrolling needn't render anything.
  useEffect(() => {
    const el = strip.current;
    if (!el) return;
    const show = () => {
      const end = el.scrollWidth - el.clientWidth;
      // Right to left, scrollLeft runs from 0 down to -end.
      const [fromStart, fromEnd] = [Math.abs(el.scrollLeft), end - Math.abs(el.scrollLeft)];
      const [left, right] = isRtl() ? [fromEnd, fromStart] : [fromStart, fromEnd];
      el.style.setProperty("--fade-left", left > 1 ? "28px" : "0px");
      el.style.setProperty("--fade-right", right > 1 ? "28px" : "0px");
    };
    show();
    el.addEventListener("scroll", show, { passive: true });
    const size = new ResizeObserver(show);
    size.observe(el);
    return () => {
      el.removeEventListener("scroll", show);
      size.disconnect();
    };
  }, []);
  return (
    <nav ref={strip} aria-label={t("app.sections")} className="section-tabs">
      {children}
    </nav>
  );
}
