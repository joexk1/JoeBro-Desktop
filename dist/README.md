# Building and releasing JoeBro (Linux and Windows)

One codebase. The default build is the production client (first run starts the bundled
backend, or connects to a server). `--features personal` is Joe's build and is not shipped.
Releases live in `github.com/joexk1/JoeBro-Desktop`, tagged `vX.Y.Z` (version = `Cargo.toml`).

## The bundled backend
`backend/src/` is the macOS app's `Backend/` folder, verbatim; `backend/shim/__main__.py` adds
the token, parent-watch, shutdown route and Windows stubs. `backend/joebro_backend.pyz` is built
from both and embedded in the binary. Re-sync from the Mac app: `backend/sync_from_mac.sh`
(then commit the new .pyz). It needs Python 3.10+ on the user's machine; without it the app says
how to install Python and offers "Connect to a server".

## Linux
    export CARGO_TARGET_DIR=...            # optional, shared target dir
    cargo build --release
    APPIMAGETOOL=/path/to/appimagetool dist/package.sh linux   # tarball (+ AppImage if the tool is set)
Output in `dist/out/`: `joebro-X.Y.Z-linux-x86_64.tar.gz` (binary, `.desktop`, icon, `install.sh`
that installs into `~/.local`), `JoeBro-X.Y.Z-x86_64.AppImage`, `SHA256SUMS`.
appimagetool: https://github.com/AppImage/appimagetool/releases (no root needed).

## Windows
Cross-compile from Linux (needs `sudo pacman -S mingw-w64-gcc`, plus `rustup target add x86_64-pc-windows-gnu`):

    cargo build --release --target x86_64-pc-windows-gnu
    dist/package.sh windows            # JoeBro-X.Y.Z-windows-x86_64.zip
Native (CI): `cargo build --release --target x86_64-pc-windows-msvc`, then Inno Setup:
`iscc /DAppVersion=X.Y.Z dist\windows\joebro.iss` gives a per-user installer (no admin).
The .exe embeds the backend and icon. Unsigned: SmartScreen will warn until it is code-signed.

## Release (CI)
`.github/workflows/release.yml` builds Linux (ubuntu-22.04) and Windows (msvc) on a `vX.Y.Z` tag and
creates a **draft** release with the artefacts and `SHA256SUMS`. Nothing is published automatically.
Needs no secrets (uses the built-in `GITHUB_TOKEN`). For signing add your certificate as repository
secrets and a signtool step before packaging.

## After each release: re-pin the installers
The website one-liners pin the version and checksums. Update them from `SHA256SUMS`:
- `~/joebro-site/install.sh`: `desktop_version` and `linux_sha` (tarball).
- `~/joebro-site/install.ps1`: `$DesktopVersion` and `$WinSha` (zip).
They refuse to install on a checksum mismatch, so a stale pin fails safe.
