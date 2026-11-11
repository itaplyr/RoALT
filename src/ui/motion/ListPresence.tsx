import type { ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import { useReducedMotionFlag } from "./reduced";
import { RISE, SPRING, STAGGER, TWEEN } from "./tokens";
import { staggerDelay, useFreshItems } from "./lists";

/** A short list whose items rise in (staggered when several arrive together) and fold away when
 * removed. Lists past `limit` render plainly: no per-row motion on long lists. */
export function ListPresence<T>({ items, getKey, render, limit = STAGGER.listLimit, as = "div", className, itemClassName }: {
  items: T[];
  getKey: (item: T) => string | number;
  render: (item: T, index: number) => ReactNode;
  limit?: number;
  as?: "div" | "ul" | "ol";
  className?: string;
  itemClassName?: string;
}) {
  const reduced = useReducedMotionFlag();
  const keys = items.map(getKey);
  const fresh = useFreshItems(keys, limit);
  const Item = as === "div" ? motion.div : motion.li;
  const List = as;
  if (items.length > limit) {
    return (
      <List className={className}>
        {items.map((item, i) =>
          as === "div" ? (
            <div key={getKey(item)} className={itemClassName}>{render(item, i)}</div>
          ) : (
            <li key={getKey(item)} className={itemClassName}>{render(item, i)}</li>
          ),
        )}
      </List>
    );
  }
  return (
    <List className={className}>
      <AnimatePresence initial={false}>
        {items.map((item, i) => {
          const k = getKey(item);
          const n = fresh.indexOf(k);
          return (
            <Item
              key={k}
              className={itemClassName}
              style={{ overflow: "clip", overflowClipMargin: 4 }}
              initial={{ opacity: 0, y: reduced ? 0 : RISE.md }}
              animate={{ opacity: 1, y: 0, transition: { ...SPRING.smooth, delay: n > 0 ? staggerDelay(n) : 0 } }}
              // Reduced motion: the height jumps, only the fade eases.
              exit={{ opacity: 0, height: 0, transition: reduced ? { height: { duration: 0 }, opacity: { duration: 0.08 } } : TWEEN.exit }}
            >
              {render(item, i)}
            </Item>
          );
        })}
      </AnimatePresence>
    </List>
  );
}
