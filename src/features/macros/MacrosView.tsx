import "./macros.css";
import { useId, useRef, useState } from "react";
import { open as openFile, save as saveFile } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { api, asError, type AccountSummary, type Macro, type MacroInput, type MacroRun, type Settings } from "../../api";
import { qk, useInstances, useMacroRuns, useMacros } from "../../data";
import { t, tx } from "../../i18n";
import { Disclosure, Kbd, Skeleton } from "../../ui/Bits";
import { Button, IconButton } from "../../ui/Button";
import { ConfirmDialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { Collapse, ListPresence, Morph, SPRING, Swap, TWEEN } from "../../ui/motion";
import { MenuItem, Popover, useAnchor } from "../../ui/Popover";
import { StatusPill } from "../../ui/Status";
import { friendly, toast, toastError } from "../../ui/Toast";
import { Avatar } from "../accounts/shared";
import { uptime, useNow } from "../launch/Running";
import { useRow } from "../settings/kit/rowError";
import { Renderers, SettingRow, type RowRenderers } from "../settings/kit/SettingRow";
import { SettingSection } from "../settings/kit/SettingSection";
import { MACROS_ENTRIES } from "./entries";
import { clientOf, MacroDialog, RunDialog, TrustDialog } from "./MacroDialogs";

const AHK_SITE = "https://www.autohotkey.com/";
// Looked up again on focus: the user may come back from installing it.
const AHK_QUERY = { queryKey: qk.ahk, queryFn: api.ahkStatus, staleTime: 0, refetchOnWindowFocus: true } as const;
const scriptFilter = () => [{ name: t("macros.fileFilter"), extensions: ["ahk", "txt"] }];
/** What New macro starts from; the backend passes the client's window id and pid as A_Args.
 * Roblox ignores keys posted to a window (ControlSend; measured for anti-AFK, afk.rs), so it
 * brings its own client to the front first and sends real key presses. */
const TEMPLATE = `#Requires AutoHotkey v2.0
; A_Args[1]: this client's window id, A_Args[2]: its process id.
Client := "ahk_id " A_Args[1]

Loop {
    WinActivate Client
    if WinWaitActive(Client, , 2)
        Send "{Space}"
    Sleep 60000
}
`;

/** Install (when it is missing) and Browse… for AutoHotkey64.exe. */
function AhkControls({ missing, failed, retrying, retry }: { missing: boolean; failed: boolean; retrying: boolean; retry: () => void }) {
  const row = useRow();
  const browse = async () => {
    const path = await openFile({ multiple: false, directory: false, filters: [{ name: "AutoHotkey", extensions: ["exe"] }] }).catch(row.fail);
    if (path) void row.write({ ahkPath: path });
  };
  return (
    <Morph innerClassName="gap-2">
      <AnimatePresence initial={false} mode="popLayout">
        {missing && (
          <motion.span key="install" className="inline-flex" initial={{ opacity: 0, scale: 0.96 }} animate={{ opacity: 1, scale: 1, transition: SPRING.snappy }} exit={{ opacity: 0, scale: 0.96, transition: TWEEN.exit }}>
            <Button size="md" variant="primary" iconEnd="external" onClick={() => openUrl(AHK_SITE).catch(row.fail)}>
              {t("macros.install")}
            </Button>
          </motion.span>
        )}
      </AnimatePresence>
      {failed && (
        <Button size="md" icon="refresh" busy={retrying} onClick={retry}>
          {t("common.tryAgain")}
        </Button>
      )}
      <Button size="md" onClick={() => void browse()}>
        {t("macros.browse")}
      </Button>
    </Morph>
  );
}

/** Where AutoHotkey v2 is: found (its path), missing (Install), or what went wrong asking. */
function AhkRow() {
  const ahk = useQuery(AHK_QUERY);
  const path = ahk.data?.path;
  const state = ahk.isError ? "error" : !ahk.data ? "loading" : path ? `found:${path}` : "missing";
  return (
    <SettingRow
      id="ahk"
      detail={
        <Morph axis="height">
          <div className="mac-ahk">
            {state === "loading" ? (
              <>
                <Skeleton width={240} height={12} className="my-1" />
                <span className="sr-only">{t("common.loading")}</span>
              </>
            ) : (
              <Swap k={state}>
                {ahk.isError ? (
                  <span className="alert">{friendly(asError(ahk.error))}</span>
                ) : path ? (
                  <span>{tx("macros.ahkFound", { path: <span className="mono selectable">{path}</span> })}</span>
                ) : (
                  <span className="mac-missing">
                    <Icon name="alert" size={14} className="text-warn" />
                    <span>{t("macros.ahkMissing")}</span>
                  </span>
                )}
              </Swap>
            )}
          </div>
        </Morph>
      }
    >
      <AhkControls missing={ahk.data !== undefined && !path} failed={ahk.isError} retrying={ahk.isFetching} retry={() => void ahk.refetch()} />
    </SettingRow>
  );
}

const RENDERERS: RowRenderers = { ahk: AhkRow };

/** One macro: its name, hotkey and marks, then Run (or Review… until it is trusted), Edit and ⋯. */
function MacroItem({ m, ahkMissing, autorun, reasonId, onRun, onReview, onEdit, onExport, onRemove }: {
  m: Macro;
  ahkMissing: boolean;
  autorun: boolean;
  reasonId: string;
  onRun: () => void;
  onReview: () => void;
  onEdit: () => void;
  onExport: () => void;
  onRemove: () => void;
}) {
  const anchor = useAnchor();
  const menu = useId();
  const [open, setOpen] = useState(false);
  const more = t("home.moreFor", { name: m.name });
  // Trusted but nothing to run it with: Run waits, and the line over the list says why. aria-disabled,
  // not disabled: it keeps the focus (Trust it lands here) and stays in the Tab order with its reason.
  const blocked = m.trusted && ahkMissing;
  return (
    <div className="mac-row" data-macro={m.id}>
      <div className="mac-what">
        <span className="mac-name">{m.name}</span>
        {m.hotkey && <Kbd keys={m.hotkey} />}
        {m.runOnLaunch && (
          <span className="tag mac-launch" data-off={!autorun || undefined}>
            {t("macros.onLaunch")}
          </span>
        )}
        {m.runOnLaunch && !autorun && <span className="mac-sub">{t("macros.autorunOff")}</span>}
        <AnimatePresence initial={false}>
          {!m.trusted && (
            <motion.span key="untrusted" className="inline-flex" exit={{ opacity: 0, scale: 0.9, transition: TWEEN.exit }}>
              <StatusPill tone="attention" label={t("macros.untrusted")} />
            </motion.span>
          )}
        </AnimatePresence>
      </div>
      <div className="mac-actions">
        <Morph>
          <Button
            size="md"
            aria-disabled={blocked || undefined}
            aria-describedby={blocked ? reasonId : undefined}
            aria-label={t(m.trusted ? "macros.runTitle" : "macros.reviewTitle", { name: m.name })}
            onClick={blocked ? undefined : m.trusted ? onRun : onReview}
          >
            <Swap k={m.trusted ? "run" : "review"}>
              {m.trusted && <Icon name="play" size={14} />}
              {t(m.trusted ? "macros.run" : "macros.review")}
            </Swap>
          </Button>
        </Morph>
        <Button size="md" variant="ghost" aria-label={t("macros.editTitle", { name: m.name })} onClick={onEdit}>
          {t("macros.edit")}
        </Button>
        <IconButton icon="dots" label={more} aria-haspopup="menu" aria-expanded={open} popoverTarget={menu} style={anchor.style} />
        <Popover
          id={menu}
          anchor={anchor.name}
          placement="below-end"
          role="menu"
          label={more}
          onToggle={(on) => {
            setOpen(on);
            // Opened from the keyboard or not, the arrows start at the first item.
            if (on) document.getElementById(menu)?.querySelector<HTMLElement>("[role=menuitem]")?.focus();
          }}
        >
          <MenuItem icon="download" onSelect={onExport}>
            {t("settings.exportEllipsis")}
          </MenuItem>
          <MenuItem icon="trash" danger onSelect={onRemove}>
            {t("common.removeEllipsis")}
          </MenuItem>
        </Popover>
      </div>
    </div>
  );
}

/** A macro running on one client, and how long it has. */
function RunItem({ run, client, onStop }: { run: MacroRun; client: ReturnType<typeof clientOf>; onStop: () => void }) {
  // Here, not in the view: only the uptime re-renders.
  const now = useNow(30_000);
  return (
    <div className="mac-row" data-run={run.instanceId}>
      {client.account && <Avatar account={client.account} size={28} />}
      <div className="mac-what mac-stack">
        <span className="mac-name">{client.name}</span>
        <span className="mac-sub">
          {run.name} · <span className="num-tab">{uptime(run.startedAt, now)}</span>
        </span>
      </div>
      <Button size="md" icon="stop" aria-label={t("macros.stopOn", { name: client.name })} onClick={onStop}>
        {t("macros.stop")}
      </Button>
    </div>
  );
}

/** The user's AutoHotkey v2 macros: what runs them, the list, and what runs now. */
export function MacrosView({ accounts, settings }: { accounts: AccountSummary[]; settings: Settings }) {
  const qc = useQueryClient();
  const ahk = useQuery(AHK_QUERY).data;
  const macros = useMacros();
  const runs = useMacroRuns().data ?? [];
  const instances = useInstances().data ?? [];
  const [editing, setEditing] = useState<MacroInput | null>(null);
  const [running, setRunning] = useState<Macro | null>(null);
  const [reviewing, setReviewing] = useState<Macro | null>(null);
  const [removing, setRemoving] = useState<Macro | null>(null);
  const list = useRef<HTMLDivElement>(null);
  const runList = useRef<HTMLDivElement>(null);
  const newButton = useRef<HTMLButtonElement>(null);
  const reasonId = useId();
  const listHeading = useId();
  const runHeading = useId();
  const needsAhk = ahk !== undefined && !ahk.path;
  const changed = () => qc.invalidateQueries({ queryKey: qk.macros });

  const importFiles = async () => {
    const paths = await openFile({ multiple: true, directory: false, filters: scriptFilter() }).catch(toastError);
    if (!paths?.length) return;
    // Some may be in before one fails.
    api.macrosImport(paths).then(() => toast(t("macros.imported")), toastError).finally(changed);
  };
  const exportMacro = async (m: Macro) => {
    const path = await saveFile({ defaultPath: `${m.name.replace(/[\\/:*?"<>|]/g, "_")}.ahk`, filters: scriptFilter() }).catch(toastError);
    if (path) api.macroExport(m.id, path).then(() => toast(t("macros.exported", { name: m.name })), toastError);
  };
  const remove = () => {
    const gone = removing;
    if (!gone) return;
    // Closed first: the prompt gives the focus back to the row's ⋯, and only then does it move on.
    setRemoving(null);
    const all = macros.data ?? [];
    const at = all.findIndex((m) => m.id === gone.id);
    const next = all[at + 1] ?? all[at - 1];
    api
      .macroRemove(gone.id)
      // On to the next row (its first live button), or to New macro once none are left.
      .then(() => changed().then(() => requestAnimationFrame(() => (next ? list.current?.querySelector<HTMLElement>(`[data-macro="${next.id}"] button:not(:disabled)`) : newButton.current)?.focus())), toastError);
  };
  // A stopped run's row folds away with the focus in it: on to the next run's Stop, or up to the last
  // macro once nothing runs.
  const handOff = (next?: MacroRun) =>
    (next
      ? runList.current?.querySelector<HTMLElement>(`[data-run="${CSS.escape(next.instanceId)}"] button`)
      : ([...(list.current?.querySelectorAll("[data-macro]") ?? [])].at(-1)?.querySelector<HTMLElement>("button:not(:disabled)") ?? newButton.current)
    )?.focus();
  const stop = (r: MacroRun) => {
    const left = runs.filter((x) => x !== r);
    handOff(left[runs.indexOf(r)] ?? left.at(-1));
    api.macroStop(r.instanceId).catch(toastError);
  };
  const stopAll = () => {
    handOff();
    api.macrosStopAll().catch(toastError);
  };

  return (
    <main className="scroll flex-1 min-w-0 page">
      <Renderers value={RENDERERS}>
        <div className="mac-page">
          <header className="mac-top">
            <div>
              <h1 className="page-title">{t("app.nav.macros")}</h1>
              <Disclosure className="mac-lead" summary={t("macros.lead")}>
                {t("macros.intro")}
              </Disclosure>
            </div>
            <Disclosure
              className="mac-warn"
              summary={
                <span className="inline-flex items-center gap-2.5">
                  <Icon name="alert" className="text-warn" />
                  {t("macros.warningShort")}
                </span>
              }
            >
              {t("macros.warning")}
            </Disclosure>
          </header>

          <SettingSection ids={MACROS_ENTRIES.map((e) => e.id)} />

          <section className="set-section" aria-labelledby={listHeading}>
            <div className="mac-head">
              <h2 id={listHeading} className="section-title">
                {t("macros.list")}
              </h2>
              <div className="mac-tools">
                <Button size="md" onClick={() => void importFiles()}>
                  {t("settings.importEllipsis")}
                </Button>
                <Button ref={newButton} size="md" variant={needsAhk ? "secondary" : "primary"} icon="plus" onClick={() => setEditing({ name: "", body: TEMPLATE, hotkey: "", runOnLaunch: false })}>
                  {t("macros.new")}
                </Button>
              </div>
            </div>
            <Collapse open={needsAhk}>
              <p id={reasonId} className="mac-reason">
                <Icon name="alert" size={14} />
                {t("macros.runNeedsAhk")}
              </p>
            </Collapse>
            <div ref={list} className="panel mac-list">
              {macros.isError ? (
                <div className="mac-row">
                  <p className="alert flex-1 min-w-0">{friendly(asError(macros.error))}</p>
                  <Button size="md" icon="refresh" busy={macros.isFetching} onClick={() => void macros.refetch()}>
                    {t("common.tryAgain")}
                  </Button>
                </div>
              ) : !macros.data ? (
                <>
                  <span className="sr-only">{t("common.loading")}</span>
                  {[180, 120].map((w) => (
                    <div key={w} className="mac-row">
                      <Skeleton width={w} height={14} />
                    </div>
                  ))}
                </>
              ) : (
                <>
                  <ListPresence
                    items={macros.data}
                    getKey={(m) => m.id}
                    itemClassName="mac-item"
                    render={(m) => (
                      <MacroItem
                        m={m}
                        ahkMissing={needsAhk}
                        autorun={settings.macroAutorun}
                        reasonId={reasonId}
                        onRun={() => setRunning(m)}
                        onReview={() => setReviewing(m)}
                        onEdit={() => setEditing({ id: m.id, name: m.name, body: m.body, hotkey: m.hotkey, runOnLaunch: m.runOnLaunch })}
                        onExport={() => void exportMacro(m)}
                        onRemove={() => setRemoving(m)}
                      />
                    )}
                  />
                  <Collapse open={macros.data.length === 0}>
                    <p className="mac-empty">{t("macros.none")}</p>
                  </Collapse>
                </>
              )}
            </div>
          </section>

          <Collapse open={runs.length > 0}>
            <section className="mac-running" aria-labelledby={runHeading}>
              <div className="mac-head">
                <h2 id={runHeading} className="section-title">
                  {t("macros.running")}
                </h2>
                <Button size="md" icon="stop" onClick={stopAll}>
                  {t("macros.stopAll")}
                </Button>
              </div>
              <div ref={runList} className="panel mac-list">
                <ListPresence
                  items={runs}
                  getKey={(r) => r.instanceId}
                  itemClassName="mac-item"
                  render={(r) => <RunItem run={r} client={clientOf(instances.find((i) => i.id === r.instanceId), accounts)} onStop={() => stop(r)} />}
                />
              </div>
            </section>
          </Collapse>
        </div>
      </Renderers>

      <MacroDialog input={editing} onClose={() => setEditing(null)} onSaved={changed} />
      <RunDialog macro={running} instances={instances} accounts={accounts} onClose={() => setRunning(null)} />
      <TrustDialog macro={reviewing} onClose={() => setReviewing(null)} onTrusted={changed} />
      <ConfirmDialog
        open={removing !== null}
        onClose={() => setRemoving(null)}
        onConfirm={remove}
        title={t("macros.removeTitle", { name: removing?.name ?? "" })}
        text={t("macros.removeText")}
        confirmLabel={t("common.remove")}
      />
    </main>
  );
}
