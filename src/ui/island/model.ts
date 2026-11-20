import type { IconName } from "../Icon";

/** One place the island goes. `current`: it is on screen (a solid tile wearing its label). */
export type IslandLeaf = {
  id: string;
  icon: IconName;
  label: string;
  /** Shown in its tooltip ("Ctrl+3"). */
  shortcut?: string;
  /** A quiet count beside the label while current ("Accounts 12"). */
  count?: number;
  /** Something that needs a look: a small badge on the tile. */
  badge?: number;
  current: boolean;
  onSelect: () => void;
};

/** A group that opens in place as an inset pill (More tools). Collapsed, its tile shows the current
 * child's icon and label. */
export type IslandGroup = {
  id: string;
  icon: IconName;
  label: string;
  children: IslandLeaf[];
  /** Children the user can reorder (saved by the caller). */
  order?: { ids: string[]; onOrder: (ids: string[]) => void };
};

export type IslandModel = {
  /** The landmark's name. */
  label: string;
  main: IslandLeaf[];
  mainOrder: { ids: string[]; onOrder: (ids: string[]) => void };
  groups: IslandGroup[];
  /** Places after the groups, in a fixed order (Settings). */
  end: IslandLeaf[];
};
