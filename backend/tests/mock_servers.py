"""Spawned by the Rust tests: starts local fakes, prints one JSON line with their
ports, then runs until stdin closes.

  python mock_servers.py services   -> {"model": PORT, "caldav": PORT}
  python mock_servers.py mac        -> {"mac": PORT}   (the JoeBro Mac app's backend, API shape only)
"""
import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import mocks  # noqa: E402


class MacHandler(BaseHTTPRequestHandler):
    """Routes and shapes of the Mac app's embedded backend (JoeBro-PRODUCTION):
    same core API as the server, but no /api/devices, research under
    /library|/status/<id>|/detail/<id>, permission replies at /api/permission/<id>."""

    def log_message(self, *a):
        pass

    def send_json(self, obj, code=200):
        b = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def authed(self):
        tok = self.server.token
        return not tok or self.headers.get("Authorization") == "Bearer " + tok

    def do_GET(self):
        path = self.path.split("?")[0]
        if path == "/api/health":
            return self.send_json({"ok": True, "local": True})
        if not self.authed():
            return self.send_json({"detail": "unauthorized"}, 401)
        self.server.seen.append(("GET", path))
        routes = {
            "/api/prefs": {"seeded_defaults_v1": True},
            "/api/sessions": [],
            "/api/models": {"items": [], "models": []},
            "/api/model-endpoints": [],
            "/api/email/status": {"configured": False, "display": "", "sources": [], "type": ""},
            "/api/calendar/status": {"configured": False, "display": "", "macos_authorized": False, "type": ""},
            "/api/calendar/events": {"events": []},
            "/api/research/active": {"active": []},
            "/api/research/library": {"research": [{"id": "r1", "query": "q", "status": "completed"}]},
            "/api/research/status/r1": {"status": "completed", "progress": {}},
            "/api/research/detail/r1": {"id": "r1", "query": "q", "report": "# R"},
            "/api/skills": {"skills": []},
            "/api/tasks": {"tasks": []},
        }
        if path in routes:
            return self.send_json(routes[path])
        self.send_json({"detail": "Not found"}, 404)   # incl. /api/devices

    def do_POST(self):
        n = int(self.headers.get("Content-Length") or 0)
        self.rfile.read(n)
        if not self.authed():
            return self.send_json({"detail": "unauthorized"}, 401)
        self.server.seen.append(("POST", self.path))
        if self.path.startswith("/api/permission/"):
            return self.send_json({"ok": True})
        self.send_json({"detail": "Not found"}, 404)   # incl. /api/session/<id>/append


def main():
    mode = sys.argv[1]
    if mode == "services":
        m, c = mocks.model_server(), mocks.caldav_server()
        print(json.dumps({"model": m.port, "caldav": c.port}), flush=True)
    else:
        s = ThreadingHTTPServer(("127.0.0.1", 0), MacHandler)
        s.token = sys.argv[2] if len(sys.argv) > 2 else ""
        s.seen = []
        threading.Thread(target=s.serve_forever, daemon=True).start()
        print(json.dumps({"mac": s.server_address[1]}), flush=True)
    sys.stdin.read()   # parent closes stdin -> exit


if __name__ == "__main__":
    main()
