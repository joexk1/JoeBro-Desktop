"""Entry point of the bundled backend (`python joebro_backend.pyz ...`).

The backend itself is the macOS app's Backend/ folder, byte for byte (synced by
`backend/sync_from_mac.sh`). This shim only adds what a desktop host needs:

  --token T        bearer token the backend demands (sets JOEBRO_TOKEN); the
                   app generates one per install. Without it the backend is
                   loopback-only with no token, exactly like the Mac app.
  --parent-pid N   exit when process N is gone (the app crashed / was killed).
  POST /api/shutdown   clean stop (needs the token), used by the app on quit.
  Windows          stub `fcntl` / `os.fchmod`, which the Mac code assumes.
"""
import os
import sys
import threading
import time
import types

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)


def _pop(flag):
    """Remove `flag value` from argv and return value ('' if absent)."""
    a = sys.argv
    for i, v in enumerate(a):
        if v == flag and i + 1 < len(a):
            val = a[i + 1]
            del a[i:i + 2]
            return val
        if v.startswith(flag + "="):
            del a[i]
            return v.split("=", 1)[1]
    return ""


def _alive(pid):
    if sys.platform == "win32":
        import ctypes
        k = ctypes.windll.kernel32
        h = k.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
        if not h:
            return False
        code = ctypes.c_ulong()
        k.GetExitCodeProcess(h, ctypes.byref(code))
        k.CloseHandle(h)
        return code.value == 259  # STILL_ACTIVE
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except OSError:
        return True


def _watch_parent(pid):
    def run():
        while True:
            time.sleep(2)
            if not _alive(pid):
                os._exit(0)
    threading.Thread(target=run, daemon=True).start()


def _windows_shims():
    if sys.platform != "win32":
        return
    if "fcntl" not in sys.modules:
        m = types.ModuleType("fcntl")
        m.LOCK_EX, m.LOCK_SH, m.LOCK_UN, m.LOCK_NB = 2, 1, 8, 4
        m.flock = lambda *a, **k: None
        sys.modules["fcntl"] = m
    if not hasattr(os, "fchmod"):
        os.fchmod = lambda *a, **k: None


def _add_shutdown(mod):
    orig = mod.Handler.do_POST

    def do_POST(self):
        if self.path.split("?", 1)[0] == "/api/shutdown":
            if not self._guard():
                return
            self.json({"ok": True})
            threading.Thread(target=self.server.shutdown, daemon=True).start()
            return
        return orig(self)

    mod.Handler.do_POST = do_POST

    # /api/health also names the backend so the app can tell it from a server.
    orig_get = mod.Handler.do_GET

    def do_GET(self):
        if self.path.split("?", 1)[0] == "/api/health":
            if not self._guard():
                return
            return self.json({"ok": True, "local": True, "backend": "joebro-bundled",
                              "version": getattr(mod, "BACKEND_VERSION", "")})
        return orig_get(self)

    mod.Handler.do_GET = do_GET


def main():
    token = _pop("--token")
    parent = _pop("--parent-pid")
    if token:
        os.environ["JOEBRO_TOKEN"] = token
    _windows_shims()
    if parent.isdigit():
        _watch_parent(int(parent))
    import joebro_backend as jb
    _add_shutdown(jb)
    jb.main()


if __name__ == "__main__":
    main()
