import "./debug.css";
import { useDeferredValue, useMemo, useRef, useState } from "react";
import { save as saveFile } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api } from "../../api";
import { fmtNum, t } from "../../i18n";
import { Chip } from "../../ui/Bits";
import { Button, IconButton } from "../../ui/Button";
import { Input } from "../../ui/Field";
import { Icon } from "../../ui/Icon";
import { Collapse, Morph, Num, Swap } from "../../ui/motion";
import { MenuItem, MenuSeparator, Popover, useAnchor } from "../../ui/Popover";
import { friendly, toast, toastError } from "../../ui/Toast";
import { LogView } from "./LogView";
import { LEVELS, useLogStream, type Level } from "./useLogStream";

/** Each level's mark: the status shapes (ui/Status), so a level reads without its colour. */
const SHAPE: Record<Level, string> = { error: "diamond", warn: "triangle", info: "dot", debug: "ring" };

/** The filter as a case-blind match with one capture group, which LogView marks. */
const finder = (q: string) => (q ? new RegExp(`(${q.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})`, "i") : null);

/** Everything RoALT logs (roalt.log), live: find the line that went wrong, or copy a report. */
export function DebugView() {
  const [shown, setShown] = useState<Record<Level, boolean>>({ error: true, warn: true, info: true, debug: false });
  const [query, setQuery] = useState("");
  const [paused, setPaused] = useState(false);
  const [copying, setCopying] = useState(false);
  const log = useLogStream(paused);
  const more = useAnchor();
  const findField = useRef<HTMLInputElement>(null);
  // The field answers each key at once; re-marking up to 5000 lines follows behind, interruptibly.
  const deferred = useDeferredValue(query);
  const find = useMemo(() => finder(deferred.trim()), [deferred]);
  const visible = useMemo(() => log.lines.filter((l) => shown[l.level] && (!find || find.test(l.text))), [log.lines, shown, find]);
  const counts = useMemo(() => {
    const c: Record<Level, number> = { error: 0, warn: 0, info: 0, debug: 0 };
    for (const l of log.lines) c[l.level]++;
    return c;
  }, [log.lines]);

  const copyShown = () => writeText(visible.map((l) => l.text).join("\n")).then(() => toast(t("debug.copied")), toastError);
  const copyReport = () => {
    if (copying) return;
    setCopying(true);
    api
      .diagnosticsText()
      .then(writeText)
      .then(() => toast(t("debug.reportCopied")), toastError)
      .finally(() => setCopying(false));
  };
  const saveReport = async () => {
    try {
      const path = await saveFile({ defaultPath: `RoALT-diagnostics-${new Date().toISOString().slice(0, 10)}.txt`, filters: [{ name: t("debug.reportFilter"), extensions: ["txt"] }] });
      if (!path) return;
      await api.diagnosticsSave(path);
      toast(t("debug.reportSaved"));
    } catch (e) {
      toastError(e);
    }
  };

  return (
    <main className="dbg page flex-1 min-w-0 min-h-0 flex flex-col pb-6">
      <h1 className="page-title">{t("debug.title")}</h1>
      <p className="page-sub">{t("debug.intro")}</p>

      <div className="dbg-bar">
        <div className="dbg-levels" role="group" aria-label={t("debug.levels")}>
          {LEVELS.map((lv) => (
            <Chip key={lv} pressed={shown[lv]} onClick={() => setShown((s) => ({ ...s, [lv]: !s[lv] }))}>
              <span aria-hidden="true" className="status-mark dbg-mark" data-shape={SHAPE[lv]} data-level={lv} />
              {t(`debug.level.${lv}`)}
              {/* Chip's own count, but tweened. */}
              <Num value={counts[lv]} format={fmtNum} className="text-fg-3 font-normal tabular-nums" />
            </Chip>
          ))}
        </div>
        <div className="dbg-find">
          <Input
            ref={findField}
            leading="search"
            type="search"
            data-find
            value={query}
            placeholder={t("debug.filter")}
            aria-label={t("debug.filter")}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <Morph className="dbg-act" innerClassName="dbg-actions">
          {/* Paused, Resume is the way back: the one primary action, with what waits behind it. */}
          <Button size="md" variant={paused ? "primary" : "secondary"} onClick={() => setPaused((p) => !p)}>
            <Swap k={paused ? "resume" : "pause"}>
              {paused ? t("debug.resume") : t("debug.pause")}
              {paused && log.held > 0 && (
                <>
                  <span aria-hidden="true" className="dbg-held">
                    <Num value={log.held} format={fmtNum} />
                  </span>
                  <span className="sr-only">{t("debug.newLines", { count: log.held })}</span>
                </>
              )}
            </Swap>
          </Button>
          <Button size="md" icon="copy" busy={copying} aria-description={t("debug.reportHint")} onClick={copyReport}>
            {t("debug.copyReport")}
          </Button>
          <IconButton icon="dots" label={t("home.more")} popoverTarget="debug-more" style={more.style} />
        </Morph>
      </div>
      <Popover id="debug-more" anchor={more.name} placement="below-end" role="menu" label={t("home.more")}>
        <MenuItem icon="copy" disabled={!visible.length} onSelect={copyShown}>{t("debug.copy")}</MenuItem>
        <MenuItem icon="trash" disabled={!log.lines.length && !log.held} onSelect={log.clear}>{t("debug.clear")}</MenuItem>
        <MenuItem icon="external" onSelect={() => api.logsOpen().catch(toastError)}>{t("debug.openFolder")}</MenuItem>
        <MenuItem icon="download" onSelect={saveReport}>{t("debug.saveReport")}</MenuItem>
        <MenuSeparator />
        <div className="dbg-note">{t("debug.reportHint")}</div>
      </Popover>

      <Collapse open={!!log.error} className="pt-3">
        <div className="notice" data-tone="attention" role="alert">
          <Icon name="alert" className="text-attention" />
          <span className="flex-1 min-w-0">{log.error && friendly(log.error)}</span>
          <Button size="md" icon="refresh" onClick={log.retry}>{t("common.tryAgain")}</Button>
        </div>
      </Collapse>

      <LogView lines={visible} find={find} label={t("debug.title")}>
        {log.loading ? (
          <p dir="auto" className="dbg-status dbg-wait">{t("common.loading")}</p>
        ) : visible.length ? null : (
          <div dir="auto" className="dbg-status">
            <p>{t("debug.empty")}</p>
            {find && log.lines.length > 0 && (
              <Button
                size="sm"
                onClick={() => {
                  setQuery("");
                  findField.current?.focus();
                }}
              >
                {t("accounts.clearFilters")}
              </Button>
            )}
          </div>
        )}
      </LogView>
    </main>
  );
}
