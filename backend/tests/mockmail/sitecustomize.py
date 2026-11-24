"""Test-only: replaces imaplib/smtplib inside the backend subprocess with local
fakes (activated by putting this directory on PYTHONPATH). Nothing here touches
the network, and "sent" mail is only appended to $JOEBRO_TEST_MAIL_OUT."""
import imaplib
import json
import os
import smtplib

RAW = (
    b"From: Alice Example <alice@example.com>\r\n"
    b"To: me@example.com\r\n"
    b"Subject: Hello from the mock mailbox\r\n"
    b"Date: Mon, 20 Oct 2026 09:00:00 +0000\r\n"
    b"Message-ID: <m1@example.com>\r\n"
    b"Content-Type: text/plain; charset=utf-8\r\n\r\n"
    b"This is a mocked message body.\r\n"
)


class FakeIMAP(imaplib.IMAP4):
    """Subclass keeps IMAP4.error/abort attributes; __init__ never opens a socket."""

    def __init__(self, host="", port=143, timeout=None, **_kw):
        self.host = host

    def login(self, user, password):
        if password == "wrong":
            raise imaplib.IMAP4.error("LOGIN failed")
        return "OK", [b"logged in"]

    def list(self, *a):
        return "OK", [b'(\\HasNoChildren) "/" "INBOX"', b'(\\HasNoChildren) "/" "Archive"']

    def select(self, folder="INBOX", readonly=False):
        return "OK", [b"1"]

    def uid(self, cmd, *args):
        cmd = cmd.lower()
        if cmd == "search":
            return "OK", [b"1"]
        if cmd == "fetch":
            return "OK", [(b"1 (UID 1 FLAGS () RFC822.SIZE %d BODY[] {%d}" % (len(RAW), len(RAW)), RAW), b")"]
        return "OK", [None]

    def fetch(self, *a):
        return "OK", [b"1 (UID 1)"]

    def noop(self):
        return "OK", []

    def logout(self):
        return "BYE", []

    def expunge(self):
        return "OK", []

    def create(self, *a):
        return "OK", []


class FakeSMTP:
    def __init__(self, host="", port=0, timeout=None, **_kw):
        pass

    def ehlo(self):
        return 250, b"ok"

    def has_extn(self, name):
        return False

    def starttls(self, *a, **k):
        return 220, b"go"

    def login(self, user, password):
        return 235, b"ok"

    def sendmail(self, sender, recipients, text):
        out = os.environ.get("JOEBRO_TEST_MAIL_OUT")
        if out:
            with open(out, "a") as f:
                f.write(json.dumps({"from": sender, "to": list(recipients), "size": len(text), "text": text}) + "\n")
        return {}

    def quit(self):
        return 221, b"bye"


if os.environ.get("JOEBRO_TEST_MOCK_MAIL") == "1":
    imaplib.IMAP4 = FakeIMAP
    imaplib.IMAP4_SSL = FakeIMAP
    smtplib.SMTP = FakeSMTP
    smtplib.SMTP_SSL = FakeSMTP
