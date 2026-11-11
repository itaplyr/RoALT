import { Icon, type IconName } from "../Icon";
import { Swap } from "./Morph";

/** An icon that changes with the reference's blur and scale (the island's parent slot, the theme key,
 * a state glyph). Reduced motion: a plain cross-fade. */
export function IconSwap({ name, size = 16, className }: { name: IconName; size?: number; className?: string }) {
  return (
    <span className={className} style={{ position: "relative", display: "inline-grid", placeItems: "center", width: size, height: size }}>
      <Swap k={name} variant="blur">
        <Icon name={name} size={size} />
      </Swap>
    </span>
  );
}
