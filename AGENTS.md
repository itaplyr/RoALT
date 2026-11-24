# RoALT reverse-engineering workspace

This repository is the Linux fork of RoALT. It launches the Cordial runtime
(`cordial-run`), which loads Roblox's official Android engine (`libroblox.so`).
Most work in this workspace is **reverse engineering**, not editing RoALT.

This project has two orchestrators:

- **`re-orchestrator`** (project default) for reverse engineering. It decomposes
  a question, launches read-only specialist subagents in parallel, verifies
  critical claims, and writes an evidence-backed report under `re/`.
- **`code-orchestrator`** for normal RoALT development. It keeps an OpenCode
  TODO list visible, launches project coding subagents in parallel on disjoint
  files, integrates their work, runs verification, and reports the real result.

Use `/code <task>` as a shortcut for the coding orchestrator. Select the agent
explicitly when a session starts in the wrong mode.

## Hard rules

- **Read-only by default.** Never modify RoALT, the Cordial source, the installed
  runtime, the APK, or `libroblox.so` unless the user explicitly asks.
- **Never commit, amend, push, rewrite history, or create tags.**
- **Do not launch a signed-in Roblox session** unless the user explicitly
  authorizes it. If authorized, use a scratch `XDG_DATA_HOME`, a throwaway
  profile, and prefer `--headless` with `cage`.
- **Never read, print, store, or transmit session credentials**: `.ROBLOSECURITY`,
  `cookies`, `identity`, `appStorage.json` secrets, launch tickets, or command
  lines containing `gameinfo:`/`ticket`.
- **No anti-cheat bypass, DRM circumvention, or credential extraction.** This is
  lawful interoperability and feasibility analysis only.
- **Never use `glob`, `fast_edit`, or `fast_apply_edit`.** Use `read`, `grep`,
  `list`, and explicit read-only shell commands.
- **Docs are hypotheses; artifacts are evidence.** READMEs, ADRs, comments, and
  issue text never outrank source, symbols, strings, decompiled code, or a
  measured run. If they disagree, report the disagreement.
- **Label every claim**: `CONFIRMED` (observed directly), `INFERRED` (reasoned
  from evidence), or `UNKNOWN` (not verified).
- **Every claim needs evidence**: `file:line`, a commit hash, a symbol/address, a
  SHA-256, or the exact command plus its key output.
- **Pin the artifact before comparing anything.** Record the engine SHA-256, the
  Cordial commit, and the build directory.

## Artifact map

| Artifact | How to find it |
|---|---|
| RoALT source | this repository |
| RoALT design/research | `docs/DESIGN.md`, `docs/analysis/`, `docs/research/` |
| Cordial upstream | clone `https://github.com/luohoa97/cordial` to `/tmp/opencode/cordial-spike` if absent |
| Installed Cordial | `/usr/bin/cordial-run`; `pacman -Q cordial` |
| Installed commit | version string `0.21.1.r0.g12c4237-1`; full hash via GitHub API if needed |
| Engine builds | `~/.cache/cordial/builds/<version>/libroblox.so` |
| APK | `~/.cache/cordial/builds/<version>/base.apk` |
| Profiles | `~/.local/share/cordial/profiles/<name>/` |
| REA | MCP tools `rea_*`, or CLI `rea` |

Discover the newest engine instead of hardcoding a version:

```bash
BUILD=$(find "$HOME/.cache/cordial/builds" -mindepth 2 -maxdepth 2 \
  -name libroblox.so -printf '%T@ %h\n' | sort -n | tail -1 | cut -d' ' -f2-)
sha256sum "$BUILD/libroblox.so"
```

The repo name is `luohoa97/cordial` (with an `a`). `luohua97` is a typo and 404s.

## Orchestration protocol

1. **Scope.** Write the question as one or more falsifiable sub-questions. Say
   what observation would answer each one.
2. **Pin.** Record engine SHA-256, build directory, Cordial installed commit, and
   upstream HEAD. Re-fetch the installed commit into the clone when comparing:
   ```bash
   git -C /tmp/opencode/cordial-spike fetch --depth 1 origin <full-hash>
   git -C /tmp/opencode/cordial-spike show <full-hash>:path/to/file
   ```
3. **Dispatch lanes in parallel.** Use the `task` tool with the subagent types
   below. Do not duplicate a lane you delegated.
4. **Verify personally.** Re-run at least one critical command per major claim.
   Prefer source/decompiled output over a subagent's summary.
5. **Report.** State verdict, confidence, blockers/enablers with evidence, what
   remains unproven, and the next experiment. Separate confirmed facts from
   inferred claims.

## Subagent roster

| `subagent_type` | Lane | Use when |
|---|---|---|
| `re-source-scout` | Cordial/RoALT source, ADRs, commits, issues | you need code archaeology or prior art |
| `re-native-mapper` | symbols, exports, strings, procedures, xrefs, call graph | you need a map of the engine before deep dives |
| `re-decompiler` | function dossiers, pseudocode, assembly, value flow | you need to understand a specific routine |
| `re-runtime-prober` | GDB/Frida/process capture, live RSS, syscalls | static evidence is ambiguous and a run is authorized |
| `re-verifier` | independent re-check of claims and evidence | before publishing a verdict |

Launch independent lanes in one message so they run concurrently. Keep dependent
lanes sequential: map first, then decompile the mapped candidates, then verify.

## Invocation

`re-orchestrator` is the project default agent. Select it, then ask a question
that names the artifact and the decision you need, for example:

- "Map `JNI_OnLoad`, `initializeNativeCode`, and the singleton initializers in
  the newest `libroblox.so`, then tell me whether the engine can be initialized
  twice in one process."
- "Compare the installed Cordial 0.21.1 source with upstream HEAD on profile
  locking and the JavaVM, and list every difference that affects multi-engine."
- "Decide whether two engines could share one Vulkan device and one PipeWire
  connection. Static evidence first; do not run Roblox."

The orchestrator will pin the artifacts, dispatch source and native-map lanes in
parallel, send the mapped addresses to the decompiler, use the runtime prober
only with authorization, run the verifier before a verdict, and write
`re/findings/<date>-<topic>.md`.

## REA rules

- Prefer the REA MCP tools (`rea_binary_session`, `rea_open_binary`,
  `rea_binary_overview`, `rea_list_procedures`, `rea_list_strings`,
  `rea_search_procedures`, `rea_search_strings`, `rea_xrefs`,
  `rea_analyze_function`, `rea_procedure_pseudo_code`,
  `rea_procedure_assembly`, `rea_batch_decompile`, `rea_trace_native_values`).
- Use the `rea` CLI with `--format json` for independent parallel probes that
  must not disturb the shared MCP session.
- The MCP binary session is **shared across subagents**. Only the orchestrator
  or one designated agent opens/switches/closes the target. Subagents must not
  call `rea_close_binary` unless the orchestrator says so.
- `libroblox.so` is large and stripped. Ghidra auto-analysis can take minutes.
  Prefer targeted operations, `detail: summary`, and paged views.
- Use `CONFIRMED` only for what the tool actually returned. Provider pseudocode
  is a reconstruction, not original source.

## Evidence record format

Use this shape in reports and notes:

```markdown
### Claim
- Label: CONFIRMED | INFERRED | UNKNOWN
- Artifact: <path> sha256:<hash> / commit <hash>
- Evidence: <file:line | symbol@address | command + output>
- Reproduce: <exact command>
- Limits: <what this does not prove>
```

## Runtime safety

- Passive first: `inspect_web_page`, `/proc/<pid>/smaps`, logs, static analysis.
- Active runs need explicit user approval and a scratch environment:
  ```bash
  XDG_DATA_HOME=/tmp/opencode/re-profile cordial-run --profile re --headless ...
  ```
- `--headless` re-execs under `cage`; it needs `cage` on PATH and
  `CORDIAL_DEV_CONTROL=1` to be observable.
- Do not leave debugger sessions, Frida scripts, or captured processes running.
  Close sessions and report residual state.
- Launching Roblox can affect real accounts and anti-abuse state. Treat it as a
  side-effecting action, not a harmless probe.

## Coding mode: `code-orchestrator`

Normal RoALT development. This is a Tauri 2 desktop app:

- frontend: React 19 + TypeScript 7 + Vite 8 + Tailwind 4 in `src/`
- backend: Rust 2024 + Tauri 2 + tokio + rusqlite/SQLCipher in `src-tauri/src/`
- generated IPC types: `src/bindings/` (regenerate with `npm run bindings`)

### Coding roster

| `subagent_type` | Owns | Use when |
|---|---|---|
| `code-cordial-optimizer` | `src-tauri/src/launcher/`, `src-tauri/src/instances/`, `src/features/launch/`, `src/features/performance/`, `src/features/monitor/` | Cordial launch flags, profiles, multi-instance, QoS, process tracking, performance |
| `code-interface` | `src/**/*.tsx`, styles, `src/i18n/locales/` | UI, layout, interaction, accessibility, translations |
| `code-logic` | `src/api.ts`, `src/data.ts`, `src/features/**/*.ts`, cross-cutting IPC/validation | state, data flow, validation, error paths, behavior wiring |
| `code-rust` | `src-tauri/src/**` except `launcher/` and `instances/` | commands, model, storage, vault, roblox_api, rpc, network, discord, tray, webview |
| `code-github` | `.github/`, `scripts/`, `installer/`, package metadata, release docs | CI, releases, installers, repo hygiene |
| `code-reviewer` | no edits | review the diff and run final verification |

### Coding rules

- Create a `todowrite` list **before** editing. Each subagent task is one todo
  item; update it when the task starts, finishes, or blocks.
- Give each subagent explicit file ownership. Do not run two subagents against
  the same file. If a change touches one file, keep it in one lane.
- Subagents return a short report: files changed, behavior, tests run. They do
  not commit, push, or open PRs unless the user explicitly asks.
- Cargo serializes on one target directory. Do **not** let several subagents run
  `cargo build`/`cargo test`/`clippy` at once. Make the edits in parallel, then
  run verification once in the orchestrator or in `code-reviewer`.
- Never hand-edit `src/bindings/*`; change the Rust type and run
  `npm run bindings`.
- User-facing strings need every locale under `src/i18n/locales/` and
  `src-tauri/locales/`; run `npm run i18n:check`.
- Preserve the `#[cfg(windows)]` / `*_linux.rs` split. A Linux-only fix must not
  break the Windows build.
- This is an account manager: do not log cookies, tokens, passwords, or vault
  material. Treat `src-tauri/src/vault/` and `storage/` as security-sensitive.
- Prefer the smallest correct change. No new dependencies without a reason.

### Verification

Run only what the change needs, in this order:

```bash
npm run typecheck          # TypeScript
npm run i18n:check         # only if strings/locales changed
npm run build              # tsc + i18n + vite
cargo test --locked --manifest-path src-tauri/Cargo.toml
cargo clippy --all-targets --locked --manifest-path src-tauri/Cargo.toml -- -D warnings
```

Report which commands actually ran and whether they passed. Never claim a build
or test that was not executed.

## Starting context from the 2026-11-02 spike

These are prior findings, not a substitute for re-verification:

- **CONFIRMED:** Cordial is architected as one engine per process
  (`docs/architecture.md:27,89`; `docs/design/instances-and-launch.md:99-103`).
- **CONFIRMED:** the JavaVM is a process singleton; a second
  `cordial_jni_create_vm()` returns null (`native/jni_shim.cpp:126-129`).
- **CONFIRMED:** bootstrap/settings delivery is once per process
  (`crates/cordial-runtime/src/bin/load.rs:658-778`).
- **CONFIRMED:** `GameActivity`, `Surface`, `InputConnection`, `Application`,
  input natives, audio policy, and graphics slots are process-global.
- **CONFIRMED:** the profile lock is a per-profile advisory `flock`; it does not
  namespace engine state (`crates/cordial-shell/src/profile.rs:196-211,260-298`).
- **CONFIRMED:** `libroblox.so` has `JNI_OnLoad`, 3,631 `.init_array`
  constructors, a statically linked mimalloc heap (~303 MiB live), and many
  one-shot singleton strings (`LuaApp already initialized`, `initializeSingleton`).
- **INFERRED:** the engine is architecturally single-context
  (`SingleSurfaceApp`); re-entrancy is untested and likely to corrupt state.
- Prior verdict: **LIKELY IMPOSSIBLE** for independent engines in one process;
  high confidence on Cordial, medium on the engine. Sharing the runtime would
  save only ~10-40 MiB per extra engine.
