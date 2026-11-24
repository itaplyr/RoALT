# Upstream v1.3.11 → Linux fork: integration and SVG refresh

Date: 2026-11-24. Scope: finish the no-commit merge of upstream
`inspectsoftware/RoALT` `main` into this Linux fork's working tree, make the
merged changes build and test on Linux, and regenerate the port-status SVG.
No commit, push, tag, stash, Roblox or Cordial session was performed.

## Pinned artifacts

- CONFIRMED — fork `HEAD` = `5fb4434238ac8df811942891eb9c71b2bd72187c`;
  `MERGE_HEAD` = `e86f566aaa1f5b6b9ee14688e344f6bb53716ac2`. Both are still
  set: `git rev-parse HEAD MERGE_HEAD` (merge remains in progress, no commit).
- CONFIRMED — upstream commits processed: `59b027b`, `42b9f80`, `e86f566`
  (`git log --oneline d633a57..refs/remotes/upstream/main`).
- CONFIRMED — version `1.3.11` is consistent in `src-tauri/Cargo.toml:3`,
  `src-tauri/Cargo.lock:3613`, `package.json:4`, `package-lock.json:3,10`,
  `src-tauri/tauri.conf.json:3`.
- CONFIRMED — SVG before this work `sha256:a03c74ef…`; after
  `sha256:767d669f…`.

## What was done

### Conflict resolution (all three `UU` paths)

- CONFIRMED — `src-tauri/src/instances/process.rs`: kept the fork's Windows
  stubs `cordial_processes()`, `profile_of()`, `profiles_by_user()` **and**
  upstream's `unreadable_roblox_pids()` / `is_roblox_helper()` from `59b027b`.
  The restored closing brace of `profiles_by_user()` was verified in the
  read-back. All code is `#[cfg(windows)]`; Linux uses `process_linux.rs`.
- CONFIRMED — `src-tauri/src/instances/tracker.rs`: call upstream's new
  8-argument `observe(...)` (signature at `tracker.rs:1714-1723`) with
  `other_btids`, then retain the fork's Cordial/cage profile-liveness check
  immediately after it. Neither behavior was dropped.
- CONFIRMED — `src-tauri/src/commands.rs`: `close_roblox()` keeps the staged
  `clear_holders_cache()` / `retake_multi()` calls, then upstream's helper
  closing block, and returns the fork's `multi_status(s, &v)` so the mocktail
  exception at `commands.rs:3141-3148` still applies.

### Linux-specific adaptations

- CONFIRMED — `src-tauri/src/instances/multi_linux.rs:70-81`: added
  `clear_holders_cache()` (no-op) and `roblox_helper_holders() -> Vec<(u32,
  Option<u64>)>` (empty). These satisfy the three shared `commands.rs` calls
  without inventing Windows semantics on Linux. `is_active()` already existed.
- CONFIRMED — `src-tauri/src/commands.rs:2719-2723`: `instance_thumbnail()` now
  uses `#[cfg(windows)] let pid = tracker.window_pid(&id)` and on Linux falls
  back to `client_pid(&s, &id).ok().flatten()`. Upstream's `window_pid()`
  requires `e.adopted` (`tracker.rs:933-938`), while every Linux launcher sets
  `bootstrapper: true`, so `Entry::new` marks those entries unadopted
  (`tracker.rs:509`). The fallback restores the fork's pre-merge expression;
  Windows behavior is unchanged.
- CONFIRMED — `src-tauri/src/instances/logwatch.rs`: the upstream insertion of
  `finds_the_log_by_user_id_when_tracker_id_is_missing` landed after the fork's
  `#[cfg(windows)]` attribute, accidentally gating a platform-neutral test on
  Windows. The attribute was moved back to `finds_the_log_by_start_time`, which
  genuinely needs `std::os::windows::fs::FileTimesExt`.
- CONFIRMED — `src-tauri/src/instances/tracker.rs`: the new
  `adopts_bootstrapper_client_by_user_id_when_btid_is_missing_and_startup_is_slow`
  test depends on the Windows-only `old_log()` helper (creation time cannot be
  set on Linux), so it is now `#[cfg(windows)]`. The shared logic itself is
  compiled and exercised by the Linux-accessible logwatch test.

### Docs and SVG

- CONFIRMED — `docs/RELEASING-LINUX.md`: stale `1.3.0` / `v1.3.0.1` text now
  says upstream `1.3.11` and first Linux tag `v1.3.11.1`; `LINUX_REVISION`
  stays `1` (`src-tauri/src/self_update_linux.rs:28`).
- CONFIRMED — `docs/PROGRESS.md`: intro now covers v1.3.11; summaries for
  Previews, Logs and Multi-client mention the merged code. Coverage numbers
  and statuses were **not** changed: no Roblox/Cordial run was authorized, so
  nothing can be newly marked Ported.
- CONFIRMED — `scripts/render-port-status.py` now also renders a data-driven
  `SVG note:` line from `docs/PROGRESS.md`. `python3 scripts/render-port-status.py`
  changed exactly one SVG line (the integration note at `y=786`); all 45
  tiles, percentages, header totals and the original footer are byte-identical.
  `python3 scripts/render-port-status.py --check` now exits 0.

## Verification (re-run by orchestrator, not subagent summaries)

```text
grep -n '<<<<<<<\|=======\|>>>>>>>' <4 resolved files>   -> no output (exit 1)
git diff --name-only --diff-filter=U                     -> empty
~/.cargo/bin/cargo check --locked --manifest-path src-tauri/Cargo.toml
  -> Finished `dev` profile ... in 7m 24s
~/.cargo/bin/cargo test --locked --manifest-path src-tauri/Cargo.toml
  -> test result: ok. 478 passed; 0 failed; 5 ignored
~/.cargo/bin/cargo test ... finds_the_log_by_user_id_when_tracker_id_is_missing
  -> 1 passed; 0 failed; 0 ignored; 482 filtered out
~/.cargo/bin/cargo clippy --all-targets --locked --manifest-path src-tauri/Cargo.toml -- -D warnings
  -> Finished `dev` profile ... in 30.87s
python3 scripts/render-port-status.py --check            -> SVG is up to date
```

The first `cargo check` attempt failed only because the container lacked
`glib-2.0` (`pkg-config: Package glib-2.0 was not found`). The standard Tauri
Linux dev headers (`libglib2.0-dev`, `libwebkit2gtk-4.1-dev`,
`libayatana-appindicator3-dev`, `librsvg2-dev`) were installed, after which all
four Rust checks passed. No RoALT, Cordial or Roblox artifact was touched.

`npm run typecheck` was **not** run: `node_modules/` is absent and no frontend
source changed in this merge (only the version string in `package.json`).

## Blockers and unknowns

- UNKNOWN — no Roblox/Cordial run, so preview fallback, log rebinding and
  multi-instance behavior are compile/test-verified but not runtime-verified.
  Launching a signed-in session was not authorized.
- UNKNOWN — Windows-only compile was not checked (no Windows target installed);
  the merge carries those changes in `#[cfg(windows)]` modules unchanged.
- INFERRED — the SVG note dates the verification; future merges should update
  `SVG note:` in `docs/PROGRESS.md` and re-run the generator so it stays true.
- Next experiment: with explicit authorization, a scratch-profile Cordial run
  to confirm previews still resolve and that `[FLog::GameJoinLoadTime] userid:`
  appears in real logs; then raise the affected PROGRESS.md statuses and
  regenerate the SVG.
