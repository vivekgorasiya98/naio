#!/usr/bin/env bash
# Smoke tests for the Mac bundle — all standard libraries importable.
set -euo pipefail

MAC_DIR="$(cd "$(dirname "$0")" && pwd)"
NEKO="$MAC_DIR/neko"

echo "== Neko Mac smoke tests =="
"$NEKO" version
echo ""

run() {
    local label="$1"
    local file="$2"
    printf "%-40s " "$label"
    if "$NEKO" run "$file" >/dev/null 2>&1; then
        echo "OK"
    else
        echo "FAIL"
        "$NEKO" run "$file"
        exit 1
    fi
}

run "hello.neko"           "$MAC_DIR/examples/hello.neko"
run "re_demo.neko"         "$MAC_DIR/examples/re_demo.neko"
run "libs_smoke.neko"      "$MAC_DIR/examples/libs_smoke.neko"

if [[ -x "$MAC_DIR/neko_home/bin/nm" ]]; then
    echo ""
    echo "Installed libraries (nm list):"
    NEKO_HOME="$MAC_DIR/neko_home" "$MAC_DIR/neko_home/bin/nm" list --installed 2>/dev/null || true
fi

echo ""
echo "All smoke tests passed."
