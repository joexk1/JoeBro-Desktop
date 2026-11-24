"""Pack backend/src/*.py (the Mac app's backend, verbatim) + shim/__main__.py
into backend/joebro_backend.pyz, a runnable zip the Rust app embeds
(include_bytes!) and starts with `python joebro_backend.pyz`.
Run after changing the backend:  python3 backend/build_pyz.py
"""
import pathlib
import zipfile

here = pathlib.Path(__file__).resolve().parent
out = here / "joebro_backend.pyz"
files = sorted(p for p in (here / "src").glob("*.py") if not p.name.startswith("test_"))
files.append(here / "shim" / "__main__.py")
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for f in files:
        # Fixed timestamps keep the archive (and its hash) reproducible.
        zi = zipfile.ZipInfo(f.name, (2026, 1, 1, 0, 0, 0))
        zi.compress_type = zipfile.ZIP_DEFLATED
        zi.external_attr = 0o644 << 16
        z.writestr(zi, f.read_bytes())
print(f"wrote {out} ({out.stat().st_size // 1024} KiB)")
