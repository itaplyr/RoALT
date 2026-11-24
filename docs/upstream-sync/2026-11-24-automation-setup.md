# Upstream sync: automation setup and merge feasibility

Date: 2026-11-24. Scope: `inspectsoftware/RoALT` `main` versus this Linux fork
(`itaplyr/RoALT` `main`). No commit, push, tag, build, or Roblox launch was
performed. No merge was applied now: the user chose **working-tree only** for
future automation runs.

## Pinned artifacts

- CONFIRMED — fork `HEAD` = `5fb4434238ac8df811942891eb9c71b2bd72187c`
  (`git log --oneline -1`: `5fb4434 Add RE/code agent workspace and custom RakNet launcher backend`).
- CONFIRMED — upstream `HEAD` = `e86f566aaa1f5b6b9ee14688e344f6bb53716ac2`
  (`git ls-remote https://github.com/inspectsoftware/RoALT.git HEAD`).
- CONFIRMED — last upstream commit merged into this fork =
  `d633a57fcd5ef58752510f9debd46caca86dd59f` (v1.3.0, `git log --oneline`).
- CONFIRMED — three upstream commits are pending: `59b027b`, `42b9f80`, `e86f566`
  (`git log --oneline d633a57..refs/remotes/upstream/main`).
- CONFIRMED — SVG `sha256:a03c74ef5b60eb8a873d1c3afdb03b0d53e4ffaa5140c401ced3a03cf3ce9f66`
  (`sha256sum docs/images/linux-port-status.svg`). It is unchanged by this work.

## What the automation does

- CONFIRMED — automation `aut_20e36c94d001lSuj5YBM2dX9JP`, "Upstream RoALT sync
  (local, no push)", cron `0 * * * *` (timezone `Europe/Berlin`, first due
  `2026-11-24T17:00:00+01:00`), action `bash scripts/upstream-sync.sh`.
  Created with `tools.automation.create`; verified with `tools.automation.list`.
- INFERRED — polling is a **shell action with no model call** (per the
  `automation.create` schema), so an hourly poll does not consume AI credit.
  `opencode run --auto --agent upstream-sync` is
  invoked only when `git ls-remote` reports a commit newer than the recorded one.
- CONFIRMED — state lives in `.git/upstream-sync/state.json` (untracked git
  metadata), initialized with `last_processed = d633a57…`, so the first scheduled
  run processes the three currently pending commits.
- CONFIRMED — the agent at `.opencode/agents/upstream-sync.md` (V2 `mode:
  primary`, V2 `permissions` array) merges with
  `git merge --no-commit --no-ff`, never commits, pushes, tags, or stashes,
  refuses to overwrite dirty tracked files, updates `docs/PROGRESS.md` and the
  SVG, and writes a dated report under `docs/upstream-sync/` (not
  `docs/research/`, which `.gitignore:21` excludes).
- CONFIRMED — `scripts/render-port-status.py --check` exits 0 on the current SVG
  (`SVG is up to date`), so its output is byte-identical to the committed SVG;
  future updates change only the header, tile percentages, and cell fills.
- INFERRED — no SVG generator existed in the repository before this change
  (`grep -RIn 'linux-port-status' README.md docs scripts .github package.json`
  found only the two embed sites).

## Pending upstream commits

| Commit | Date | Subject | Files |
|---|---|---|---|
| `59b027b` | 2026-11-20 | fixed ERROR_INVALID_HANDLE issue, experimental | `commands.rs`, `instances/multi.rs`, `instances/process.rs`, version files |
| `42b9f80` | 2026-11-21 | fix: overview window detection not working properly in some cases | `commands.rs`, `instances/logwatch.rs`, `instances/process.rs`, `instances/tracker.rs`, `instances/window.rs` |
| `e86f566` | 2026-11-21 | chore: bump version to 1.3.11 | `package.json`, `Cargo.toml`, `Cargo.lock`, `tauri.conf.json`, lockfile |

Evidence: `git show --stat --oneline --format='%h %s%n%ad' --date=short <commit>`
for each hash; total diff `git diff --stat d633a57..refs/remotes/upstream/main`
= 11 files, 500 insertions, 88 deletions.

## Feature triage

### Can be merged (shared code, with review)

- CONFIRMED — `logwatch.rs`: `HEAD_BYTES` 256 KiB → 1 MiB and a new
  `find_log_by_user()` that reads up to 4 MiB, skips already-bound logs and
  other launches' browser tracker ids, and matches
  `[FLog::GameJoinLoadTime] … userid:` (`42b9f80`; upstream
  `logwatch.rs:47-49,116-151`). This file is shared on Linux
  (`src-tauri/src/instances/mod.rs:34`; fork `logwatch.rs:47,93`).
- INFERRED — `tracker.rs`: `LOG_STARTUP_SECS = 15`, a one-sided log/process start
  window in `pick_client()`, `taken_logs`/`other_btids` bookkeeping, and the
  `window_pid()` helper (upstream `tracker.rs:26,888,1778-1786`; fork
  `tracker.rs:24,1813-1815`). It is shared code and the new tests are
  platform-neutral, but the benefit depends on the same Roblox log format.
- CONFIRMED — version files move `1.3.0` → `1.3.11`. This fork's release docs
  say `Cargo.toml`, `package.json`, and `tauri.conf.json` keep the three-part
  **upstream** version while four-part Linux tags add `LINUX_REVISION`
  (`docs/RELEASING-LINUX.md:3-29`, `src-tauri/src/self_update_linux.rs:28`).
  So the bump is correct to carry, but a Linux release then needs
  `LINUX_REVISION = 1` and a `v1.3.11.1` tag, not `v1.3.0.x`.

### Windows-only (carry harmlessly, not Linux features)

- CONFIRMED — `multi.rs` (~167 lines) handles `ERROR_INVALID_HANDLE` by opening
  a competing semaphore/timer/file-mapping, inspects the Windows handle table
  for holders of `ROBLOX_singletonEvent`, and finds elevated/protected Roblox
  helpers (`59b027b`; upstream `multi.rs` imports `windows::Win32::…` and
  functions `create_mutex`, `find_holders`, `open_blocking_non_event`). This
  module is `#[cfg(windows)]` here (`src-tauri/src/instances/mod.rs:43-46`);
  Linux uses `multi_linux.rs:1-6`, where multi-instance is Cordial profiles and
  there is no Windows singleton to hold.
- CONFIRMED — `process.rs` adds `unreadable_roblox_pids()`, `is_roblox_helper()`,
  and window scoring by caption/area; these are Win32 APIs. The Linux counterpart
  has `main_window(_pid) -> None` (`process_linux.rs:250`) and no singleton
  helpers.
- CONFIRMED — `window.rs` (~109 lines) guards tiny/minimized/maximized window
  resizing with `IsIconic`, `GetWindowLongPtrW`, and frame measurement; the
  Linux path is `window_linux.rs`. The *idea* (never record a collapsed size,
  never resize a minimized window) is transferable; the code is not.

### Not directly mergeable (needs Linux work)

- CONFIRMED — upstream `commands.rs` now calls `s.multi.clear_holders_cache()`
  and `s.multi.roblox_helper_holders()` (`59b027b`, upstream
  `src-tauri/src/commands.rs:~2911` and `~3094-3112`, in `multi_instance_set` and
  `close_roblox`). Neither method exists in this fork's
  `multi.rs` or `multi_linux.rs` (`grep -n 'clear_holders_cache\|roblox_helper_holders'
  src-tauri/src/instances/multi*.rs src-tauri/src/commands.rs` → no matches).
  Without Linux stubs or `#[cfg(windows)]` gating, a Linux build fails.
- INFERRED — upstream `instance_thumbnail()` replaces `client_pid()` with
  `tracker.window_pid()` (`42b9f80`, upstream `src-tauri/src/commands.rs:2579`).
  That helper
  requires `e.adopted`, which this fork sets only when adopting a bootstrapper
  client (`src-tauri/src/instances/tracker.rs:1801`); Cordial-launched clients
  may not be marked adopted. On Linux it could silently disable previews.
- UNKNOWN — whether Cordial's logs contain `[FLog::GameJoinLoadTime] userid:`
  lines at all. No Roblox/Cordial run was authorized, so the user-id log fallback
  is unproven on this fork.

## Merge outcome and SVG status

- CONFIRMED — no merge was applied in this session (`git status --short` shows
  only the three new automation files as untracked). The first scheduled run will
  attempt `git merge --no-commit --no-ff refs/remotes/upstream/main`; based on the
  missing `Multi` methods above, expect a Linux build break until stubs/gating
  are added, not a git conflict.
- CONFIRMED — the SVG is unchanged: 45 tiles, header "14 ported, 31 partial,
  untested or missing" (`docs/images/linux-port-status.svg:3`), matching
  `docs/PROGRESS.md` "Current totals".
- INFERRED — none of the three upstream commits justifies changing a Linux
  status today. All fixes are either Windows-only or untested on Linux, so
  `docs/PROGRESS.md` should keep its current statuses.

## Verification performed

```text
git ls-remote https://github.com/inspectsoftware/RoALT.git HEAD
  -> e86f566aaa1f5b6b9ee14688e344f6bb53716ac2
git log --oneline d633a57..refs/remotes/upstream/main
  -> e86f566 / 42b9f80 / 59b027b
git diff --stat d633a57..refs/remotes/upstream/main
  -> 11 files, 500 insertions(+), 88 deletions(-)
bash -n scripts/upstream-sync.sh                 -> ok
UPSTREAM_SYNC_DRY_RUN=1 bash scripts/upstream-sync.sh
  -> "new upstream commits: d633a57…..e86f566… / dry run: no fetch, no agent"
python3 scripts/render-port-status.py --check    -> "SVG is up to date"
sha256sum docs/images/linux-port-status.svg      -> a03c74ef…
```

## Blockers, unknowns, next experiment

- Blocker: `commands.rs` needs Linux `Multi` stubs or `cfg(windows)` gating before
  upstream `multi.rs` changes can coexist in a Linux build.
- Blocker: real-time detection is an hourly shell poll; there is no GitHub/MCP
  event source configured, so detection latency is up to one hour.
- Unknown: first unattended `opencode run --auto` execution has not happened yet;
  its permission behavior and runtime are unverified.
- Next experiment: let the first scheduled run complete, read its agent log at
  `.git/upstream-sync/agent.log` and its report under `docs/upstream-sync/`, then run
  `npm run typecheck` and `cargo check --locked --manifest-path src-tauri/Cargo.toml`
  on the merged working tree. Separately, inspect one real Cordial log for a
  `GameJoinLoadTime` line before treating the user-id fallback as portable.
