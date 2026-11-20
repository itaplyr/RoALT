import assert from "node:assert/strict";

// Custom looks: whatever is picked, text, borders and status colours read on the surfaces they sit on.
const { BUILTINS, contrast, lookTokens, readShared, builtinOf } = await import("../src/app/look.ts");
const { fromCode, toCode } = await import("../src/ui/shareCode.ts");

/** Every pair the readability pass promises, at its minimum. */
function readsWell(look, name) {
  const c = lookTokens(look).colors;
  const surfaces = [c.ground, c["surface-1"], c["surface-2"], c["surface-3"], c.float, c.island, c.thumb];
  const checks = [
    ["fg", surfaces, 4.5],
    ["fg-2", [c.ground, c["surface-1"], c["surface-2"], c.float], 4.5],
    ["fg-3", [c.ground, c["surface-1"], c.float], 4.5],
    ["edge", [c.ground, c["surface-1"], c["surface-2"], c.island], 3],
    ["on-fg", [c.fg], 4.5],
    ...["ok", "info", "attention", "warn", "err"].map((id) => [id, [c.ground, c["surface-1"], c.float], 4.5]),
  ];
  for (const [id, on, min] of checks)
    for (const bg of on) assert.ok(contrast(c[id], bg) >= min - 1e-9, `${name}: ${id} ${c[id]} on ${bg} is ${contrast(c[id], bg).toFixed(2)}:1`);
}

// The bases as they come, the built-ins, and colours chosen to be unreadable.
readsWell({ base: "dark", colors: {} }, "dark");
readsWell({ base: "light", colors: {} }, "light");
for (const b of BUILTINS) readsWell(b.look, b.id);
readsWell({ base: "light", colors: { "surface-1": "#ffffff", fg: "#ffffff" } }, "white on white");
readsWell({ base: "dark", colors: { ground: "#000000", fg: "#000000", edge: "#000000", ok: "#000000" } }, "black on black");
readsWell({ base: "dark", colors: { ground: "#ff0000", "surface-1": "#00ff00", fg: "#0000ff" } }, "loud");

// The bases' own colours aren't touched: an empty look is the theme as it was.
const dark = lookTokens({ base: "dark", colors: {} }).colors;
assert.equal(dark.fg, "#ededed");
assert.equal(dark["surface-1"], "#111111");
// A picked ground carries the surfaces worked out from it; a pick itself is kept.
const blue = lookTokens({ base: "dark", colors: { ground: "#0b1020" } }).colors;
assert.equal(blue.ground, "#0b1020");
assert.notEqual(blue["surface-1"], "#111111", "panels follow the picked ground");
// Unknown ids and bad colours are ignored.
assert.equal(lookTokens({ base: "dark", colors: { nope: "#ff0000", fg: "red" } }).colors.fg, "#ededed");

// Share codes: a round trip, and what isn't one.
const shared = { v: 1, base: "dark", colors: { ground: "#0b1020", fg: "#e6ecff" }, accent: "#7aa2ff", corners: 150 };
const code = toCode("RoALT-THEME:", JSON.stringify(shared));
assert.deepEqual(readShared(fromCode("RoALT-THEME:", code)), shared);
assert.equal(fromCode("RoALT-THEME:", "RoALT-PERF:abc"), null);
assert.equal(fromCode("RoALT-THEME:", "RoALT-THEME:!!"), null);
assert.equal(readShared('{"v":2,"base":"dark","colors":{},"accent":"","corners":100}'), null, "a newer version");
assert.equal(readShared('{"v":1,"base":"dark","colors":{},"accent":"red","corners":100}'), null, "a bad accent");
assert.equal(readShared('{"v":1,"base":"dark","colors":{},"accent":"","corners":42}'), null, "corners off the list");
assert.deepEqual(readShared('{"v":1,"base":"light","colors":{"fg":"#ABCDEF","x":"#000000","ground":"blue"},"accent":"","corners":0}')?.colors, { fg: "#abcdef" }, "keeps only colours it knows");

// A built-in is recognised until it is edited.
assert.equal(builtinOf(BUILTINS[0].look)?.id, BUILTINS[0].id);
assert.equal(builtinOf({ ...BUILTINS[0].look, colors: { ...BUILTINS[0].look.colors, fg: "#000001" } }), null);

console.log("look: ok");
