import { useEffect, useRef, type CSSProperties, type Dispatch, type ReactNode, type SetStateAction } from "react";
import { api } from "../../api";
import { t } from "../../i18n";
import { Input } from "../../ui/Field";
import { Collapse, Swap } from "../../ui/motion";
import { readDraft, targetText, type Draft, type Tone } from "./target";
import "./launch.css";

/** A parse's tone as a shape and a status hue (never the accent). */
export const MARK: Record<Tone, CSSProperties> = {
  ok: { background: "var(--ok)", borderRadius: "50%" },
  info: { background: "var(--info)", borderRadius: 2 },
  bad: { background: "var(--attention)", transform: "rotate(45deg) scale(.85)" },
};

/** Where to play: a game link, private server, place ID or @username, read by the backend 150 ms after
 * typing stops. Below it, what it read (the shape morphs with the tone), the draft's note and
 * `action` (Play's Save / Edit favourite); a bare job ID asks for its place, and a paste goes straight
 * there. `id` names the field (Play's launch-target, Home's home-tab-target); its place field is
 * `${id}-place`. A private server's masked draft (draftFor) is selected whole on focus: typing
 * replaces it. */
export function TargetField({ draft, setDraft, id = "launch-target", autoFocus, disabled, action }: {
  draft: Draft;
  setDraft: Dispatch<SetStateAction<Draft>>;
  id?: string;
  /** Focus it on mount when empty (straight to pasting a link). */
  autoFocus?: boolean;
  disabled?: boolean;
  action?: ReactNode;
}) {
  const field = useRef<HTMLInputElement>(null);
  const pasted = useRef(false);
  const { text } = draft;
  const { parsed, place, look: now } = readDraft(draft);
  // Cleared, the row folds away still saying what it read.
  const kept = useRef(now);
  if (now) kept.current = now;
  const look = now ?? (text.trim() ? null : kept.current);
  const known = draft.parsed?.text === text && draft.parsed.value.kind === "target" ? draft.parsed.value.target : null;
  // Only a private server's description stands in for its link; any other text is what was typed.
  const masked = known?.kind === "privateAccess" && targetText(known) !== text;

  // The field can be below the fold: no jump.
  useEffect(() => {
    if (autoFocus && !text.trim()) field.current?.focus({ preventScroll: true });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  // The result is kept with the text it belongs to; launching parses again if this hasn't caught up.
  useEffect(() => {
    if (!text.trim() || draft.parsed?.text === text) return;
    const timer = setTimeout(() => {
      api.targetParse(text).then(
        (value) => setDraft((d) => (d.text === text ? { ...d, parsed: { text, value } } : d)),
        () => {}, // launching parses again and shows the error
      );
    }, 150);
    return () => clearTimeout(timer);
  }, [text]); // eslint-disable-line react-hooks/exhaustive-deps

  const tone = look?.tone ?? "reading";
  return (
    <div className="flex flex-col gap-2">
      <label className="label" htmlFor={id}>{t("launch.targetLabel")}</label>
      <Input
        ref={field}
        id={id}
        mono
        placeholder={t("launch.targetPlaceholder")}
        autoComplete="off"
        disabled={disabled}
        value={text}
        onPaste={() => void (pasted.current = true)}
        onKeyDown={() => void (pasted.current = false)}
        onFocus={(e) => masked && e.currentTarget.select()}
        // Else the click that focused it puts the caret back.
        onMouseUp={(e) => masked && e.preventDefault()}
        onChange={(e) => {
          const v = e.target.value;
          setDraft((d) => ({ ...d, text: v, favId: null, note: null }));
        }}
      />
      <Collapse open={!!text.trim()}>
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 min-h-11">
          <span aria-hidden="true" className="launch-mark">
            <Swap k={tone} variant="blur">
              <span className="launch-mark-shape" data-tone={tone} style={look ? MARK[look.tone] : undefined} />
            </Swap>
          </span>
          <span className="flex items-baseline gap-3 min-w-0 flex-1" aria-live="polite">
            {look ? (
              <>
                <span className="font-semibold flex-none">{look.kind}</span>
                <span className={`mono text-[13px] truncate ${look.tone === "bad" ? "text-attention" : "text-fg-2"}`} title={look.detail}>
                  {look.detail}
                </span>
              </>
            ) : (
              <span className="text-fg-2">{t("launch.reading")}</span>
            )}
          </span>
          {draft.note && <span className="tag flex-none max-w-[30%] truncate">{draft.note}</span>}
          {action}
        </div>
      </Collapse>
      <Collapse open={parsed?.kind === "jobIdOnly"}>
        <div className="flex flex-col gap-1.5 pt-2 max-w-[320px]">
          <label className="label" htmlFor={`${id}-place`}>{t("launch.placeId")}</label>
          <Input
            id={`${id}-place`}
            ref={(el) => {
              // Straight here after a pasted job ID, not while one is typed.
              if (el && pasted.current) {
                pasted.current = false;
                el.focus();
              }
            }}
            mono
            inputMode="numeric"
            placeholder={t("launch.placeIdPlaceholder")}
            disabled={disabled}
            value={draft.place}
            invalid={draft.place.trim() !== "" && place === null}
            onChange={(e) => {
              const v = e.target.value;
              setDraft((d) => ({ ...d, place: v, favId: null, note: null }));
            }}
          />
        </div>
      </Collapse>
    </div>
  );
}
