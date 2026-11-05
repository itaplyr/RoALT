#!/usr/bin/env bash
# RoALT Linux installer / updater.
#
# Installs the latest Linux release from itaplyr/RoALT into ~/.local/bin, adds a desktop
# entry, and installs Cage plus Cordial when they are missing. Re-run it to update.
#
# Supported families: Arch, Debian/Ubuntu, Fedora/RHEL, openSUSE, Alpine, Void, and
# unknown distributions that provide one of the supported package managers.
set -euo pipefail

REPO="itaplyr/RoALT"
ASSET="roalt-linux-x86_64"
BASE="https://github.com/${REPO}/releases/latest/download"
CORDIAL_FLATPAK="io.github.luohua97.Cordial"
FLATHUB_REPO="https://flathub.org/repo/flathub.flatpakrepo"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
APP_DIR="$DATA_DIR/applications"
ICON_DIR="$DATA_DIR/icons/hicolor/128x128/apps"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

say() { printf '\033[1mRoALT:\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mRoALT:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31mRoALT:\033[0m %s\n' "$*" >&2; exit 1; }

# ---------------------------------------------------------------- platform

if [[ "$(uname -s)" != "Linux" ]]; then
  die "This installer is for Linux only."
fi
case "$(uname -m)" in
  x86_64|amd64) ;;
  *) die "Only x86_64 Linux builds are published for now (this is $(uname -m))." ;;
esac
command -v curl >/dev/null 2>&1 || die "curl is required."
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is required."

DISTRO_ID=""
DISTRO_LIKE=""
DISTRO_NAME="unknown"
if [[ -r /etc/os-release ]]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  DISTRO_ID="${ID:-}"
  DISTRO_LIKE="${ID_LIKE:-}"
  DISTRO_NAME="${PRETTY_NAME:-${NAME:-${DISTRO_ID:-unknown}}}"
fi

# Map os-release IDs to a package-manager family. Fall back to whatever
# supported package manager is actually installed, so unknown distros still work.
detect_family() {
  local ids
  ids=" ${DISTRO_ID,,} ${DISTRO_LIKE,,} "
  case "$ids" in
    *" arch "*|*" artix "*|*" endeavouros "*|*" manjaro "*|*" garuda "*) printf 'arch\n' ;;
    *" debian "*|*" ubuntu "*|*" linuxmint "*|*" pop "*) printf 'debian\n' ;;
    *" fedora "*|*" rhel "*|*" centos "*|*" rocky "*|*" almalinux "*) printf 'fedora\n' ;;
    *suse*) printf 'suse\n' ;;
    *" alpine "*) printf 'alpine\n' ;;
    *" void "*) printf 'void\n' ;;
    *) printf 'unknown\n' ;;
  esac
}

detect_pm() {
  local pm=""
  case "$(detect_family)" in
    arch) pm="pacman" ;;
    debian) pm="apt-get" ;;
    fedora) pm="dnf" ;;
    suse) pm="zypper" ;;
    alpine) pm="apk" ;;
    void) pm="xbps-install" ;;
  esac
  if [[ -n "$pm" ]] && command -v "$pm" >/dev/null 2>&1; then
    printf '%s\n' "$pm"
    return
  fi
  for pm in pacman apt-get dnf zypper apk xbps-install; do
    if command -v "$pm" >/dev/null 2>&1; then
      printf '%s\n' "$pm"
      return
    fi
  done
  printf 'none\n'
}

PM="$(detect_pm)"

as_root() {
  if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
    "$@"
  elif command -v sudo >/dev/null 2>&1; then
    sudo "$@"
  else
    return 127
  fi
}

# Install a system package with the detected manager. Only this function uses sudo.
pkg_install() {
  local pkg="$1"
  case "$PM" in
    pacman) as_root pacman -S --needed --noconfirm "$pkg" ;;
    apt-get)
      as_root apt-get update &&
        as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y "$pkg"
      ;;
    dnf) as_root dnf install -y "$pkg" ;;
    zypper) as_root zypper --non-interactive install "$pkg" ;;
    apk) as_root apk add --no-cache "$pkg" ;;
    xbps-install) as_root xbps-install -Sy "$pkg" ;;
    *) return 1 ;;
  esac
}

pm_hint() {
  case "$PM" in
    pacman) printf 'sudo pacman -S %s' "$1" ;;
    apt-get) printf 'sudo apt-get install %s' "$1" ;;
    dnf) printf 'sudo dnf install %s' "$1" ;;
    zypper) printf 'sudo zypper install %s' "$1" ;;
    apk) printf 'sudo apk add %s' "$1" ;;
    xbps-install) printf 'sudo xbps-install -S %s' "$1" ;;
    *) printf 'your package manager' ;;
  esac
}

# ---------------------------------------------------------------- Cage

install_cage() {
  if command -v cage >/dev/null 2>&1; then
    return
  fi
  say "Installing Cage for headless clients (distro: $DISTRO_NAME)"
  if [[ "$PM" == "none" ]]; then
    warn "Cage is not installed and no supported package manager was found."
    warn "Install Cage manually if you need headless clients."
    return
  fi
  if ! pkg_install cage; then
    warn "Cage could not be installed automatically. Try: $(pm_hint cage)"
    warn "Without Cage only headless clients are affected."
  fi
}

# ---------------------------------------------------------------- Cordial

cordial_available() {
  command -v cordial-run >/dev/null 2>&1 && return 0
  command -v cordial >/dev/null 2>&1 && return 0
  if command -v flatpak >/dev/null 2>&1 && flatpak info "$CORDIAL_FLATPAK" >/dev/null 2>&1; then
    return 0
  fi
  return 1
}

ensure_flatpak() {
  command -v flatpak >/dev/null 2>&1 && return 0
  if [[ "$(detect_family)" == "arch" ]]; then
    warn "Flatpak is not installed. Install it manually to use Flatpak Cordial."
    return 1
  fi
  say "Installing Flatpak for Cordial"
  if [[ "$PM" == "none" ]] || ! pkg_install flatpak; then
    warn "Flatpak is not installed and could not be installed automatically."
    return 1
  fi
  command -v flatpak >/dev/null 2>&1
}

install_cordial_flatpak() {
  if ! ensure_flatpak; then
    return 1
  fi
  if ! flatpak remote-add --if-not-exists --user flathub "$FLATHUB_REPO" >/dev/null 2>&1; then
    warn "Could not add the Flathub remote; Flatpak Cordial may still be available."
  fi
  say "Installing Cordial from Flathub"
  flatpak install -y --user flathub "$CORDIAL_FLATPAK" >/dev/null 2>&1 ||
    flatpak install -y flathub "$CORDIAL_FLATPAK" >/dev/null 2>&1 ||
    true
  cordial_available
}

install_cordial() {
  if cordial_available; then
    return
  fi
  say "Installing Cordial (distro: $DISTRO_NAME)"
  local family helper
  family="$(detect_family)"

  if [[ "$family" == "arch" ]]; then
    helper=""
    if command -v paru >/dev/null 2>&1; then
      helper="paru"
    elif command -v yay >/dev/null 2>&1; then
      helper="yay"
    fi
    if [[ -n "$helper" ]]; then
      # Prefer the package name, then the -git variant used by older AUR entries.
      "$helper" -S --needed --noconfirm cordial ||
        "$helper" -S --needed --noconfirm cordial-git ||
        true
    fi
    # Official repositories (if the package exists there), then Flatpak.
    if ! cordial_available && pacman -Si cordial >/dev/null 2>&1; then
      as_root pacman -S --needed --noconfirm cordial ||
        warn "The official cordial package could not be installed."
    fi
  fi

  if ! cordial_available; then
    install_cordial_flatpak || true
  fi

  if ! cordial_available; then
    warn "Cordial is not installed. Install Cordial or Flatpak Cordial, then rerun this script."
    warn "Without it RoALT cannot launch Roblox."
  fi
}

install_cage
install_cordial

# ---------------------------------------------------------------- RoALT

say "Downloading the latest RoALT Linux release"
curl -fL --retry 3 --proto '=https' --proto-redir '=https' --tlsv1.2 \
  -o "$TMP/$ASSET" "$BASE/$ASSET"
curl -fL --retry 3 --proto '=https' --proto-redir '=https' --tlsv1.2 \
  -o "$TMP/$ASSET.sha256" "$BASE/$ASSET.sha256"

# Verify the SHA-256 from the release checksum before touching the install path.
line="$(head -n 1 "$TMP/$ASSET.sha256" | tr -d '\r')"
read -r -a parts <<< "$line"
expected="${parts[0]:-}"
name="${parts[1]:-}"
name="${name#\*}"
name="${name#./}"
[[ "$expected" =~ ^[0-9a-fA-F]{64}$ ]] || die "The release checksum is malformed."
[[ "${name##*/}" == "$ASSET" ]] || die "The release checksum is for '$name', expected '$ASSET'."
actual="$(sha256sum "$TMP/$ASSET" | awk '{print $1}')"
if [[ "${actual,,}" != "${expected,,}" ]]; then
  die "SHA-256 verification failed: expected $expected, got $actual."
fi
say "SHA-256 verified"

mkdir -p "$BIN_DIR" "$APP_DIR"
install -m 0755 "$TMP/$ASSET" "$BIN_DIR/roalt"

# Use a bundled icon only when this script sits next to the source tree or an
# icon was shipped alongside it. Never fetch an icon over the network.
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd || true)"
ICON_SRC=""
if [[ -n "$SCRIPT_DIR" ]]; then
  for candidate in \
    "$SCRIPT_DIR/../src-tauri/icons/128x128.png" \
    "$SCRIPT_DIR/src-tauri/icons/128x128.png" \
    "$SCRIPT_DIR/roalt.png"; do
    if [[ -f "$candidate" ]]; then
      ICON_SRC="$candidate"
      break
    fi
  done
fi
ICON_LINE=""
if [[ -n "$ICON_SRC" ]]; then
  mkdir -p "$ICON_DIR"
  install -m 0644 "$ICON_SRC" "$ICON_DIR/roalt.png"
  ICON_LINE="Icon=roalt"
fi

{
  printf '[Desktop Entry]\n'
  printf 'Type=Application\n'
  printf 'Name=RoALT\n'
  printf 'Comment=Roblox account manager and multi-client launcher\n'
  printf 'Exec=%s\n' "$BIN_DIR/roalt"
  if [[ -n "$ICON_LINE" ]]; then
    printf '%s\n' "$ICON_LINE"
  fi
  printf 'Terminal=false\n'
  printf 'Categories=Utility;Game;\n'
  printf 'StartupWMClass=roalt\n'
} > "$APP_DIR/roalt.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APP_DIR" >/dev/null 2>&1 || true
fi
if [[ -n "$ICON_SRC" ]] && command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$DATA_DIR/icons/hicolor" >/dev/null 2>&1 || true
fi

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    warn "$BIN_DIR is not in PATH."
    warn "Add it in your shell config if you want to run 'roalt' without the full path."
    warn "This installer does not edit shell rc files."
    ;;
esac

say "Installed $BIN_DIR/roalt"
say "Run it from your application launcher or with: $BIN_DIR/roalt"
