import assert from "node:assert/strict";
import { releaseNotes } from "../src/features/news/notes.ts";

// The shape RoALT's release notes have on GitHub.
const md = `## Fixes
- **Anti-AFK now really keeps clients online.** Roblox only counts a real key press.
- Anti-AFK has one mode now.

## New
* **Join our Discord** button above [Donate](https://example.com).

No Authenticode signature yet: SmartScreen warns.

## Empty
**Layout**
`;
assert.deepEqual(releaseNotes(md), [
  { title: "Fixes", lines: ["Anti-AFK now really keeps clients online. Roblox only counts a real key press.", "Anti-AFK has one mode now."] },
  { title: "New", lines: ["Join our Discord button above Donate."] },
]);
// Items before any heading get an untitled section; nothing usable gives none.
assert.deepEqual(releaseNotes("- one\r\n- `two`"), [{ title: "", lines: ["one", "two"] }]);
assert.deepEqual(releaseNotes(""), []);
assert.deepEqual(releaseNotes("Just a paragraph."), []);
console.log("News checks passed: release notes.");
