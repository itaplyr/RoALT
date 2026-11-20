/** `jump.ts`'s jumpTo for the always-loaded shell: the settings registry loads when a jump is asked for. */
export const jumpTo = (id: string) => void import("./jump").then((m) => m.jumpTo(id));
