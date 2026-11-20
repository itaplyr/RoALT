import "./mods.css";
import { useRef, useState, type Ref } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openFile, save as saveFile } from "@tauri-apps/plugin-dialog";
import { api } from "../../api";
import type { ModItem } from "../../bindings/ModItem";
import type { ModsList } from "../../bindings/ModsList";
import { qk, useSetSettings, useSettings } from "../../data";
import { fmtSize, t } from "../../i18n";
import { Disclosure, Skeleton } from "../../ui/Bits";
import { Button, IconButton } from "../../ui/Button";
import { Switch } from "../../ui/controls";
import { ConfirmDialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { MenuItem, MenuSeparator, Popover, useAnchor } from "../../ui/Popover";
import { StatusPill } from "../../ui/Status";
import { LoadFailed, toast, toastError } from "../../ui/Toast";
import { reordered } from "../dashboard/homeLayout";
import { useGridDrag } from "../dashboard/useGridDrag";
import { useJumpTarget } from "../settings/jump";

const zipFilter = () => [{ name: t("mods.zipFilter"), extensions: ["zip"] }];

/** Mods: packs of textures, sounds and fonts laid over RoALT's own copy of Roblox (never Roblox's
 * folder), for the clients it starts next. Top of the list wins a file two mods change. */
export function ModsView() {
  const qc = useQueryClient();
  const settings = useSettings().data;
  const setSettings = useSetSettings();
  // Asked again each time the view opens: a launch since may have applied or skipped them.
  const list = useQuery({ queryKey: qk.mods, queryFn: api.modsList, staleTime: 0 });
  const [ack, setAck] = useState(false);
  const [importing, setImporting] = useState(false);
  const master = useRef<HTMLDivElement>(null);
  useJumpTarget("mods.on", master);

  const refresh = () => qc.invalidateQueries({ queryKey: qk.mods });
  const pickAndImport = async () => {
    const path = await openFile({ multiple: false, directory: false, filters: zipFilter() });
    if (!path) return;
    setImporting(true);
    try {
      const got = await api.modsImport(path);
      const added = t("mods.imported", { name: got.name, count: got.files });
      toast(got.skipped ? `${added} ${t("mods.skipped", { count: got.skipped })}` : added);
      await refresh();
    } catch (e) {
      toastError(e);
    } finally {
      setImporting(false);
    }
  };
  const importZip = () => {
    if (importing) return;
    if (!settings?.modsAck) return setAck(true);
    void pickAndImport();
  };

  const on = settings?.modsOn ?? true;
  const mods = list.data?.mods ?? [];
  return (
    <main className="scroll flex-1 min-w-0 page">
      <div className="mods-page">
        <header className="mods-head">
          <div className="mods-title">
            <h1 className="page-title">{t("app.nav.mods")}</h1>
            <Button variant="primary" icon="download" busy={importing} onClick={importZip}>
              {t(importing ? "mods.importing" : "mods.import")}
            </Button>
          </div>
          <Disclosure className="mods-lead" summary={t("mods.lead")}>
            {t("mods.intro")}
          </Disclosure>
        </header>

        <div ref={master} className="set-row mods-master">
          <div className="set-line">
            <div className="set-text">
              <div className="set-title">
                <span className="set-label">{t("mods.useMods")}</span>
              </div>
              <div className="set-detail">
                <State on={on} list={list.data} />
              </div>
            </div>
            <div className="set-control">
              <Switch checked={on} label={t("mods.useMods")} onChange={(v) => setSettings.mutate({ modsOn: v }, { onSuccess: () => void refresh() })} />
            </div>
          </div>
        </div>

        {list.isError ? (
          <LoadFailed error={list.error} busy={list.isFetching} onRetry={() => void list.refetch()} />
        ) : !list.data ? (
          <div className="mods-list">
            <Skeleton height={56} />
            <Skeleton height={56} />
          </div>
        ) : mods.length === 0 ? (
          <div className="mods-empty">
            <Icon name="palette" size={28} />
            <p className="mods-empty-title">{t("mods.emptyTitle")}</p>
            <p>{t("mods.emptyText")}</p>
            <p>{t("mods.emptyBloxstrap")}</p>
          </div>
        ) : (
          <ModList mods={mods} dimmed={!on} />
        )}
      </div>

      <ConfirmDialog
        open={ack}
        safe
        onClose={() => setAck(false)}
        onConfirm={() => {
          setAck(false);
          setSettings.mutate({ modsAck: true });
          void pickAndImport();
        }}
        title={t("mods.ackTitle")}
        confirmLabel={t("mods.ackOk")}
        text={
          <ul className="mods-ack">
            <li>{t("mods.ack1")}</li>
            <li>{t("mods.ack2")}</li>
            <li>{t("mods.ack3")}</li>
          </ul>
        }
      />
    </main>
  );
}

/** Off, nothing on, or how the last launch with mods on went. */
function State({ on, list }: { on: boolean; list: ModsList | undefined }) {
  const anyOn = !!list?.mods.some((m) => m.on);
  const last = list?.last;
  const pill = !on ? (
    <StatusPill tone="neutral" label={t("common.off")} />
  ) : !list ? null : !anyOn ? (
    <StatusPill tone="neutral" label={t("mods.noneOn")} />
  ) : !last ? (
    <StatusPill tone="info" label={t("mods.nextLaunch")} />
  ) : last.applied ? (
    <StatusPill tone="ok" label={t("mods.applied")} detail={last.note} />
  ) : (
    <StatusPill tone="warn" label={t("mods.notApplied")} detail={last.note} />
  );
  return pill;
}

/** The mods, top first; the grip drags one (or the arrows move it). */
function ModList({ mods, dimmed }: { mods: ModItem[]; dimmed: boolean }) {
  const qc = useQueryClient();
  const ids = mods.map((m) => m.id);
  const setList = (next: ModItem[]) => qc.setQueryData<ModsList>(qk.mods, (l) => (l ? { ...l, mods: next } : l));
  const failed = (e: unknown) => {
    toastError(e);
    void qc.invalidateQueries({ queryKey: qk.mods });
  };
  const drag = useGridDrag((from, over) => {
    const next = reordered(ids, ids, from, over);
    setList(next.map((id) => mods.find((m) => m.id === id)!));
    api.modsReorder(next).catch(failed);
  }, true);

  return (
    <>
      <span className="sr-only" aria-live="polite">{drag.moved}</span>
      <div ref={drag.grid as Ref<HTMLDivElement>} className="mods-list sort-col" data-dimmed={dimmed || undefined} aria-label={t("mods.listLabel")} role="list">
        {mods.map((m) => (
          <ModRow
            key={m.id}
            mod={m}
            grip={drag.grip(m.name)}
            onToggle={(v) => {
              setList(mods.map((x) => (x.id === m.id ? { ...x, on: v } : x)));
              api.modsSetOn(m.id, v).catch(failed);
            }}
          />
        ))}
      </div>
      <p className="mods-order">{t("mods.order")}</p>
    </>
  );
}

function ModRow({ mod, grip, onToggle }: { mod: ModItem; grip: ReturnType<ReturnType<typeof useGridDrag>["grip"]>; onToggle: (on: boolean) => void }) {
  const qc = useQueryClient();
  const more = useAnchor();
  const menuId = `mod-${mod.id}`;
  const [removing, setRemoving] = useState(false);
  const [busy, setBusy] = useState(false);
  const by = [mod.author && t("mods.by", { author: mod.author }), mod.version].filter(Boolean).join(" · ");

  const exportZip = async () => {
    try {
      const path = await saveFile({ defaultPath: await api.modsExportName(mod.id), filters: zipFilter() });
      if (!path) return;
      await api.modsExport(mod.id, path);
      toast(t("mods.exported", { name: mod.name }));
    } catch (e) {
      toastError(e);
    }
  };
  const remove = () => {
    setBusy(true);
    api
      .modsRemove(mod.id)
      .then(() => toast(t("mods.removed", { name: mod.name })), toastError)
      .finally(() => {
        setBusy(false);
        setRemoving(false);
        void qc.invalidateQueries({ queryKey: qk.mods });
      });
  };

  return (
    <div className="mods-row" role="listitem">
      <button type="button" className="mods-grip" aria-label={t("mods.move", { name: mod.name })} {...grip}>
        <Icon name="grip" />
      </button>
      <div className="mods-text" title={mod.description || undefined}>
        <span className="mods-name">{mod.name}</span>
        <span className="mods-meta">
          {[by, t("mods.files", { count: mod.files }), fmtSize(mod.bytes)].filter(Boolean).join(" · ")}
        </span>
      </div>
      <Switch checked={mod.on} label={mod.name} onChange={onToggle} />
      <IconButton icon="dots" label={t("home.more")} popoverTarget={menuId} style={more.style} />
      <Popover id={menuId} anchor={more.name} placement="below-end" role="menu" label={t("home.more")}>
        <MenuItem icon="upload" onSelect={() => void exportZip()}>{t("mods.export")}</MenuItem>
        <MenuSeparator />
        <MenuItem icon="trash" danger onSelect={() => setRemoving(true)}>{t("mods.remove")}</MenuItem>
      </Popover>
      <ConfirmDialog
        open={removing}
        busy={busy}
        onClose={() => setRemoving(false)}
        onConfirm={remove}
        title={t("mods.removeTitle", { name: mod.name })}
        text={t("mods.removeText")}
        confirmLabel={t("mods.remove")}
      />
    </div>
  );
}
