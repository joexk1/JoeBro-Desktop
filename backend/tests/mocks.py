"""Local fakes used by the backend and client tests: an OpenAI-compatible model
server (no real API credits) and a CalDAV server. Pure stdlib."""
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ICS = """BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//mock//EN
BEGIN:VEVENT
UID:evt-mock-1
SUMMARY:Mock standup
DTSTART:20261022T090000Z
DTEND:20261022T093000Z
END:VEVENT
END:VCALENDAR
"""


class _Base(ThreadingHTTPServer):
    daemon_threads = True

    def start(self):
        threading.Thread(target=self.serve_forever, daemon=True).start()
        return self

    @property
    def port(self):
        return self.server_address[1]


class ModelHandler(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _json(self, obj, code=200):
        b = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        if self.path.rstrip("/").endswith("/models"):
            return self._json({"data": [{"id": "mock-model", "object": "model"}]})
        self._json({"error": "nope"}, 404)

    def do_POST(self):
        n = int(self.headers.get("Content-Length") or 0)
        body = json.loads(self.rfile.read(n) or b"{}")
        self.server.calls.append(body)
        if not self.path.rstrip("/").endswith("/chat/completions"):
            return self._json({"error": "nope"}, 404)
        words = ["Mock ", "reply ", "from ", "the ", "fake ", "model."]
        if body.get("stream"):
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            for w in words:
                chunk = {"choices": [{"index": 0, "delta": {"content": w}}]}
                self.wfile.write(b"data: " + json.dumps(chunk).encode() + b"\n\n")
            fin = {"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                   "usage": {"prompt_tokens": 3, "completion_tokens": 6, "total_tokens": 9}}
            self.wfile.write(b"data: " + json.dumps(fin).encode() + b"\n\ndata: [DONE]\n\n")
            return
        self._json({"choices": [{"index": 0, "message": {"role": "assistant", "content": "".join(words)},
                                 "finish_reason": "stop"}],
                    "usage": {"prompt_tokens": 3, "completion_tokens": 6, "total_tokens": 9}})


def model_server():
    s = _Base(("127.0.0.1", 0), ModelHandler)
    s.calls = []
    return s.start()


class CalDavHandler(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _send(self, code, body=b"", headers=None):
        self.send_response(code)
        for k, v in (headers or {}).items():
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _auth_ok(self):
        return self.headers.get("Authorization", "").startswith("Basic ")

    def handle_one(self):
        if not self._auth_ok():
            return self._send(401, b"auth", {"WWW-Authenticate": 'Basic realm="x"'})
        n = int(self.headers.get("Content-Length") or 0)
        data = self.rfile.read(n) if n else b""
        st = self.server.store
        m = self.command
        if m == "PROPFIND":
            return self._send(207, b'<?xml version="1.0"?><d:multistatus xmlns:d="DAV:"/>',
                              {"Content-Type": "application/xml"})
        if m == "REPORT":
            parts = []
            for href, (ics, etag) in st.items():
                parts.append(f'<d:response><d:href>{href}</d:href><d:propstat><d:prop><d:getetag>"{etag}"</d:getetag>'
                             f'<c:calendar-data>{ics}</c:calendar-data></d:prop></d:propstat></d:response>')
            xml = ('<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">'
                   + "".join(parts) + "</d:multistatus>")
            return self._send(207, xml.encode(), {"Content-Type": "application/xml"})
        if m == "PUT":
            href = self.path
            if href in st and self.headers.get("If-None-Match") == "*":
                return self._send(412)
            etag = str(len(st) + 1) + str(len(data))
            st[href] = (data.decode(), etag)
            self.server.puts.append(href)
            return self._send(201, b"", {"ETag": f'"{etag}"'})
        if m in ("GET", "HEAD"):
            if self.path in st:
                ics, etag = st[self.path]
                return self._send(200, ics.encode() if m == "GET" else b"", {"ETag": f'"{etag}"', "Content-Type": "text/calendar"})
            return self._send(404)
        if m == "DELETE":
            st.pop(self.path, None)
            self.server.deletes.append(self.path)
            return self._send(204)
        self._send(405)

    do_PROPFIND = do_REPORT = do_PUT = do_GET = do_HEAD = do_DELETE = handle_one


def caldav_server():
    s = _Base(("127.0.0.1", 0), CalDavHandler)
    s.store = {"/cal/evt-mock-1.ics": (ICS, "e1")}
    s.puts, s.deletes = [], []
    return s.start()
