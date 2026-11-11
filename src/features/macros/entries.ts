import type { Entry } from "../settings/registry";

/** Macros' settings for the settings registry (search, jumps, rows), in screen order. */
export const MACROS_ENTRIES: readonly Entry[] = [
  { id: "ahk", place: { view: "macros" }, label: "macros.ahk", field: "ahkPath", control: { kind: "custom" }, invalidates: ["ahk"], aliases: ["ahk", "autohotkey", "exe"] },
  { id: "autorun", place: { view: "macros" }, label: "settings.macroAutorun", more: "settings.macroAutorunHint", field: "macroAutorun", control: { kind: "switch" }, aliases: ["ahk", "script", "launch"] },
];
