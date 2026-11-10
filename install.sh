#!/usr/bin/env bash
set -Eeuo pipefail

REPO="ada-h00k/dual-battery-tray"
API="https://api.github.com/repos/${REPO}/releases/latest"
BIN_DIR="${HOME}/.local/bin"
CONFIG_DIR="${HOME}/.config/dual-battery-tray"
SERVICE_DIR="${HOME}/.config/systemd/user"
RULE_FILE="70-dual-battery-tray-keychron.rules"

for cmd in curl tar sha256sum install; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "Fehlendes Programm: $cmd" >&2; exit 1; }
done

command -v systemctl >/dev/null 2>&1 || { echo "systemctl wurde nicht gefunden." >&2; exit 1; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Suche das neueste Release von ${REPO} ..."
release_json="$(curl -fsSL -H 'Accept: application/vnd.github+json' "$API")"

asset_url="$(printf '%s' "$release_json" | grep -oE '"browser_download_url": "[^"]+linux-x86_64\.tar\.gz"' | head -n1 | cut -d'"' -f4)"
checksum_url="$(printf '%s' "$release_json" | grep -oE '"browser_download_url": "[^"]+linux-x86_64\.sha256"' | head -n1 | cut -d'"' -f4)"

if [[ -z "$asset_url" || -z "$checksum_url" ]]; then
  echo "Kein passendes Linux-x86_64-Release gefunden. Ist bereits ein GitHub Release veröffentlicht?" >&2
  exit 1
fi

archive="$tmp/release.tar.gz"
checksum="$tmp/release.sha256"
curl -fL --retry 3 "$asset_url" -o "$archive"
curl -fL --retry 3 "$checksum_url" -o "$checksum"

expected="$(awk 'NR==1 {print $1}' "$checksum")"
actual="$(sha256sum "$archive" | awk '{print $1}')"
[[ "$expected" == "$actual" ]] || { echo "SHA256-Prüfung fehlgeschlagen!" >&2; exit 1; }
echo "SHA256-Prüfung erfolgreich."

tar -xzf "$archive" -C "$tmp"
package_dir="$(find "$tmp" -mindepth 1 -maxdepth 1 -type d -name 'dual-battery-tray-*' | head -n1)"
[[ -n "$package_dir" ]] || { echo "Ungültiges Release-Archiv." >&2; exit 1; }
[[ -x "$package_dir/dual-battery-tray" ]] || { echo "Programmdatei fehlt im Archiv." >&2; exit 1; }

mkdir -p "$BIN_DIR" "$CONFIG_DIR" "$SERVICE_DIR"
install -m 0755 "$package_dir/dual-battery-tray" "$BIN_DIR/dual-battery-tray"
install -m 0644 "$package_dir/systemd/dual-battery-tray.service" "$SERVICE_DIR/dual-battery-tray.service"

if [[ ! -e "$CONFIG_DIR/config.toml" ]]; then
  install -m 0644 "$package_dir/config.toml.example" "$CONFIG_DIR/config.toml"
  echo "Standardkonfiguration angelegt: $CONFIG_DIR/config.toml"
else
  echo "Vorhandene Konfiguration bleibt erhalten."
fi

if [[ -f "$package_dir/udev/$RULE_FILE" ]]; then
  if command -v sudo >/dev/null 2>&1; then
    sudo install -m 0644 "$package_dir/udev/$RULE_FILE" "/etc/udev/rules.d/$RULE_FILE"
    sudo udevadm control --reload-rules
    sudo udevadm trigger
  else
    echo "Hinweis: sudo fehlt; udev-Regeln wurden nicht installiert."
  fi
fi

systemctl --user daemon-reload
systemctl --user enable --now dual-battery-tray.service

echo
echo "dual-battery-tray wurde installiert und der User-Service gestartet."
echo "Konfiguration: $CONFIG_DIR/config.toml"
echo "Status:        systemctl --user status dual-battery-tray.service"
if ! command -v headsetcontrol >/dev/null 2>&1; then
  echo "Hinweis: HeadsetControl fehlt. Für den Logitech-Headset-Akku bitte separat installieren."
fi
