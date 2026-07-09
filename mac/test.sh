#!/usr/bin/env bash
# Smoke tests for the Mac bundle — all standard libraries importable.
set -euo pipefail

MAC_DIR="$(cd "$(dirname "$0")" && pwd)"
NIAO="$MAC_DIR/niao"

echo "== Niao Mac smoke tests =="
"$NIAO" version
echo ""

run() {
    local label="$1"
    local file="$2"
    printf "%-40s " "$label"
    if "$NIAO" run "$file" >/dev/null 2>&1; then
        echo "OK"
    else
        echo "FAIL"
        "$NIAO" run "$file"
        exit 1
    fi
}

run "hello.niao"           "$MAC_DIR/examples/hello.niao"
run "re_demo.niao"         "$MAC_DIR/examples/re_demo.niao"
run "libs_smoke.niao"      "$MAC_DIR/examples/libs_smoke.niao"

if [[ -x "$MAC_DIR/niao_home/bin/nm" ]]; then
    echo ""
    echo "Installed libraries (nm list):"
    NIAO_HOME="$MAC_DIR/niao_home" "$MAC_DIR/niao_home/bin/nm" list --installed 2>/dev/null || true
fi

echo ""
echo "All smoke tests passed."
