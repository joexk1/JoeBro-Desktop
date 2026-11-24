#!/bin/sh
# Install JoeBro into ~/.local (no root): binary, launcher entry, icon.
# Run from the unpacked tarball:  ./install.sh   (uninstall:  ./install.sh --uninstall)
set -eu
here=$(cd "$(dirname "$0")" && pwd)
prefix="${PREFIX:-$HOME/.local}"
bin="$prefix/bin/joebro"
desk="$prefix/share/applications/joebro.desktop"
icon="$prefix/share/icons/hicolor/256x256/apps/joebro.png"
if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$bin" "$desk" "$icon"
    echo "JoeBro removed. Your data in ~/.config/joebro and ~/.local/share/joebro is untouched."
    exit 0
fi
mkdir -p "$prefix/bin" "$prefix/share/applications" "$prefix/share/icons/hicolor/256x256/apps"
install -m 755 "$here/joebro" "$bin"
install -m 644 "$here/joebro.png" "$icon"
sed "s|^Exec=.*|Exec=$bin|" "$here/joebro.desktop" > "$desk"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$prefix/share/applications" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q "$prefix/share/icons/hicolor" >/dev/null 2>&1 || true
echo "Installed JoeBro to $bin"
case ":$PATH:" in *":$prefix/bin:"*) ;; *) echo "Add $prefix/bin to your PATH to start it with: joebro" ;; esac
command -v python3 >/dev/null 2>&1 || echo "JoeBro's local backend needs Python 3.10+ (sudo apt install python3 / dnf install python3 / pacman -S python)."
