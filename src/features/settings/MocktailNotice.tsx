import { useId } from "react";
import { t, type Key } from "../../i18n";
import { Icon } from "../../ui/Icon";

/** The catalog keys this notice needs. They are not in `src/i18n/locales/*.json` yet (this lane
 * must not edit catalogs), so `pending` shows the English line below until every locale carries
 * them. Once they do, `t` returns the translation and the fallback is unused. */
const KEYS = {
  title: "settings.mocktailTitle" as Key,
  text: "settings.mocktailText" as Key,
};

const ENGLISH = {
  title: "Mocktail is experimental",
  text: "Mocktail normally allows one client. With Multi-instance on, RoALT uses one launcher copy and isolated data per account; this is experimental and not verified with a signed-in launch. There are no live previews.",
};

/** `t` returns the key itself while the catalogs have no entry for it: show the English line then. */
function pending(key: Key, english: string) {
  const text = t(key);
  return text === key ? english : text;
}

/** A small, non-blocking note about the Mocktail backend: one client at a time, no live previews.
 * Shown in Settings › Launching and Performance › RoALT, beside the rows it affects. */
export function MocktailNotice({ className = "" }: { className?: string }) {
  const titleId = useId();
  return (
    <div className={`notice ${className}`} role="note" aria-labelledby={titleId} data-mocktail>
      <Icon name="info" size={16} className="shrink-0 text-fg-2" />
      <div className="flex-[1_1_320px] min-w-0">
        <p id={titleId} className="font-medium">
          {pending(KEYS.title, ENGLISH.title)}
        </p>
        <p className="text-fg-2 text-[13px]">{pending(KEYS.text, ENGLISH.text)}</p>
      </div>
    </div>
  );
}
