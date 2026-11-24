#!/bin/sh
# Copy the macOS app's backend (verbatim) into backend/src and rebuild the .pyz.
# Usage: backend/sync_from_mac.sh [path/to/prod-mac]   (default ../../prod-mac)
set -e
cd "$(dirname "$0")"
MAC="${1:-../../prod-mac}/JoeBro/Backend"
[ -f "$MAC/joebro_backend.py" ] || { echo "no backend at $MAC"; exit 1; }
rm -f src/*.py
cp "$MAC"/*.py src/
rm -f src/test_*.py
cp "$MAC"/test_*.py tests/ 2>/dev/null || true
python3 build_pyz.py
