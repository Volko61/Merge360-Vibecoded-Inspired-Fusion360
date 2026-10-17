#!/bin/sh
# Merge360 installer / updater for Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/Volko61/Merge360-Vibecoded-Inspired-Fusion360/main/install.sh | sh
#   wget -qO- https://raw.githubusercontent.com/Volko61/Merge360-Vibecoded-Inspired-Fusion360/main/install.sh | sh
#
# Re-running it updates Merge360 to the latest release (so does `merge360 --update`).
#
# Environment:
#   MERGE360_VERSION   install this version (e.g. 0.3.0) instead of the latest
#   MERGE360_PREFIX    install prefix (default: ~/.local, or /usr/local when run as root)
#   MERGE360_UNINSTALL=1  remove Merge360
set -eu

REPO="Volko61/Merge360-Vibecoded-Inspired-Fusion360"

if [ "$(id -u)" -eq 0 ]; then default_prefix=/usr/local; else default_prefix="$HOME/.local"; fi
PREFIX="${MERGE360_PREFIX:-$default_prefix}"
BIN="$PREFIX/bin/merge360"
DESKTOP="$PREFIX/share/applications/merge360.desktop"

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

if [ "${MERGE360_UNINSTALL:-}" = 1 ]; then
  rm -f "$BIN" "$DESKTOP"
  say "Merge360 removed from $PREFIX."
  exit 0
fi

[ "$(uname -s)" = Linux ] || die "this installer is for Linux (on Windows use: winget install Volko61.Merge360)"
case "$(uname -m)" in
  x86_64|amd64) target=x86_64-unknown-linux-gnu ;;
  aarch64|arm64) target=aarch64-unknown-linux-gnu ;;
  *) die "unsupported architecture: $(uname -m)" ;;
esac

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1"; }
  fetch_to() { curl -fsSL -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO- "$1"; }
  fetch_to() { wget -qO "$2" "$1"; }
else
  die "curl or wget is required"
fi
command -v tar >/dev/null 2>&1 || die "tar is required"
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is required"

if [ -n "${MERGE360_VERSION:-}" ]; then
  version="${MERGE360_VERSION#v}"
else
  version=$(fetch "https://api.github.com/repos/$REPO/releases/latest" \
    | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\{0,1\}\([^"]*\)".*/\1/p' | head -n 1)
  [ -n "$version" ] || die "could not determine the latest release"
fi

if [ -x "$BIN" ]; then
  current=$("$BIN" --version 2>/dev/null | awk '{print $2}' || true)
  if [ "$current" = "$version" ]; then
    say "Merge360 $version is already installed."
    exit 0
  fi
  say "Updating Merge360 ${current:-?} -> $version"
else
  say "Installing Merge360 $version"
fi

name="merge360-$version-$target"
base="https://github.com/$REPO/releases/download/v$version"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

fetch_to "$base/$name.tar.gz" "$tmp/$name.tar.gz" || die "download failed: $base/$name.tar.gz"
fetch_to "$base/SHA256SUMS.txt" "$tmp/SHA256SUMS.txt" || die "could not download checksums"
expected=$(awk -v f="$name.tar.gz" '$2 == f || $2 == "*" f {print $1}' "$tmp/SHA256SUMS.txt")
[ -n "$expected" ] || die "no checksum listed for $name.tar.gz"
actual=$(sha256sum "$tmp/$name.tar.gz" | awk '{print $1}')
[ "$expected" = "$actual" ] || die "checksum mismatch"

tar xzf "$tmp/$name.tar.gz" -C "$tmp"
mkdir -p "$PREFIX/bin" "$PREFIX/share/applications"
# Write next to the target then rename, so a running instance can be replaced.
install -m 755 "$tmp/$name/merge360" "$BIN.new"
mv -f "$BIN.new" "$BIN"

cat > "$DESKTOP" <<DESK
[Desktop Entry]
Type=Application
Name=Merge360
Comment=Parametric 3D CAD
Exec=$BIN %f
Terminal=false
Categories=Graphics;3DGraphics;Engineering;
MimeType=application/x-merge360;
DESK

say "Installed $BIN"
case ":$PATH:" in
  *":$PREFIX/bin:"*) ;;
  *) say "Note: add $PREFIX/bin to your PATH." ;;
esac
