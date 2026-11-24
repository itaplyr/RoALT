# Upstream sync merge report — 2026-11-24

Scope: `inspectsoftware/RoALT` `refs/remotes/upstream/main` into this Linux fork
(`itaplyr/RoALT`), range `d633a57fcd5ef58752510f9debd46caca86dd59f..e86f566aaa1f5b6b9ee14688e344f6bb53716ac2`.
This is a working-tree/index merge only. No commit, amend, stash, push, pull,
tag, PR, build, test, or Roblox/Cordial launch was performed.

- CONFIRMED — the earlier same-date automation setup/feasibility note was
  preserved as `docs/upstream-sync/2026-11-24-automation-setup.md`; this file is
  the merge-execution report for the same pending range.
  Evidence: `ls -la docs/upstream-sync/` after the rename.

## Pinned artifacts

- CONFIRMED — fork `HEAD` before and after the merge =
  `5fb4434238ac8df811942891eb9c71b2bd72187c` (`git rev-parse HEAD`;
  `git log --oneline -1` = `5fb4434 Add RE/code agent workspace and custom RakNet launcher backend`).
- CONFIRMED — upstream head / resulting `MERGE_HEAD` =
  `e86f566aaa1f5b6b9ee14688e344f6bb53716ac2` (`git rev-parse refs/remotes/upstream/main`
  and `git rev-parse HEAD MERGE_HEAD`).
- CONFIRMED — merge base of this range =
  `d633a57fcd5ef58752510f9debd46caca86dd59f` (`git rev-parse d633a57…`).
- CONFIRMED — pending list in `.git/upstream-sync/pending-commits.txt` has
  exactly `e86f566`, `42b9f80`, `59b027b`, matching
  `git log --oneline d633a57…..refs/remotes/upstream/main`.
- CONFIRMED — upstream diff size =
  `git diff --stat d633a57…..refs/remotes/upstream/main` → 11 files,
  500 insertions(+), 88 deletions(-).

## New commits processed

- CONFIRMED — `59b027bb64058b5dc5a93529dccc857f05ced43a`,
  2026-11-20, `fixed ERROR_INVALID_HANDLE issue, experimental`:
  `commands.rs`, `instances/multi.rs`, `instances/process.rs`, version files.
- CONFIRMED — `42b9f80194d3eeaad943f1b8ec9665488ac7ee88`,
  2026-11-21, `fix: overview window detection not working properly in some cases`:
  `commands.rs`, `instances/logwatch.rs`, `instances/process.rs`,
  `instances/tracker.rs`, `instances/window.rs`.
- CONFIRMED — `e86f566aaa1f5b6b9ee14688e344f6bb53716ac2`,
  2026-11-21, `chore: bump version to 1.3.11`:
  `package-lock.json`, `package.json`, `src-tauri/Cargo.lock`,
  `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`.

Evidence for all three: `git show --stat --format=fuller <hash>` and
`git show --format= <hash>`.

## Classification of every upstream change

| Class | Commit | Change, commit and `file:line` / symbol | Linux relevance and label |
|---|---|---|---|
| Fork-specific | `e86f566` | Version `1.3.1` → `1.3.11` in `package.json:4`, `package-lock.json:3,9`, `src-tauri/Cargo.toml:3`, `src-tauri/Cargo.lock:3613`, `src-tauri/tauri.conf.json:4` (`git show e86f566:<file>`). `59b027b` had already moved `1.3.0` → `1.3.1` in the same five files. | CONFIRMED — `docs/RELEASING-LINUX.md:3-29` says the fork uses `v<upstream version>.<linux revision>` and that `Cargo.toml`, `package.json` and `tauri.conf.json` keep the three-part upstream version; the file still names upstream `1.3.0` and `v1.3.0.1` at `:14,17,20-25`. INFERRED — this upstream bump is correct to carry, but release docs/tag expectations need a fork-side update before any Linux release. No tag was created. |
| Mergeable | `42b9f80` | `src-tauri/src/instances/logwatch.rs`: `HEAD_BYTES` 256 KiB → 1 MiB (`:47`), new `USER_HEAD_BYTES` 4 MiB (`:49`), `find_log_by_user()` (`:116`) and `names_user()` match `[FLog::GameJoinLoadTime] … userid:` while excluding bound logs and other launches' BTIDs. | CONFIRMED — `logwatch` is shared on Linux (`src-tauri/src/instances/mod.rs:34`) and this file auto-merged without conflict. INFERRED — useful to the fork only if Cordial logs contain that line; no Linux run was made. |
| Mergeable | `42b9f80` | `src-tauri/src/instances/tracker.rs`: `LOG_STARTUP_SECS = 15` (`:26`), `Tracker::window_pid()` (`:888-893`), extra `observe()` arguments `taken_logs`/`other_btids` (`:1640-1652`), one-sided start window in `pick_client()` (`:1778-1791`). | CONFIRMED — `tracker` is shared (`src-tauri/src/instances/mod.rs:75`) and the new platform-neutral tests merged. CONFIRMED — `Tracker::poll()` conflicts with the fork's Cordial profile-liveness check (see Merge outcome), so the file is not cleanly merged. INFERRED — the log/process timing fix may apply to Cordial, but that is unverified. |
| Mergeable | `42b9f80` | `src-tauri/src/commands.rs`: `instance_thumbnail()` (`:2577-2580`) now checks `s.tracker.get(&id)` and takes `s.tracker.window_pid(&id)` instead of `client_pid()`. | CONFIRMED — this hunk auto-merged; post-merge call site is `src-tauri/src/commands.rs:2720` and Linux `window::preview` exists at `src-tauri/src/instances/window_linux.rs:113`. INFERRED — `window_pid()` requires `e.adopted`, so previews could change for Cordial-launched clients that are not marked adopted; not run on Linux. |
| Windows-only | `59b027b` | `src-tauri/src/commands.rs`: `s.multi.clear_holders_cache()` in `multi_instance_set` (`:2914`) and `close_roblox` (`:3096,3106`); `s.multi.roblox_helper_holders()` and helper-process closing in `close_roblox` (`:3099-3108`). | CONFIRMED — these call Windows singleton-holder methods; `close_roblox` is one of the three conflicts. `multi_linux.rs` exposes only `enable` (`:36`), `disable` (`:45`) and `status` (`:78`), with no `clear_holders_cache` or `roblox_helper_holders`. INFERRED — a Linux build will fail until these calls are gated or stubbed; no build was run. |
| Windows-only | `59b027b` | `src-tauri/src/instances/multi.rs`: Win32 imports `OpenFileMappingW`, `OpenSemaphoreW`, `OpenWaitableTimerW` (`:24-28`); `Multi::clear_holders_cache()` (`:121`), `Multi::roblox_helper_holders()` (`:127`), `create_mutex()`'s `ERROR_INVALID_HANDLE` path (`:345-373`), `open_blocking_non_event()` (`:382-389`), `find_holders()` handle-table/elevated-owner checks (`:539-580`), plus semaphore/timer tests. | CONFIRMED — all are Win32 kernel-object/singleton behavior for `ROBLOX_singletonEvent`; this module is `#[cfg(windows)]` in this fork (`src-tauri/src/instances/mod.rs:42-46`), and Linux uses `multi_linux.rs` (Cordial profiles). Harmless to carry in the merge, not a Linux feature. |
| Windows-only | `59b027b` | `src-tauri/src/instances/process.rs`: `unreadable_roblox_pids()` via `CreateToolhelp32Snapshot` (`:47-65`) and `is_roblox_helper()` for `RobloxCrashHandler.exe` / `RobloxPlayerLauncher.exe` (`:67-69`). | CONFIRMED — Win32 process/handle inspection for the Windows singleton. The Linux counterpart has no such helpers; `process_linux.rs:39,133,148,250` supplies `cordial_processes`, `profile_of`, `profiles_by_user` and a handle-less `main_window`. |
| Windows-only | `42b9f80` | `src-tauri/src/instances/process.rs`: `main_window()` (`:282-318`) scores top-level HWNDs with `GetWindowLongPtrW`, `GetWindowRect`, `WS_CAPTION`, `WS_EX_TOOLWINDOW` and `IsIconic`. | CONFIRMED — Win32 window queries; this hunk overlaps the fork's `cordial_processes`/profile helpers and is one of the three conflicts. Linux `main_window` returns `(u32, bool)` without an HWND (`process_linux.rs:250`). |
| Windows-only | `42b9f80` | `src-tauri/src/instances/window.rs`: `MIN_CLIENT_SIZE`/`sane_client_size()` (`:152-155`), minimized/maximized guards in `keep_tiny()`, `free_resize()` (`:478`), `micro()` (`:521`) and `normal()` (`:733`), and `frame()`/`frame_or_default()` (`:752-775`), with Win32 `IsIconic`, `SetWindowPos`, `GetWindowRect`, `GetClientRect`. | CONFIRMED — this module is `#[cfg(windows)]` (`src-tauri/src/instances/mod.rs:69-73`); Linux uses `window_linux.rs`. The invariant "never record/apply a collapsed size or resize a minimized window" is transferable as an idea, but the merged code is not a Linux port. |

- CONFIRMED — all 11 changed upstream paths are accounted for in the table
  above (`git diff --stat d633a57……e86f566…` plus the staged file list below).

## Local merge outcome

- CONFIRMED — command run: `git merge --no-commit --no-ff refs/remotes/upstream/main`.
  Verbatim output (exit code `1`):

```text
Auto-merging src-tauri/Cargo.lock
Auto-merging src-tauri/Cargo.toml
Auto-merging src-tauri/src/commands.rs
CONFLICT (content): Merge conflict in src-tauri/src/commands.rs
Auto-merging src-tauri/src/instances/logwatch.rs
Auto-merging src-tauri/src/instances/process.rs
CONFLICT (content): Merge conflict in src-tauri/src/instances/process.rs
Auto-merging src-tauri/src/instances/tracker.rs
CONFLICT (content): Merge conflict in src-tauri/src/instances/tracker.rs
Auto-merging src-tauri/tauri.conf.json
Automatic merge failed; fix conflicts and then commit the result.
```

- CONFIRMED — exactly three paths conflicted: `src-tauri/src/commands.rs`,
  `src-tauri/src/instances/process.rs`, `src-tauri/src/instances/tracker.rs`
  (`git diff --name-only --diff-filter=U`). All conflict markers were left in
  place; no side was discarded or forced.
- CONFIRMED — conflict marker locations (`grep -nE '^(<<<<<<<|=======|>>>>>>>)'`
  on those files):
  `commands.rs:3378,3380,3394`; `process.rs:46,60,85`;
  `tracker.rs:1442,1457,1460`.
- CONFIRMED — the competing sides are: `commands.rs` fork
  `Ok(multi_status(s, &v))` versus upstream helper-holder close plus
  `s.multi.status(...)`; `process.rs` fork `cordial_processes`/`profile_of`/
  `profiles_by_user` versus upstream `unreadable_roblox_pids`/
  `is_roblox_helper`; `tracker.rs` fork `observe(...)` plus Cordial
  profile-liveness handling versus upstream `observe(..., &mut taken_logs,
  &other_btids, now)`.
- CONFIRMED — pre-merge `git status --porcelain` showed only untracked
  `.opencode/agents/`, `docs/upstream-sync/`, `scripts/render-port-status.py`
  and `scripts/upstream-sync.sh`; no tracked file touched by the upstream diff
  was dirty, so the merge was allowed to start.
- CONFIRMED — post-merge `git status --porcelain`: eight paths staged clean
  (`package-lock.json`, `package.json`, `src-tauri/Cargo.lock`,
  `src-tauri/Cargo.toml`, `src-tauri/src/instances/logwatch.rs`,
  `src-tauri/src/instances/multi.rs`, `src-tauri/src/instances/window.rs`,
  `src-tauri/tauri.conf.json`), three paths `UU` (the conflicts above), and the
  same four untracked fork paths.
- CONFIRMED — `HEAD` is still `5fb4434…` and `MERGE_HEAD` is `e86f566…`; the
  merge remains uncommitted in the working tree/index.
- CONFIRMED — no build or test command was executed; `cargo`/`npm` verification
  is intentionally deferred until a porter resolves the conflicts. INFERRED —
  the merged tree does not currently compile because conflict markers and
  missing Linux `Multi` methods remain.

## PROGRESS.md and SVG status

- CONFIRMED — `docs/PROGRESS.md` was not edited: `git diff --name-only -- docs/PROGRESS.md`
  printed nothing. There is no Linux run or resolved port change in this merge,
  so no status moved; "Current totals" remain **14 ported, 31 partial,
  untested or missing** (`docs/PROGRESS.md:26`).
- CONFIRMED — `python3 scripts/render-port-status.py --check` printed
  `SVG is up to date` and exited `0`; no re-render was needed.
- CONFIRMED — `sha256sum docs/images/linux-port-status.svg` =
  `a03c74ef5b60eb8a873d1c3afdb03b0d53e4ffaa5140c401ced3a03cf3ce9f66`,
  unchanged from the pre-merge setup note.

## Blockers and unknowns

- CONFIRMED blocker — three unresolved conflict paths listed above. No
  resolution was attempted, per the merge lane rules.
- CONFIRMED blocker — upstream `commands.rs` calls
  `Multi::clear_holders_cache()` / `Multi::roblox_helper_holders()` while the
  Linux `multi` module (`multi_linux.rs`) defines neither
  (`src-tauri/src/instances/multi_linux.rs:36,45,78`). INFERRED — needs
  `#[cfg(windows)]` gating or Linux-equivalent stubs before a Linux build.
- CONFIRMED blocker — the staged version bump makes the working tree
  `1.3.11`, while `docs/RELEASING-LINUX.md:14,17,20-25` still describes
  `1.3.0`/`v1.3.0.1`. INFERRED — release notes and the next Linux revision
  must be aligned before tagging (this lane does not tag).
- INFERRED — `window_pid()`'s `e.adopted` condition may suppress Linux
  previews for Cordial-launched clients; not observed in a run.
- UNKNOWN — whether Cordial logs contain
  `[FLog::GameJoinLoadTime] … userid:`, so the value of the user-id log
  fallback on Linux is unproven. No Roblox/Cordial session was launched.
- UNKNOWN — the correct conflict-resolution shape for the fork's Cordial
  profile-liveness logic versus upstream's log rebinding; that is a porter
  decision, not an automatic merge decision.
- INFERRED — next experiment after porter resolution: run
  `cargo check --locked --manifest-path src-tauri/Cargo.toml` and a Linux
  preview/launch session, then update `docs/PROGRESS.md` and re-render the SVG
  only if a status actually changes.

## Verification commands recorded

```text
git rev-parse HEAD refs/remotes/upstream/main d633a57… e86f566…
  -> 5fb4434238ac8df811942891eb9c71b2bd72187c
     e86f566aaa1f5b6b9ee14688e344f6bb53716ac2
     d633a57fcd5ef58752510f9debd46caca86dd59f
     e86f566aaa1f5b6b9ee14688e344f6bb53716ac2
git log --oneline d633a57…..refs/remotes/upstream/main
  -> e86f566 / 42b9f80 / 59b027b
git diff --stat d633a57…..refs/remotes/upstream/main
  -> 11 files, 500 insertions(+), 88 deletions(-)
git status --porcelain                 # before merge: untracked files only
git merge --no-commit --no-ff refs/remotes/upstream/main
  -> three content conflicts, exit 1 (verbatim output above)
git diff --name-only --diff-filter=U    -> commands.rs, process.rs, tracker.rs
git rev-parse HEAD MERGE_HEAD           -> 5fb4434… / e86f566…
python3 scripts/render-port-status.py --check
  -> "SVG is up to date" (exit 0)
sha256sum docs/images/linux-port-status.svg
  -> a03c74ef5b60eb8a873d1c3afdb03b0d53e4ffaa5140c401ced3a03cf3ce9f66
```
