#!/usr/bin/env bash
# One-time setup on macOS — builds from mac/engine/ into mac/niao_home/bin/
set -euo pipefail

MAC_DIR="$(cd "$(dirname "$0")" && pwd)"
ENGINE="$MAC_DIR/engine"
NIAO_HOME="$MAC_DIR/niao_home"
BIN_DIR="$NIAO_HOME/bin"

export NIAO_HOME
export PATH="$BIN_DIR:$PATH"

echo "== Niao Mac setup =="
echo "   Folder:    $MAC_DIR"
echo "   NIAO_HOME: $NIAO_HOME"
echo ""

if [[ ! -f "$ENGINE/Cargo.toml" ]]; then
    echo "Missing mac/engine/ (compiler source)."
    echo "On Windows, run:  powershell -File mac/prepare-bundle.ps1"
    exit 1
fi

if ! command -v rustc >/dev/null 2>&1; then
    echo "Rust required (one-time install):"
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo "  source \"\$HOME/.cargo/env\""
    echo "Then run:  ./setup.sh"
    exit 1
fi

if ! xcode-select -p >/dev/null 2>&1; then
    echo "Xcode Command Line Tools required:"
    echo "  xcode-select --install"
    exit 1
fi

mkdir -p "$BIN_DIR"

if [[ ! -x "$BIN_DIR/niao" ]] || [[ ! -x "$BIN_DIR/nm" ]]; then
    echo "Building Niao (first time — may take 5–10 minutes)..."
    cd "$ENGINE"
    cargo build --release --no-default-features --bin niao --bin nm
    cp "$ENGINE/target/release/niao" "$BIN_DIR/niao"
    cp "$ENGINE/target/release/nm" "$BIN_DIR/nm"
    chmod +x "$BIN_DIR/niao" "$BIN_DIR/nm"
    echo "Built: $BIN_DIR/niao"
else
    echo "Already built. Skip, or delete bin/niao and re-run setup to rebuild."
fi

echo ""
echo "Setup complete."
echo ""
echo "  ./niao version"
echo "  ./niao run examples/hello.niao"
echo "  ./test.sh"
echo ""
echo "Global commands (optional, add to ~/.zshrc):"
echo "  export NIAO_HOME=\"$NIAO_HOME\""
echo "  export PATH=\"$BIN_DIR:\$PATH\""
