import { t } from "../../../i18n";
import { Kbd } from "../../../ui/Bits";
import { NumberField, OffOrNumberField, Segmented, Switch, TextField } from "../../../ui/controls";
import { Select } from "../../../ui/Field";
import { say, type Field } from "../registry";
import { patchFor, valueOf } from "./patch";
import { useRow } from "./rowError";

const WIDTH = { sm: "w-32", md: "w-48", lg: "w-64" } as const;

/** The control an entry declares, bound to its field. It saves quietly; a refusal or a bad number
 * shows at its row. Custom, action and link entries draw nothing here: their view does. */
export function AutoControl() {
  const { entry: e, ctx, write, setError, describedBy } = useRow();
  const c = e.control;
  // Before reading the field: theirs may not be a Settings one ("vault.mode").
  if (c.kind === "custom" || c.kind === "action" || c.kind === "link") return null;
  const f = (typeof e.field === "string" ? e.field : e.field?.[0]) as Field;
  const label = t(e.label);
  const value = f && valueOf(f, ctx.s);
  const put = (v: unknown) => write(patchFor(f, v, ctx.s));
  switch (c.kind) {
    case "switch":
      return <Switch label={label} checked={value as boolean} onChange={put} describedBy={describedBy} />;
    case "segmented":
      return (
        <Segmented
          label={c.label ? t(c.label) : label}
          value={value as string | number}
          mono={c.mono}
          options={c.options.map((o) => ({ value: o.value, label: say(o.label), title: o.title && say(o.title) }))}
          onChange={put}
        />
      );
    case "select":
      return <Select value={value as string} onChange={put} options={c.options.map((o) => ({ value: o.value, label: say(o.label), lang: o.lang }))} aria-label={label} aria-describedby={describedBy} />;
    case "number": {
      const k = c.scale ?? 1;
      return (
        <NumberField
          name={`${label}, ${say(c.unit)}`}
          unit={say(c.unit)}
          short={say(c.short)}
          min={c.min}
          max={c.max}
          step={c.step}
          value={(value as number) / k}
          onCommit={(n) => void put(Math.round(n * k))}
          onInvalid={setError}
          describedBy={describedBy}
        />
      );
    }
    case "offOrValue":
      return <OffOrNumberField label={label} value={value as number} range={c} unit={say(c.unit)} short={say(c.short)} onCommit={(n) => void put(n)} describedBy={describedBy} />;
    case "text":
      // A refused save rejects, so the field puts the saved text back.
      return <TextField label={label} value={value as string} placeholder={t(c.placeholder)} className={WIDTH[c.size]} describedBy={describedBy} onCommit={(v) => put(v).then((s) => s ?? Promise.reject())} />;
    case "shortcut":
      return <Kbd keys={c.combo} />;
    default:
      return null;
  }
}
