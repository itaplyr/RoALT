import { useEffect, useLayoutEffect, useReducer, useRef, useState } from "react";
import type { Group } from "../../api";
import { groupColor } from "../../data";
import { fmtNum, t, type Key } from "../../i18n";
import { Button, IconButton } from "../../ui/Button";
import { ListPresence, Num, Swap } from "../../ui/motion";
import { PillLabel, PillSeparator, PillToolbar } from "../../ui/PillToolbar";
import { MenuItem, MenuLabel, MenuSeparator, Popover, useAnchor } from "../../ui/Popover";

/** Same as `BEFRIEND_MAX` in src-tauri/src/commands.rs, which refuses more. */
const BEFRIEND_MAX = 20;
const GROUP_MENU = "acc-group-menu";
const MORE_MENU = "acc-more-menu";

/** A counted phrase ("12 selected") whose number tweens to each new count. */
export function Counted({ k, n }: { k: Key; n: number }) {
  const text = t(k, { count: n });
  const num = fmtNum(n);
  const at = text.indexOf(num);
  if (at < 0) return <>{text}</>;
  return (
    <>
      {text.slice(0, at)}
      <Num value={n} format={(v) => fmtNum(Math.round(v))} />
      {text.slice(at + num.length)}
    </>
  );
}

/** The header's one pill. At rest: check, export, add. With rows ticked it morphs into what applies to
 * them; reasons for what won't work show in the line under it, never in a tooltip alone. */
export function AccountsToolbar({ total, selected, groups, playable, friendable, mainSkipped, checking, befriending, onCheckAll, onCheck, onExport, onAdd, onPlay, onBefriend, onAddToGroup, onEdit, onRemove, onClear }: {
  total: number;
  /** Ticked account ids. */
  selected: number[];
  groups: Group[];
  /** How many of the ticked can launch now, and how many are signed in (Make friends). */
  playable: number;
  friendable: number;
  /** Select-all left the MAIN account out. */
  mainSkipped: boolean;
  /** A sign-in check runs (one at a time: the others wait for it). */
  checking: boolean;
  befriending: boolean;
  onCheckAll: () => void;
  onCheck: () => void;
  onExport: (ids: number[] | null) => void;
  onAdd: () => void;
  onPlay: () => void;
  onBefriend: () => void;
  onAddToGroup: (g: Group) => void;
  /** The ticked accounts' overrides, all at once. */
  onEdit: () => void;
  onRemove: () => void;
  onClear: () => void;
}) {
  const n = selected.length;
  const groupMenu = useAnchor();
  const moreMenu = useAnchor();
  const root = useRef<HTMLDivElement>(null);
  const set = useRef<HTMLSpanElement>(null);
  // The pill changed sets under the keyboard: the focus moves to the new set's first control (the
  // old set is still there, leaving, so it is found by its own marker).
  useEffect(() => {
    const el = root.current;
    if (el?.contains(document.activeElement)) el.querySelector<HTMLElement>(n > 0 ? "[data-sel]" : "[data-rest]")?.focus();
  }, [n > 0]); // eslint-disable-line react-hooks/exhaustive-deps

  // Make friends, the groups and Export go into "…" while the whole selection is wider than the
  // header (German needs far more than English); folded, they come back once that width fits.
  const [fold, setFold] = useState(false);
  const unfolded = useRef(0);
  const [, resized] = useReducer((x: number) => x + 1, 0);
  useLayoutEffect(() => {
    const ro = new ResizeObserver(() => resized());
    ro.observe(root.current!.parentElement!);
    return () => ro.disconnect();
  }, []);
  useLayoutEffect(() => {
    const header = root.current?.parentElement;
    const pill = set.current?.closest<HTMLElement>(".pill-toolbar");
    if (!header || !pill || !set.current || (!fold && n === 0)) return;
    if (fold) {
      if (header.clientWidth >= unfolded.current) setFold(false);
      return;
    }
    // The set's own width, whatever the pill's spring shows of it now, plus the pill's padding.
    const need = set.current.offsetWidth + pill.offsetWidth - (pill.firstElementChild as HTMLElement).offsetWidth;
    if (need > header.clientWidth) {
      unfolded.current = need;
      setFold(true);
    }
  });
  const canBefriend = friendable >= 2 && friendable <= BEFRIEND_MAX;
  const befriend = () => !befriending && canBefriend && onBefriend();

  const reasons = [
    n > 0 && playable < n && { id: "launch", text: t("accounts.cantLaunch", { count: n - playable }) },
    n > 0 && mainSkipped && { id: "main", text: t("accounts.mainSkipped") },
    n > 0 && !canBefriend && { id: "befriend", text: t("accounts.befriendRange", { max: BEFRIEND_MAX }) },
  ].filter((r): r is { id: string; text: string } => !!r);

  const groupItems =
    groups.length === 0 ? (
      // An item, not bare text: a menu holds items only, and the keyboard lands on it and reads it.
      <div role="menuitem" aria-disabled="true" tabIndex={-1} className="menu-item max-w-64 whitespace-normal text-[13px] text-fg-3">
        {t("accounts.noGroupsMenu")}
      </div>
    ) : (
      groups.map((g) => (
        <MenuItem key={g.id} onSelect={() => onAddToGroup(g)}>
          <span className="inline-flex items-center gap-2.5 min-w-0">
            <span aria-hidden="true" className="w-2 h-2 flex-none rounded-full" style={{ background: groupColor(g) }} />
            <span className="truncate">{g.name}</span>
          </span>
        </MenuItem>
      ))
    );

  const resting = (
    <>
      {total > 0 && (
        <>
          <Button variant="ghost" size="md" icon="refresh" busy={checking} data-rest data-tip={t("accounts.validateAll")} data-tip-kbd="Ctrl+R" onClick={onCheckAll}>
            {checking ? t("accounts.validating") : t("accounts.validateAll")}
          </Button>
          <Button variant="ghost" size="md" icon="download" onClick={() => onExport(null)}>
            {t("accounts.exportCookiesEllipsis")}
          </Button>
        </>
      )}
      <Button variant="primary" size="md" icon="plus" data-rest={total === 0 || undefined} data-tip={t("accounts.add.title")} data-tip-kbd="Ctrl+N" onClick={onAdd}>
        {t("accounts.add.title")}
      </Button>
    </>
  );

  const selection = (
    <>
      <PillLabel>
        <Counted k="accounts.selected" n={n} />
      </PillLabel>
      <PillSeparator />
      <Button
        variant="primary"
        size="md"
        icon="play"
        data-sel
        aria-disabled={playable === 0 || undefined}
        aria-describedby={playable < n ? "acc-why-launch" : undefined}
        onClick={() => playable > 0 && onPlay()}
      >
        {t("common.playEllipsis")}
      </Button>
      <Button variant="ghost" size="md" icon="refresh" busy={checking} onClick={onCheck}>
        {checking ? t("accounts.validating") : t("accounts.menu.validate")}
      </Button>
      {fold ? (
        <IconButton icon="dots" label={t("home.more")} popoverTarget={MORE_MENU} style={moreMenu.style} />
      ) : (
        <>
          <Button
            variant="ghost"
            size="md"
            icon="accounts"
            busy={befriending}
            aria-disabled={!canBefriend || undefined}
            aria-describedby={!canBefriend ? "acc-why-befriend" : undefined}
            onClick={befriend}
          >
            {befriending ? t("accounts.befriending") : t("accounts.befriendEllipsis")}
          </Button>
          <Button variant="ghost" size="md" iconEnd="chevron" popoverTarget={GROUP_MENU} style={groupMenu.style}>
            {t("accounts.addToGroup")}
          </Button>
          <Button variant="ghost" size="md" icon="pencil" aria-haspopup="dialog" onClick={onEdit}>
            {t("accounts.bulk.edit")}
          </Button>
          <Button variant="ghost" size="md" icon="download" onClick={() => onExport(selected)}>
            {t("accounts.exportCookiesEllipsis")}
          </Button>
        </>
      )}
      <PillSeparator />
      <Button variant="ghost" size="md" icon="trash" className="acc-danger" onClick={onRemove}>
        {t("common.removeEllipsis")}
      </Button>
      <IconButton icon="x" label={t("accounts.clear")} shortcut="Esc" onClick={onClear} />
    </>
  );

  return (
    <div ref={root} className="ms-auto flex flex-col items-end gap-1.5 min-w-0 max-w-full">
      <PillToolbar label={t("accounts.menu.actions")}>
        {/* Positioned: the leaving set is lifted out over the arriving one while the pill springs. */}
        <span ref={set} className="relative inline-flex items-center gap-0.5">
          <Swap k={n > 0 ? "selection" : "rest"} variant="rise">
            {n > 0 ? selection : resting}
          </Swap>
        </span>
      </PillToolbar>
      <div aria-live="polite">
        <ListPresence
          items={reasons}
          getKey={(r) => r.id}
          className="flex flex-col items-end"
          render={(r) => (
            <p id={`acc-why-${r.id}`} className="text-[13px] text-fg-2 text-end">
              {r.text}
            </p>
          )}
        />
      </div>
      <Popover id={GROUP_MENU} anchor={groupMenu.name} placement="below-end" role="menu" label={t("accounts.addToGroup")}>
        {groupItems}
      </Popover>
      <Popover id={MORE_MENU} anchor={moreMenu.name} placement="below-end" role="menu" label={t("home.more")}>
        <MenuItem icon="accounts" disabled={!canBefriend} onSelect={befriend}>{t("accounts.befriendEllipsis")}</MenuItem>
        <MenuItem icon="download" onSelect={() => onExport(selected)}>{t("accounts.exportCookiesEllipsis")}</MenuItem>
        <MenuItem icon="pencil" onSelect={onEdit}>{t("accounts.bulk.edit")}</MenuItem>
        <MenuSeparator />
        <MenuLabel>{t("accounts.addToGroup")}</MenuLabel>
        {groupItems}
      </Popover>
    </div>
  );
}
