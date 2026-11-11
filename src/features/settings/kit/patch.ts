import type { Settings, SettingsPatch } from "../../../api";
import type { Field } from "../registry";

/** A field's value: "antiAfk.intervalMin" reads into the nested object. */
export const valueOf = (f: Field, s: Settings): unknown => f.split(".").reduce<unknown>((o, k) => (o as Record<string, unknown>)[k], s);

/** The patch that sets `f` to `v`. A nested field sends its whole object, merged with the current one:
 * the backend replaces nested objects whole. */
export function patchFor(f: Field, v: unknown, s: Settings): SettingsPatch {
  const [top, ...path] = f.split(".");
  const put = (o: unknown, [k, ...rest]: string[]): unknown => (k === undefined ? v : { ...(o as object), [k]: put((o as Record<string, unknown>)[k], rest) });
  return { [top]: put(s[top as keyof Settings], path) } as SettingsPatch;
}

if (import.meta.env.DEV) {
  const s = { fpsCap: 30, adaptive: { enabled: true, background: { cpuCap: 25, eco: false, trimEveryMin: null, affinity: null } } } as unknown as Settings;
  console.assert(JSON.stringify(patchFor("fpsCap", 60, s)) === '{"fpsCap":60}', "patchFor: a plain field");
  console.assert(
    JSON.stringify(patchFor("adaptive.background.eco", true, s)) === '{"adaptive":{"enabled":true,"background":{"cpuCap":25,"eco":true,"trimEveryMin":null,"affinity":null}}}',
    "patchFor: a nested field keeps its siblings",
  );
  console.assert(valueOf("adaptive.background.cpuCap", s) === 25, "valueOf: a nested field");
}
