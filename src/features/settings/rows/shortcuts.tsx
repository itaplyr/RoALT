import { t } from "../../../i18n";
import { Kbd } from "../../../ui/Bits";
import { SHORTCUTS } from "../shortcuts";

const GO = SHORTCUTS.filter((s) => /^Ctrl\+\d$/.test(s.combo));
const ACT = SHORTCUTS.filter((s) => !GO.includes(s));

/** Every keyboard shortcut as keycaps: going to a view, then acting on what is there. Side by side
 * when there is room. */
export function ShortcutsTable() {
  return (
    <div className="set-keys">
      {[GO, ACT].map((list, i) => (
        <ul key={i}>
          {list.map((s) => (
            <li key={s.id} className="set-key">
              <span className="min-w-0">{t(s.label)}</span>
              <Kbd keys={s.combo} />
            </li>
          ))}
        </ul>
      ))}
    </div>
  );
}
