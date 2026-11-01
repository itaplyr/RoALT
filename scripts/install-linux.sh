#!/usr/bin/env bash
# RoALT Linux installer / updater.
#
# Installs the latest Linux release from itaplyr/RoALT into ~/.local/bin, adds a desktop
# entry, and installs Cage plus Cordial when they are missing. Re-run it to update.
set -euo pipefail

REPO="itaplyr/RoALT"
ASSET="roalt-linux-x86_64"
BASE="https://github.com/${REPO}/releases/latest/download"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

say() { printf '\033[1mRoALT:\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mRoALT:\033[0m %s\n' "$*" >&2; }

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "This installer is for Linux only." >&2
  exit 1
fi
case "$(uname -m)" in
  x86_64|amd64) ;;
  *) echo "Only x86_64 Linux builds are published for now (this is $(uname -m))." >&2; exit 1 ;;
esac
command -v curl >/dev/null || { echo "curl is required." >&2; exit 1; }

install_cage() {
  command -v cage >/dev/null && return
  say "Installing Cage for headless clients"
  if command -v pacman >/dev/null; then
    sudo pacman -S --needed --noconfirm cage
  elif command -v apt-get >/dev/null; then
    sudo apt-get update && sudo apt-get install -y cage
  elif command -v dnf >/dev/null; then
    sudo dnf install -y cage
  elif command -v zypper >/dev/null; then
    sudo zypper install -y cage
  else
    warn "Cage is not installed and no supported package manager was found. Headless clients need it."
  fi
}

install_cordial() {
  command -v cordial-run >/dev/null && return
  command -v cordial >/dev/null && return
  if command -v flatpak >/dev/null && flatpak info io.github.luohua97.Cordial >/dev/null 2>&1; then
    return
  fi
  say "Installing Cordial"
  if command -v pacman >/dev/null; then
    if command -v paru >/dev/null; then
      paru -S --needed --noconfirm cordial-git || true
    elif command -v yay >/dev/null; then
      yay -S --needed --noconfirm cordial-git || true
    elif pacman -Si cordial >/dev/null 2>&1; then
      sudo pacman -S --needed --noconfirm cordial
    fi
  fi
  if ! command -v cordial-run >/dev/null && ! command -v cordial >/dev/null && command -v flatpak >/dev/null; then
    flatpak install -y flathub io.github.luohua97.Cordial || true
  fi
  if ! command -v cordial-run >/dev/null && ! command -v cordial >/dev/null && ! flatpak info io.github.luohua97.Cordial >/dev/null 2>&1; then
    warn "Cordial is not installed. Install Cordial or Flatpak Cordial, then rerun this script."
    warn "Without it RoALT cannot launch Roblox."
  fi
}

install_cage
install_cordial

say "Downloading the latest RoALT Linux release"
curl -fL --retry 3 --proto '=https' --tlsv1.2 -o "$TMP/$ASSET" "$BASE/$ASSET"
curl -fL --retry 3 --proto '=https' --tlsv1.2 -o "$TMP/$ASSET.sha256" "$BASE/$ASSET.sha256"
(cd "$TMP" && sha256sum -c "$ASSET.sha256")

mkdir -p "$BIN_DIR" "$APP_DIR"
install -m 0755 "$TMP/$ASSET" "$BIN_DIR/roalt"
cat > "$APP_DIR/roalt.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=RoALT
Comment=Roblox account manager and multi-client launcher
Exec=$BIN_DIR/roalt
Terminal=false
Categories=Utility;Game;
StartupWMClass=roalt
DESKTOP

if command -v update-desktop-database >/dev/null; then
  update-desktop-database "$APP_DIR" >/dev/null 2>&1 || true
fi

say "Installed $BIN_DIR/roalt"
say "Run it from your application launcher or with: $BIN_DIR/roalt"
