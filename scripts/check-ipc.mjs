// Checks that the IPC command names stay in sync across:
// - src/api.ts (`invoke("...")`)
// - src-tauri/src/main.rs (`generate_handler![...]`)
// - src/dev/mockIpc.ts (`case "...":`)
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const read = (p) => readFileSync(join(root, p), "utf8");

const apiSrc = read("src/api.ts");
const mainSrc = read("src-tauri/src/main.rs");
const mockSrc = read("src/dev/mockIpc.ts");

const invoked = new Set([...apiSrc.matchAll(/\binvoke(?:<[^>]*>)?\(\s*"([a-z0-9_]+)"/g)].map((m) => m[1]));
const handlerBlock = mainSrc.match(/generate_handler!\[([\s\S]*?)\]/)?.[1] ?? "";
const registered = new Set([...handlerBlock.matchAll(/[a-z0-9_]+::([a-z0-9_]+)/g)].map((m) => m[1]));
const mocked = new Set([...mockSrc.matchAll(/\bcase\s+"([a-z0-9_]+)":/g)].map((m) => m[1]));
// Commands invoked directly outside src/api.ts (e.g. error boundary logging).
const DIRECT_INVOKE = new Set(["debug_log"]);

const problems = [];
for (const cmd of invoked) {
  if (!registered.has(cmd)) problems.push(`api.ts invokes "${cmd}", which is not registered in src-tauri/src/main.rs`);
  if (!mocked.has(cmd)) problems.push(`api.ts invokes "${cmd}", which is not handled in src/dev/mockIpc.ts`);
}
for (const cmd of registered) {
  if (!invoked.has(cmd) && !DIRECT_INVOKE.has(cmd)) problems.push(`src-tauri/src/main.rs registers "${cmd}", which is never invoked in src/api.ts`);
}

if (problems.length) {
  for (const p of problems) console.error(p);
  console.error(`\n${problems.length} IPC contract problem(s).`);
  process.exit(1);
}
console.log(`IPC contract OK: ${invoked.size} invoked, ${registered.size} registered, ${mocked.size} mocked.`);
