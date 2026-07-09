#!/usr/bin/env bash
# Niao {{VERSION}} installer — {{LABEL}}
# Usage: bash niao-{{VERSION}}-{{PLATFORM}}-install.sh
# Installs niao + nm to ~/.niao/bin and updates your shell PATH.

set -euo pipefail

VERSION="{{VERSION}}"
PLATFORM="{{PLATFORM}}"
ARCHIVE="{{ARCHIVE_URL}}"
INSTALL_ROOT="${NIAO_INSTALL_DIR:-$HOME/.niao}"
BIN_DIR="$INSTALL_ROOT/bin"
TMP="${TMPDIR:-/tmp}/niao-install-$$"

echo ""
echo "Niao $VERSION Setup — {{LABEL}}"
echo "================================"
echo ""

mkdir -p "$TMP" "$BIN_DIR"

echo "Downloading..."
if command -v curl >/dev/null 2>&1; then
  curl -fsSL "$ARCHIVE" -o "$TMP/niao-bundle.tar.gz"
elif command -v wget >/dev/null 2>&1; then
  wget -qO "$TMP/niao-bundle.tar.gz" "$ARCHIVE"
else
  echo "Error: curl or wget required" >&2
  exit 1
fi

echo "Extracting to $INSTALL_ROOT..."
tar -xzf "$TMP/niao-bundle.tar.gz" -C "$TMP"
# Bundle layout: niao/bin/... or flat bin/... at archive root
if [ -d "$TMP/niao/bin" ]; then
  BUNDLE="$TMP/niao"
elif [ -d "$TMP/bin" ]; then
  BUNDLE="$TMP"
else
  echo "Error: unexpected archive layout" >&2
  exit 1
fi

if [ -f "$BUNDLE/bin/niao" ] || [ -f "$BUNDLE/bin/niao.exe" ]; then
  cp -f "$BUNDLE/bin/niao" "$BIN_DIR/niao" 2>/dev/null || cp -f "$BUNDLE/bin/niao.exe" "$BIN_DIR/niao"
  cp -f "$BUNDLE/bin/nm" "$BIN_DIR/nm" 2>/dev/null || cp -f "$BUNDLE/bin/nm.exe" "$BIN_DIR/nm"
  chmod +x "$BIN_DIR/niao" "$BIN_DIR/nm" 2>/dev/null || true
  if [ -d "$BUNDLE/niao_libs" ]; then
    rm -rf "$INSTALL_ROOT/niao_libs"
    cp -R "$BUNDLE/niao_libs" "$INSTALL_ROOT/"
  fi
else
  echo "Error: niao/nm binaries not found in archive" >&2
  exit 1
fi

rm -rf "$TMP"

# PATH — add to profile if missing
path_line="export PATH=\"$BIN_DIR:\$PATH\""
updated=0
for profile in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.profile"; do
  if [ -f "$profile" ] && ! grep -qF "$BIN_DIR" "$profile" 2>/dev/null; then
    echo "" >> "$profile"
    echo "# Niao $VERSION" >> "$profile"
    echo "$path_line" >> "$profile"
    echo "  Updated $profile"
    updated=1
  fi
done

export PATH="$BIN_DIR:$PATH"

echo ""
echo "Installed to: $INSTALL_ROOT"
echo "  niao: $BIN_DIR/niao"
echo "  nm:   $BIN_DIR/nm"
if [ "$updated" = 0 ]; then
  echo ""
  echo "Add to PATH manually:"
  echo "  $path_line"
fi

if [ -x "$BIN_DIR/niao" ]; then
  echo ""
  echo "Version: $($BIN_DIR/niao version 2>/dev/null || echo unknown)"
fi

echo ""
echo "Open a new terminal, then run:"
echo "  niao version"
echo "  nm install nllm    # optional libraries via nm"
echo ""
echo "Done."
