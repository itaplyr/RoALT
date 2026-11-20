import "../settings.css";
import { createContext, useContext, useId, useRef, useState, type ComponentType, type ReactNode } from "react";
import { motion } from "motion/react";
import { useQueryClient } from "@tanstack/react-query";
import { qk, useBackgroundUrl, useSetSettings, useSettings, useVault } from "../../../data";
import { t } from "../../../i18n";
import { Icon } from "../../../ui/Icon";
import { Collapse, SPRING } from "../../../ui/motion";
import { useJumpTarget } from "../jump";
import { childrenOf, dyn, entry, paragraphs, shown, type Ctx } from "../registry";
import { AutoControl } from "./AutoControl";
import { errorText, RowContext, type RowApi } from "./rowError";

/** A view's own drawing of its custom and action rows, by entry id. Each draws its whole row:
 * `breaker: () => <SettingRow id="breaker"><BreakerFields /></SettingRow>`. */
export type RowRenderers = Readonly<Record<string, ComponentType>>;
export const Renderers = createContext<RowRenderers>({});

/** Inside search results: the rows whose match is in their "More about" text, which then opens. */
export const SearchHits = createContext<ReadonlySet<string> | null>(null);

/** What row conditions read, as it is now. */
export function useCtx(): Ctx {
  // Views mount only once the settings and the vault are loaded (App).
  const s = useSettings().data!;
  const vault = useVault().data!;
  const background = !!useBackgroundUrl();
  return { s, vault, background };
}

/** An entry's row: its view's renderer when it has one, else the row its entry declares. */
export function EntryRow({ id }: { id: string }) {
  const Draw = useContext(Renderers)[id];
  return Draw ? <Draw /> : <SettingRow id={id} />;
}

/** One setting's row, drawn from its registry entry: the label and one-line hint, a "More about"
 * disclosure, the control, the rows that depend on it, and its result and error lines. It is the
 * entry's jump target (`set-{id}`). */
export function SettingRow({ id, name, children, title, status, detail, more, below }: {
  id: string;
  /** What the "More about" button calls the row, when its title isn't its label ("RoALT 0.9.2"). */
  name?: string;
  /** The control, in place of the one the entry declares. */
  children?: ReactNode;
  /** A title of its own in place of the label, which stays its search name (and, without `name`,
   * its "More about" one). */
  title?: ReactNode;
  /** Beside the title: a StatusPill. */
  status?: ReactNode;
  /** Under the hint: a live line (a path, a version). */
  detail?: ReactNode;
  /** The disclosure's body, in place of the entry's paragraphs, when it depends on the row's state. */
  more?: ReactNode;
  /** Under the row, full width: a notice. */
  below?: ReactNode;
}) {
  const e = entry(id);
  const ctx = useCtx();
  const qc = useQueryClient();
  const set = useSetSettings({ quiet: true });
  const ref = useRef<HTMLDivElement>(null);
  const uid = useId();
  const hits = useContext(SearchHits);
  const found = !!hits?.has(id);
  const [open, setOpen] = useState(found);
  // Typing on until the words are in this row's disclosure opens it; the user may close it again.
  const [wasFound, setWasFound] = useState(found);
  if (found !== wasFound) {
    setWasFound(found);
    if (found) setOpen(true);
  }
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<string | null>(null);
  useJumpTarget(id, ref, !hits);

  const label = t(e.label);
  const about = t("settings.moreAbout", { label: name ?? label });
  const hint = e.hint && dyn(e.hint, ctx);
  const body = more ?? paragraphs(e, ctx).map((p) => <p key={p}>{p}</p>);
  const hasMore = more !== undefined || !!e.more;
  const ids = { hint: `${uid}-hint`, more: `${uid}-more`, error: `${uid}-error` };
  const kids = childrenOf(id);
  const subs = kids.filter((k) => shown(k, ctx));
  const toggle = () => setOpen((o) => !o);

  const row: RowApi = {
    entry: e,
    ctx,
    describedBy: [hint && ids.hint, error && ids.error].filter(Boolean).join(" ") || undefined,
    write: (patch) => {
      setError(null);
      return set.mutateAsync(patch).then(
        (s) => {
          for (const k of e.invalidates ?? []) qc.invalidateQueries({ queryKey: qk[k] });
          return s;
        },
        (err) => void setError(errorText(err)),
      );
    },
    fail: (err) => setError(errorText(err)),
    setError,
    setResult,
    refocus: (selector = "button") =>
      requestAnimationFrame(() => {
        const el = ref.current;
        const at = document.activeElement;
        if (el && (!at || at === document.body || at.matches("main") || el.contains(at))) el.querySelector<HTMLElement>(`.set-control ${selector}`)?.focus();
      }),
  };

  const moreButton = hasMore && (
    <button
      type="button"
      className="set-more-btn"
      aria-expanded={open}
      aria-controls={open ? ids.more : undefined}
      aria-label={about}
      data-tip={about}
      onClick={toggle}
    >
      <motion.span className="grid" initial={false} animate={{ rotate: open ? 180 : 0 }} transition={SPRING.snappy}>
        <Icon name="chevron" size={14} />
      </motion.span>
    </button>
  );

  return (
    <RowContext value={row}>
      {/* A named group: a bare "Open" or "Remove" inside is heard with its setting's name. */}
      <div ref={ref} id={`set-${id}`} data-setting className="set-row" role="group" aria-label={name ?? label}>
        <div className="set-line">
          <div className="set-text">
            <div className="set-title">
              {title ?? <span className="set-label">{label}</span>}
              {status}
              {!hint && moreButton}
            </div>
            {hint && (
              <p className="set-hint">
                {/* The line opens its own disclosure too; the button is the keyboard's way. The id is
                    the line's alone: a control described by it doesn't read out the button. */}
                <span id={ids.hint} data-more={hasMore || undefined} onClick={hasMore ? toggle : undefined}>{hint}</span>
                {moreButton}
              </p>
            )}
            {detail && <div className="set-detail">{detail}</div>}
          </div>
          <div className="set-control">{children ?? <AutoControl />}</div>
        </div>
        {hasMore && (
          <Collapse open={open}>
            <div id={ids.more} className="set-more">{body}</div>
          </Collapse>
        )}
        {kids.length > 0 && (
          <Collapse open={subs.length > 0}>
            <div className="set-subs-pad">
              <div className="set-subs">
                {subs.map((k) => (
                  <EntryRow key={k.id} id={k.id} />
                ))}
              </div>
            </div>
          </Collapse>
        )}
        {below}
        <div role="status">
          <Collapse open={!!result}>
            <p className="set-result">{result}</p>
          </Collapse>
        </div>
        <Collapse open={!!error}>
          <p id={ids.error} role="alert" className="alert set-error">{error}</p>
        </Collapse>
      </div>
    </RowContext>
  );
}
