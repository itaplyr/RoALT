# Releasing the Linux fork

The Linux fork uses its own release number after the upstream version:

```text
v<upstream version>.<linux revision>
```

Examples:

```text
v0.9.2.1
v0.9.2.2
v1.3.0.1
```

- `0.9.2` / `1.3.0` is the upstream RoALT version.
- `.1`, `.2`, ... are Linux-fork releases for that upstream version.
- The latest Linux tag in this repository is `v1.3.11.1` (upstream `1.3.11`).
  Upstream `v1.3.11` is merged; `v1.3.11.1` is the first Linux release for it.
- Four-part tags never collide with upstream's three-part tags.

The current tree is upstream version `1.3.11` with `LINUX_REVISION = 1`, so the
release is `v1.3.11.1`.

The app version shown in Settings is computed from `Cargo.toml` plus
`LINUX_REVISION` in `src-tauri/src/self_update_linux.rs`. `Cargo.toml`,
`package.json` and `tauri.conf.json` keep the normal three-part upstream version.

## Making a Linux release

1. Merge the upstream release and resolve the Linux build.
2. Set `LINUX_REVISION = 1` for the first Linux release of that upstream version.
3. For later Linux-only fixes, keep the upstream version and increment
   `LINUX_REVISION` only.
4. Run the checks:

   ```bash
   npm run build
   cd src-tauri
   cargo test --locked
   ```

5. Commit the changes.
6. Tag and push:

   ```bash
   git tag v1.3.0.1
   git push fork v1.3.0.1
   ```

The **Release Linux** workflow builds the binary and creates a draft release with:

```text
roalt-linux-x86_64
roalt-linux-x86_64.sha256
install-linux.sh
SHA256SUMS.txt
```

Publish the draft when it looks right. The Windows release workflow ignores
four-part Linux tags.

## Release notes

- Mocktail is experimental. Upstream Mocktail is single-instance by policy;
  RoALT's Multi-instance setting copies the launcher per account and isolates
  auth, cache, state and Android data. Describe that as an experimental path,
  never as a drop-in replacement and never as verified: no signed-in Mocktail
  launch has been tested. Say that plainly.
- Cordial remains the tested multi-instance launcher, so multi-client claims
  belong to Cordial. Do not describe Mocktail as the multi-instance default.
- Clips, Trades, Outfits, owning or buying private servers and the Friend
  requests view were removed upstream in v1.3.0. Do not list them as missing
  Linux features; joining private servers by link and the Servers › Private tab
  remain.

## Auto-update

RoALT checks the Linux fork's latest published GitHub release:

```text
https://api.github.com/repos/itaplyr/RoALT/releases/latest
```

- at startup
- every five minutes
- when the setting **Check for RoALT updates** is on

If a newer four-part tag exists with the Linux binary and checksum, the update
banner appears. **Update now** downloads both files, verifies the SHA-256,
replaces the running executable and restarts RoALT.

The updater only replaces the binary. It does not install Cordial or Cage. Use
`install-linux.sh` for the initial machine setup or to reinstall.

## Installer

`scripts/install-linux.sh`:

- checks Linux and x86_64
- detects Arch, Debian/Ubuntu, Fedora/RHEL, openSUSE, Alpine, Void, and unknown
  distributions that provide a supported package manager
- installs Cage if missing, through the matching package manager
  (`pacman`, `apt-get`, `dnf`, `zypper`, `apk`, or `xbps-install`)
- installs Cordial if missing: paru/yay, the official package, or Flatpak on
  Arch; Flatpak (installed first when missing) on other distributions
- does not install Mocktail; the experimental backend is a separate, opt-in
  install that the script leaves alone
- downloads the latest `roalt-linux-x86_64` and its SHA-256, then verifies the
  binary before installing it
- installs it to `${XDG_BIN_HOME:-$HOME/.local/bin}/roalt`
- writes `~/.local/share/applications/roalt.desktop`
- installs an icon only when one is already bundled with the release or source
  tree; it never downloads an icon
- warns when `~/.local/bin` is not on `PATH` instead of editing shell rc files
- refreshes the desktop database and icon cache when available

The script requires `bash`, `curl`, and `sha256sum`. Alpine does not ship `bash`
by default; install it with `sudo apk add bash` first. Package-manager installs
use `sudo` only when they are actually needed; run the script as a normal user.

Run it after downloading and reviewing it:

```bash
curl -fsSLO https://github.com/itaplyr/RoALT/releases/latest/download/install-linux.sh
less install-linux.sh
bash install-linux.sh
```

Unknown distributions are not refused: the script falls back to whichever
supported package manager is installed, and still installs RoALT when Cage or
Cordial cannot be installed automatically.
