import { useId, type ReactNode } from "react";
import { t, type Key } from "../../../i18n";
import { Collapse } from "../../../ui/motion";
import { entry } from "../registry";
import { EntryRow, useCtx } from "./SettingRow";

/** A group of rows: a heading over one panel, rows apart by hairlines. A row with a condition opens
 * and closes in place. `children` go after the rows (a list that isn't settings, say). */
export function SettingSection({ label, ids = [], children }: { label?: Key; ids?: readonly string[]; children?: ReactNode }) {
  const ctx = useCtx();
  const heading = useId();
  return (
    <section className="set-section" aria-labelledby={label && heading}>
      {label && <h2 id={heading} className="section-title set-heading">{t(label)}</h2>}
      <div className="panel set-group">
        {ids.map((id) => {
          const when = entry(id).when;
          return when ? (
            <Collapse key={id} open={when(ctx)} className="set-wrap">
              <EntryRow id={id} />
            </Collapse>
          ) : (
            <EntryRow key={id} id={id} />
          );
        })}
        {children}
      </div>
    </section>
  );
}
