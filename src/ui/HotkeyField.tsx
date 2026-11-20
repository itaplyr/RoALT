import { useState, type KeyboardEvent } from "react";
import { t } from "../i18n";
import { Kbd } from "./Bits";
import { IconButton } from "./Button";

const MODIFIERS = new Set(["Control", "Shift", "Alt", "Meta"]);

const SPECIAL_CODES: Record<string, string> = {
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  Insert: "Insert",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  Minus: "Minus",
  Equal: "Equal",
  BracketLeft: "BracketLeft",
  BracketRight: "BracketRight",
  Backslash: "Backslash",
  Semicolon: "Semicolon",
  Quote: "Quote",
  Comma: "Comma",
  Period: "Period",
  Slash: "Slash",
  Backquote: "Backquote",
};

/** Maps a KeyboardEvent to the shortcut string `tauri_plugin_global_shortcut` parses (e.g. `Ctrl+Shift+F12`). */
export function keyEventToCombo(e: KeyboardEvent): string | null {
  if (MODIFIERS.has(e.key)) return null;
  let main: string | null = null;
  if (/^Key[A-Z]$/.test(e.code)) main = e.code.slice(3);
  else if (/^Digit[0-9]$/.test(e.code)) main = e.code.slice(5);
  else if (/^F([1-9]|1[0-9]|2[0-4])$/.test(e.code)) main = e.code;
  else if (/^Numpad[0-9]$/.test(e.code)) main = e.code;
  else if (e.code in SPECIAL_CODES) main = SPECIAL_CODES[e.code];
  if (!main) return null;

  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  return [...mods, main].join("+");
}

/** Press-to-record global hotkey control: click to listen, press a key combination to save,
 * Esc to cancel, or press the clear button to turn the hotkey off. */
export function HotkeyField({
  label,
  value,
  describedBy,
  onCommit,
}: {
  label: string;
  value: string;
  describedBy?: string;
  onCommit: (next: string) => Promise<unknown>;
}) {
  const [recording, setRecording] = useState(false);

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
      setRecording(false);
      return;
    }
    const combo = keyEventToCombo(e);
    if (!combo) return;
    setRecording(false);
    void onCommit(combo).catch(() => {});
  };

  return (
    <span className="inline-flex items-center gap-1.5">
      <button
        type="button"
        className={`btn btn-md min-w-[144px] justify-center ${recording ? "btn-primary" : ""}`}
        aria-label={recording ? t("settings.hotkeyListening") : `${label}: ${value || t("common.off")}`}
        aria-pressed={recording}
        aria-describedby={describedBy}
        onClick={() => setRecording((r) => !r)}
        onBlur={() => setRecording(false)}
        onKeyDown={onKeyDown}
      >
        {recording ? (
          <span>{t("settings.hotkeyPress")}</span>
        ) : value ? (
          <Kbd keys={value} />
        ) : (
          <span className="text-fg-3">{t("settings.hotkeyRecord")}</span>
        )}
      </button>
      {value && !recording && (
        <IconButton
          icon="x"
          label={t("settings.hotkeyClear")}
          onClick={() => void onCommit("").catch(() => {})}
        />
      )}
    </span>
  );
}
