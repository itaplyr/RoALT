import type { ReactNode, Ref } from "react";
import { byOrder, reordered } from "./homeLayout";
import { useGridDrag } from "./useGridDrag";

/** What a sorted button spreads on itself to be dragged. */
export type DragProps = ReturnType<ReturnType<typeof useGridDrag>["item"]>;

/** `ids` in the saved `order`, each `render`ed from its `name`; dragging one, or Alt and the arrows on
 * it, saves the new order. `column`: one above the other. Each must render one element, with a key. */
export function Sortable<T extends number | string>({ ids, order, onOrder, name, render, column = false, className }: {
  ids: readonly T[];
  order: T[];
  onOrder: (next: T[]) => void;
  name: (id: T) => string;
  render: (id: T, drag: DragProps) => ReactNode;
  column?: boolean;
  className?: string;
}) {
  const shown = byOrder(ids, order);
  const drag = useGridDrag((from, over) => onOrder(reordered(order, shown, from, over)), column);
  return (
    <>
      <span className="sr-only" aria-live="polite">{drag.moved}</span>
      <div ref={drag.grid as Ref<HTMLDivElement>} className={`${column ? "sort-col" : "sort-row"} ${className ?? ""}`}>
        {shown.map((id) => render(id, drag.item(name(id))))}
      </div>
    </>
  );
}
