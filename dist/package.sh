#!/bin/bash
# Package release artefacts into dist/out/ from already-built binaries.
#   dist/package.sh linux      needs target/release/joebro
#   dist/package.sh windows    needs target/x86_64-pc-windows-gnu/release/joebro.exe
# Honours CARGO_TARGET_DIR. Writes dist/out/SHA256SUMS. APPIMAGETOOL=path enables the AppImage.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$PWD
T=${CARGO_TARGET_DIR:-$ROOT/target}
V=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
OUT=$ROOT/dist/out
mkdir -p "$OUT"

linux() {
    local name=joebro-$V-linux-x86_64 d=$OUT/stage/joebro-$V-linux-x86_64
    rm -rf "$OUT/stage"; mkdir -p "$d"
    install -m 755 "$T/release/joebro" "$d/joebro"
    strip "$d/joebro" 2>/dev/null || true
    cp dist/linux/joebro.desktop dist/linux/install.sh "$d/"
    cp assets/icon/icon_256.png "$d/joebro.png"
    printf 'JoeBro %s for Linux\n\nRun ./install.sh to install into ~/.local (no root), or run ./joebro directly.\nThe local backend needs Python 3.10+. Connecting to a server needs nothing.\n' "$V" > "$d/README.txt"
    tar -C "$OUT/stage" -czf "$OUT/$name.tar.gz" "joebro-$V-linux-x86_64"
    # AppImage (host GL/Wayland/X11 libs are used, not bundled).
    local tool=${APPIMAGETOOL:-}
    if [ -n "$tool" ] && [ -x "$tool" ]; then
        local ad=$OUT/stage/AppDir
        mkdir -p "$ad/usr/bin"
        install -m 755 "$d/joebro" "$ad/usr/bin/joebro"
        cp dist/linux/joebro.desktop "$ad/joebro.desktop"
        cp assets/icon/icon_256.png "$ad/joebro.png"
        printf '#!/bin/sh\nexec "$(dirname "$(readlink -f "$0")")/usr/bin/joebro" "$@"\n' > "$ad/AppRun"; chmod +x "$ad/AppRun"
        ARCH=x86_64 "$tool" --appimage-extract-and-run "$ad" "$OUT/JoeBro-$V-x86_64.AppImage" >/dev/null 2>&1 \
            && echo "AppImage built" || echo "AppImage build failed (skipped)"
    else
        echo "APPIMAGETOOL not set: skipping the AppImage"
    fi
    rm -rf "$OUT/stage"
}

windows() {
    local exe=$T/x86_64-pc-windows-gnu/release/joebro.exe
    [ -f "$exe" ] || exe=$T/x86_64-pc-windows-msvc/release/joebro.exe
    local d=$OUT/stage/JoeBro-$V-windows-x86_64
    rm -rf "$OUT/stage"; mkdir -p "$d"
    cp "$exe" "$d/JoeBro.exe"
    cp "$exe" "$OUT/JoeBro.exe"            # input of the Inno Setup script
    printf 'JoeBro %s for Windows\r\n\r\nDouble-click JoeBro.exe. The local backend is built in; it needs Python 3.10+ (winget install Python.Python.3.12).\r\nConnecting to a server needs nothing.\r\n' "$V" > "$d/README.txt"
    (cd "$OUT/stage" && zip -qr "$OUT/JoeBro-$V-windows-x86_64.zip" "JoeBro-$V-windows-x86_64")
    rm -rf "$OUT/stage"
}

for what in "$@"; do "$what"; done
(cd "$OUT" && rm -f SHA256SUMS && sha256sum *.tar.gz *.zip *.AppImage *setup.exe 2>/dev/null > SHA256SUMS || true; cat SHA256SUMS)
