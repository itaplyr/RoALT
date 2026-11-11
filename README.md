# RoALT for Linux

> [!WARNING]
> **This Linux fork is a work in progress.** Some features are missing or incomplete, some may not work
> as intended, and performance and memory use are still being improved. Expect rough edges, and keep
> backups of your accounts and vault.

## Linux port status

![RoALT Linux port status](docs/images/linux-port-status.svg)

Status is for this Linux fork, not upstream Windows RoALT. Green means working or complete; gray means
partial, untested, missing, or not applicable. The image covers accounts, launching, multi-client, headless
mode, previews, recovery, servers, FPS, Discord, limits, metrics, window controls, updates, anti-AFK,
resizing, re-authentication, self-update, interface, island, languages, Max alts, usage stats, logs,
batches, alerts, news, cookie export, sounds, settings search, vault flow, launch queue, layout preview,
macros, bandwidth and audio mute.

RoALT is a local-only Roblox account manager and multi-client launcher, running as a Tauri desktop app.
This branch is the Linux fork: it launches Roblox through **Cordial** and keeps cookies encrypted on
your machine. It talks only to Roblox, GitHub for Linux release updates, and your own Discord webhook
if you configure one.

No RoALT account, no RoALT server, no telemetry.

## What's new from upstream v1.0.0

The v1.0.0 merge brings the new shared UI and feature work into this fork. Shared frontend and backend
features arrive with the merge; anything that depends on a Windows API is called out below.

| Feature | Upstream v1.0.0 | On this Linux fork |
|---|---|---|
| Island redesign | The title-bar island holds every section and grows launch progress and running clients out of its side. | **Ported**; seen working under WebKitGTK in the v1.0.0 dev run. |
| Macros | AutoHotkey v2 scripts per client, with import/export, hotkeys and review. | **Windows-only.** AutoHotkey is not available on Linux and the merged backend reads the Windows registry. |
| Smarter batches and alerts | Strictly one-at-a-time batches, same-server picking, cancel/re-pick, richer Discord alerts. | Batch engine and Discord alerts are shared; the new batch paths still need a Linux run. |
| News | Releases, DevForum announcements and release notes, and Roblox newsroom in one tab. | **Ported**; loaded live news under WebKitGTK in the v1.0.0 dev run. |
| Cookie export | Writes `username:cookie` lines to a temporary file and deletes it after 10 minutes. | **Ported**; Linux uses `localtime_r`, atomic writes and the same cleanup timer. |
| Configurable UI sounds | Locally synthesised clicks and notifications, off by default. | Shared frontend; **untested** under WebKitGTK. |
| Audio mute on launch | Mutes each client's own audio session. | **Ported** with `pactl` on PulseAudio/PipeWire. |
| Settings search and registry | One search covers Settings, Performance, Discord and Macros; settings render from a registry. | Shared frontend; **untested** after the merge. |
| Vault flow and recovery | Clearer unlock/recovery flow, recovery-code guidance and backups. | Shared flow on top of the Linux device key; **partial** until exercised. |
| Launch queue | One launch at a time shared by Play, Home, Servers and recent servers. | Shared frontend and batch backend; **needs a Linux verification pass**. |
| Layout preview | Shows where launched windows will go before a launch. | **Not yet on Linux.** The merged command uses Windows monitor and window APIs. |
| Server browser | Browse public servers, see private servers and send accounts into one. | Already working on Linux through the existing server path. |

## Requirements

> [!NOTE]
> This fork requires Cordial for the supported Linux path.
>
> | Requirement | Notes |
> |---|---|
> | Linux desktop | Wayland or X11. Hyprland gives the best window controls (focus, hide, reveal). |
> | Cordial | Required for the supported Linux path. Gives every account its own profile and allows several clients at once. |
> | Cage | Required only for **Headless clients** (`cage` must be on `PATH`). |
> | WebKitGTK | Used for RoALT's own UI and Roblox sign-in windows. |

RoALT finds `cordial-run` on `PATH` and uses the newest build under Cordial's cache. You can point it at another loader with `ROALT_CORDIAL_RUN=/path/to/cordial-run`.


## Install

Download the installer, review it, then run it:

```bash
curl -fsSLO https://github.com/itaplyr/RoALT/releases/latest/download/install-linux.sh
less install-linux.sh
bash install-linux.sh
```

It supports Arch, Debian/Ubuntu, Fedora, openSUSE, Alpine, Void, and unknown
distributions with a supported package manager. It installs Cage and Cordial
when missing, verifies the RoALT binary's SHA-256, puts RoALT in `~/.local/bin`,
and adds a desktop entry. To upgrade later, run the same command again or use
the in-app update banner. The script needs `bash`, `curl`, and `sha256sum`
(Alpine: `sudo apk add bash` first).

Build from source instead:

```bash
git clone https://github.com/itaplyr/RoALT.git
cd RoALT
npm ci
npm run build
cargo build --release --manifest-path src-tauri/Cargo.toml
./src-tauri/target/release/roalt
```

Development build with hot reload:

```bash
npm run tauri dev
```

### Build dependencies

Arch Linux:

```bash
sudo pacman -S --needed base-devel curl wget file openssl \
  webkit2gtk-4.1 libappindicator-gtk3 librsvg patchelf
```

Debian/Ubuntu:

```bash
sudo apt install build-essential curl wget file libssl-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev patchelf
```

Also install Node.js 24, a stable Rust toolchain, Cordial, and — for headless clients — `cage`.

## Launching Roblox

RoALT starts `cordial-run` directly with a per-account profile:

```text
cordial-run --profile roalt_<user id> --join-url <roblox link>
```

This is the same direct path Cordial's own shell uses, but RoALT supplies the profile and link itself.
That avoids Cordial's profile picker and keeps each account isolated.

### Launch queue

Play, Home's tabs and cards, Servers, Recently closed and an account's recent servers all share one
launch engine. Launches run one at a time; a running batch blocks single launches too. The same queue
also carries the per-launch FPS cap and the single launch-progress view.

### Batches and same server

Batches launch accounts strictly one after another with a stagger. **Same server** either picks a
public server with room for everyone first, or sends the rest to wherever the first account landed.
Cancel at any time, then re-pick the accounts that did not launch.

### Layout preview

Upstream v1.0.0 can draw the planned window spots for a few seconds before launching. The merged Linux
command still uses Windows monitor and window APIs, so **layout preview is not available on Linux yet**.
Fit/Tiny window layouts themselves still use the Linux window-control path where a supported compositor
tool exists.

### Server browser

The Servers page browses public servers by players or ping, lists private servers and sends any chosen
accounts into the selected server. Server selection uses the account's cookie.

### FPS cap

The **Performance → FPS cap** setting is applied on Linux through Cordial. It can still be ignored by
some Cordial builds or after the client rewrites its settings, so treat it as best effort. RoALT also
passes the lighter-rendering and lowest-graphics flags through Cordial; some builds may ignore them.
The FPS precedence is:

1. the cap chosen for this launch on the Play page;
2. the account's own cap;
3. the global Performance setting;
4. `0` leaves the client alone.

For Cordial, RoALT writes a merged flags file for the launch and points `CORDIAL_FLAGS` at it. The file
contains the account profile's existing flags plus:

```json
{ "DFIntTaskSchedulerTargetFps": 60 }
```

Because `CORDIAL_FLAGS` takes precedence over the profile's own `flags.json`, a leftover
`DFIntTaskSchedulerTargetFps` in that file can no longer silently defeat RoALT's setting. Other flags in
the profile are preserved.

### Headless clients

With **Headless clients** enabled, RoALT passes `--headless` to Cordial. Cordial re-execs under cage with
`WLR_BACKENDS=headless`, clears `DISPLAY` and `WAYLAND_DISPLAY`, and renders with no window on your
desktop.

A true headless client has no host window to reveal. That means:

- its preview comes from Cordial's dev-control socket (`screenshot`), not from a window capture;
- clicking **Show** closes it and restarts it windowed;
- **Minimize** on a windowed client also switches it to headless, because the window's memory cannot be
  reclaimed in place.

Headless clients also get `CORDIAL_AUDIO=aaudio-refuse`, so FMOD opens no audio stream. Their render
resolution comes from **Performance → Headless resolution** (default `640x360`) and is passed to
Cordial as `CORDIAL_RESOLUTION`; a new value applies the next time a headless client starts.

### Window controls

RoALT finds a window by the tracked PID and its child processes, because the window may belong to a
Cordial child rather than the process RoALT started. It supports Hyprland (`hyprctl`), Sway
(`swaymsg`), and X11 through `xdotool` or `wmctrl`. KDE/GNOME Wayland sessions without one of those
tools can still launch clients and show previews, but window controls may be unavailable.

## Features

The shared RoALT feature set from upstream v1.0.0, with the Linux status called out where it differs.

| Area | What you get |
|---|---|
| Encrypted vault | Cookies are encrypted locally. On Linux the vault key derives from the machine id and your user id; set a master password for stronger protection. Encrypted `.roalt` backups and recovery codes work. |
| Accounts | Add through Roblox's sign-in page (WebKitGTK), by pasting a cookie or many at once, or by importing from Roblox Account Manager. Groups, aliases, notes, colours, favourites, ordering, presence, Robux, friends, followers, subscriptions, join date, email and 2-step status. |
| Play | Game links, place IDs, `roblox://` links, job IDs, private server links, `roblox.com/share` links, universes, profiles and `@username`. Join a friend or another account. Favourites on Play and in the tray. |
| Batches | One-at-a-time launches with a stagger, same-server picking or following the first account, cancel and re-pick. |
| Recovery | Auto-rejoin after kicks, shutdowns and lost connections; opt-in auto-restart for crashes, frozen windows and a memory limit; crash breaker and crash history. |
| Island and interface | The title-bar island holds navigation and grows launch progress and running clients. Browser-style account tabs, top and side layouts, drag sorting, themes, accent colours, background images and motion preferences. Treat the v1.0.0 island as untested on Linux. |
| Home | Live previews for every client, status, game, uptime, CPU and RAM, card sorting and compact cards, plus window and limit actions. Preview capture uses Cordial's dev-control path on Linux. |
| Limits | Per-client CPU niceness, core affinity, efficiency priority, adaptive background limits and memory throttling. Working-set trim is a no-op on Linux. |
| FPS and graphics | FPS cap, lowest graphics, lighter rendering and graphics flags through Cordial. Some builds ignore them, so treat them as best effort. |
| Server browser | Public servers by players or ping, private servers, recent servers and same-server launches. |
| Discord | Webhook alerts for crashes, disconnects, restarts and batches, per-account muting, and rich presence through the local Discord app. |
| News | GitHub releases, DevForum announcements and release notes, and Roblox's newsroom in one tab. Ported and loaded under WebKitGTK. |
| Cookie export | Writes a temporary `username:cookie` file and deletes it after ten minutes. Works on Linux through `localtime_r` and atomic file writes. |
| UI sounds | Locally synthesised click and notification sounds, off by default, with a volume control and preview. Shared frontend; untested under WebKitGTK. |
| Settings search | One search finds any setting across Settings, Performance, Discord and Macros and jumps to it. Settings render from a shared registry. |
| Macros | Upstream runs AutoHotkey v2 scripts on chosen clients, with import/export, review and hotkeys. **Windows-only:** AutoHotkey is not available on Linux. |
| Stop all | End every Roblox process and stop pending launches and rejoins from the app menu, the tray, the running-clients sheet or a panic hotkey. |
| Languages | 18 languages, including Arabic right-to-left. Some newer and Linux-only strings can still fall back to English. |
| Updates | Roblox builds are handled by Cordial. RoALT checks the Linux fork's GitHub releases and updates only its own binary after verifying the SHA-256. |

## Data and privacy

RoALT data lives in:

```text
$XDG_DATA_HOME/roalt     (usually ~/.local/share/roalt)
```

It contains the SQLCipher database, logs, temporary exports and temporary webview data. Logs for Roblox
clients are under `logs/clients/`. Set `ROALT_DATA_DIR` to use a different folder, or move the folder
from **Settings → Data**.

On Linux the vault key is derived from `/etc/machine-id` (or the DBus machine id) and your user id. This
is not as strong as Windows DPAPI: a copy of the database and machine id from the same machine can be
opened elsewhere. Set a **master password** if you want the vault protected against that.

RoALT does not implement Windows Hello on Linux. Master-password re-authentication is the available path.

RoALT talks only to `*.roblox.com` and `*.rbxcdn.com`, GitHub to look for Linux release updates, and your
own Discord webhook if configured. It sends no telemetry and has no RoALT server or account.

## What works on Linux

| Area | Status |
|---|---|
| Accounts, avatars, presence, Robux, friends, groups, aliases, notes | Works |
| Roblox sign-in through WebKitGTK, cookie import, cookie checks | Works |
| Launch one account or many, same-server batches, favourites | Partial: launching does not always open the game directly |
| Launch queue | Partial: shared queue is present; needs a Linux verification pass |
| Auto-rejoin, auto-restart, crash breaker, rejoin history | Verified: closing a client reopened it as configured |
| Server browser and private servers | Works |
| Home previews, live state, uptime, CPU, private memory, disk I/O | Works through `/proc` and Cordial devctl |
| Per-client CPU niceness, core affinity, efficiency priority | Works through the Linux scheduler |
| Adaptive limits, memory throttle, trim scheduling | Works; trim itself is a no-op on Linux |
| FPS cap and graphics flags (Cordial) | Partial: FPS is best effort; RoALT passes the render flags through `CORDIAL_FLAGS`, but some builds may ignore them |
| Headless clients under cage | Partial: no host window, but headless handling can still be improved |
| Hyprland/Sway/X11 focus, minimize, restore | Partial: tools and compositor support vary |
| Fit/Tiny window layouts | Partial: compositor-managed, with micro/tile controls where supported |
| Discord alerts and rich presence | Verified; the new v1.0.0 alert kinds need a run |
| News | Ported: live news loaded under WebKitGTK |
| Settings search and registry | Untested: shared frontend is present |
| Island redesign | Ported: seen working under WebKitGTK |
| Configurable UI sounds | Untested: shared frontend, off by default |
| Vault flow and recovery improvements | Partial: shared flow on the Linux device key |
| Encrypted `.roalt` backups, recovery codes, performance profiles | Works |
| Localization and themes | Works |

## Linux limitations

- **Anti-AFK works on Hyprland, Sway and X11 when the required input tool is available.** Other
  Wayland compositors have no equivalent path yet.
- **GPU metrics are not available.** CPU, memory and disk I/O are sampled from `/proc`.
- **Drag-resizing is compositor-managed.** The Windows low-level mouse-hook path is a no-op on Linux;
  RoALT adds micro/tile controls where supported.
- **Bandwidth/QoS limiting is unavailable.** The Linux backend rejects the apply request and the UI
  hides the Windows-only row, so no traffic policy is claimed.
- **Audio mute needs `pactl`.** It mutes the client's PipeWire/PulseAudio sink-inputs; a client with
  no audio stream yet is retried for a few seconds and then left alone.
- **Macros are not available.** AutoHotkey v2 is Windows software and the merged backend reads the
  Windows registry.
- **Layout preview is not available yet.** The merged command uses Windows monitor and window APIs.
- **Windows Hello is not available.** Use a master password for re-authentication.
- **RoALT self-update replaces only the binary.** Use the installer script or reinstall when
  Cordial, Cage or the desktop entry needs changing.
- **Roblox client updates are handled automatically by Cordial.** RoALT does not run Roblox's Windows
  installer.
- **`Take over` and the Windows singleton lock do not apply.** Cordial profiles provide multi-instance.
- **True headless clients cannot be shown without a restart.** They have no host window, and headless
  handling still has room for improvement.

## Linux versioning and releases

Linux builds use four-part tags:

```text
v0.9.2.1
v0.9.2.2
v1.0.0.1
```

The first three parts follow the upstream RoALT version; the last part is the Linux
revision. The latest Linux tag is **v0.9.2.2**, built on upstream **v0.9.2**.
Upstream has since released **v1.0.0**, which this fork has merged; the next Linux
release is expected to be **v1.0.0.1**. This keeps Linux releases independent of
upstream's release tags. See
[`docs/RELEASING-LINUX.md`](docs/RELEASING-LINUX.md) for the release steps, assets and
auto-update behavior.

## Development

Architecture, tests, and useful commands:

```bash
npm run build       # TypeScript check, i18n check, Vite build
npm run typecheck   # TypeScript only
npm run bindings    # regenerate src/bindings from Rust types
cd src-tauri
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```

### Project layout

```text
src/                         React UI
src/app/                     shell, island model, navigation, theme, menu
src/styles/                  tokens and layered CSS
src/ui/                      shared components, island and motion
src-tauri/src/main.rs        Tauri builder and managed state
src-tauri/src/commands.rs    IPC commands
src-tauri/src/launcher/      targets, URIs, bulk launches, Cordial start
src-tauri/src/instances/     process tracking, logs, limits, metrics, windows
src-tauri/src/macros.rs      AutoHotkey macro runner (Windows-only)
src-tauri/src/news.rs        News sources
src-tauri/src/cookie_export.rs  temporary cookie export
src-tauri/src/vault/         encrypted vault and Linux device key
src-tauri/src/storage/       SQLCipher database and migrations
docs/                        design notes and research
```

Linux-specific files include `client_linux.rs`, `process_linux.rs`, `window_linux.rs`, `metrics_linux.rs`,
`limits_linux.rs`, `multi_linux.rs`, `update_linux.rs`, `webview_linux.rs`, `audio_linux.rs`, `qos_linux.rs`,
`resize_linux.rs`, and `splash_linux.rs`.

## Troubleshooting

**RoALT says Roblox needs Cordial**
Install Cordial, make sure `cordial-run` is on `PATH`, and check that Cordial has downloaded a
Roblox build. If you use a custom engine, set `ROALT_CORDIAL_RUN`.

**Headless launch fails.**
Install `cage`. RoALT refuses to fall back to a visible window because that would defeat the setting.

**Show restarts the client instead of revealing it.**
That is expected for a true headless client: it has no host window. Start it without Headless if you want
a window that can be shown and hidden.

**The FPS cap seems to do nothing.**
Check that the account profile is being launched through Cordial. RoALT writes the cap into a
merged `CORDIAL_FLAGS` file; confirm the launch log mentions the flags file and the client starts after it.

**Window controls do nothing.**
They need a supported compositor and tool: Hyprland (`hyprctl`), Sway (`swaymsg`), or X11
(`xdotool`/`wmctrl`). Launching and previews still work without them.

**Macros, cookie export or layout preview do not work.**
These are Windows-only or not yet ported in the v1.0.0 merge. See the Linux limitations above.

## Credits

- [RoALT](https://github.com/inspectsoftware/RoALT) — the original Windows project by
  [inspectsoftware](https://github.com/inspectsoftware). This Linux fork is maintained at
  [itaplyr/RoALT](https://github.com/itaplyr/RoALT).
- [Cordial](https://github.com/luohoa97/cordial) — the Android Roblox runtime for Linux by
  [luohoa97](https://github.com/luohoa97), which RoALT uses to launch and manage Roblox clients.

## License

[MIT](LICENSE). The license covers the code, not the RoALT name or logo; see [NOTICE](NOTICE.md)
before you fork or reuse it.

## Disclaimer

RoALT is an independent project, not affiliated with or endorsed by Roblox Corporation. Use it only with
accounts you own. Roblox does not officially support running several clients at once and may change its
client at any time.
