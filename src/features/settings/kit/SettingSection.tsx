import { useId, type ReactNode } from "react";
import { t, type Key } from "../../../i18n";
import { Collapse } from "../../../ui/motion";
import { entry } from "../registry";
import { EntryRow, useCtx } from "./SettingRow";

/** A group of rows: a heading over one panel, rows apart by hairlines. A row with a condition opens
 * and closes in place. `children` go after the rows (a list that isn't settings, say). */
export function SettingSection({ label, ids = [], sub, block, children }: {
  label?: Key;
  ids?: readonly string[];
  /** Under a titled part of its page (a Settings area): the heading is one level down. */
  sub?: boolean;
  /** Its name as a block of Settings' page, for the list at the side to find it. */
  block?: string;
  children?: ReactNode;
}) {
  const ctx = useCtx();
  const heading = useId();
  const Heading = sub ? "h3" : "h2";
  return (
    <section className="set-section" aria-labelledby={label && heading} data-block={block}>
      {label && <Heading id={heading} className="section-title set-heading">{t(label)}</Heading>}
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
