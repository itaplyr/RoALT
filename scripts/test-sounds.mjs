import assert from "node:assert/strict";

// Audio behavior can be checked without speakers, a browser, or a test framework.
const events = new Map();
const storage = new Map();
let now = 1000;
let notes = [];
let stops = 0;
Object.defineProperty(globalThis, "performance", { value: { now: () => now } });
globalThis.localStorage = { getItem: (key) => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) };
globalThis.document = { hidden: false, addEventListener: (name, fn, options) => events.set(name, { fn, options }) };
globalThis.window = { addEventListener() {} };
globalThis.Element = class {
  constructor(attrs = {}, selection = false) { this.attrs = attrs; this.selection = selection; }
  getAttribute(name) { return this.attrs[name] ?? null; }
  matches(selector) { return selector.includes(":disabled") ? false : this.selection; }
  closest(selector) { return selector.includes("data-sound") ? null : this; }
};
globalThis.HTMLInputElement = class extends Element {};
globalThis.AudioContext = class {
  state = "suspended"; currentTime = 0; destination = {};
  resume() { this.state = "running"; return Promise.resolve(); }
  createOscillator() {
    const note = {};
    return {
      frequency: { setValueAtTime(value) { note.from = value; }, exponentialRampToValueAtTime(value) { note.to = value; } },
      connect() {}, disconnect() {}, start() { notes.push(note); }, stop() { stops++; },
    };
  }
  createGain() { return { gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect() {}, disconnect() {} }; }
};
const sound = await import("../src/ui/sounds.ts");
sound.setSoundPreferences({ enabled: true }); // Off by default; opt in for the checks below.
sound.installSounds();
const click = events.get("click");
sound.installSounds();
assert.equal(events.get("click"), click);
assert.equal(click.options.capture, true, "read switches before asynchronous React updates");
sound.playSound("notification");
assert.equal(notes.length, 0, "don't play until the user interacts");
events.get("pointerdown").fn();
click.fn({ target: new Element({ "aria-checked": "false" }) });
assert.deepEqual(notes.map((n) => n.from), [480, 720], "switching on rises");
now += 100;
notes = [];
click.fn({ target: new Element({ "aria-checked": "true" }) });
assert.deepEqual(notes.map((n) => n.from), [600, 400], "switching off falls");
assert.equal(sound.interactionSound(new Element({ "aria-pressed": "true" }, true)), null, "selected presets stay quiet");
assert.equal(sound.interactionSound(new Element({ "aria-pressed": "false" }, true)), "click");
const checkbox = new HTMLInputElement();
checkbox.type = "checkbox"; checkbox.checked = true;
assert.equal(sound.interactionSound(checkbox), "on");
checkbox.checked = false;
assert.equal(sound.interactionSound(checkbox), "off");
notes = [];
now += 100;
sound.playSound("notification");
assert.deepEqual(notes.map((n) => n.from), [740, 620]);
sound.playSound("notification");
assert.equal(notes.length, 2, "rate-limit repeated notifications");
sound.setSoundPreferences({ enabled: false });
assert.ok(stops > 6, "muting stops queued sounds");
now += 2000;
sound.playSound("error");
assert.equal(notes.length, 2);
sound.setSoundPreferences({ enabled: true, volume: 0 });
sound.playSound("click");
assert.equal(notes.length, 2, "zero volume is silent");
sound.setSoundPreferences({ volume: 0.4 });
document.hidden = true;
sound.playSound("click");
assert.equal(notes.length, 2, "hidden views stay quiet");
assert.deepEqual(JSON.parse(storage.get("roalt.sounds")), { enabled: true, volume: 0.4 });
console.log("Sound checks passed: toggle direction, notifications, throttling, mute, volume, persistence, and event order.");
