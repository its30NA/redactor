#!/usr/bin/env python3
"""A minimal W3C WebDriver client for the Linux app smoke (tools/ci/linux-smoke.sh).

Standard library only. tauri-driver keeps the session between calls, so each call is one
short step and the shell script reads the outcome from the exit code and one line on stdout:

  webdriver.py session <app>                       print the session id (polls up to --timeout)
  webdriver.py title <sid>                         print the document title
  webdriver.py type <sid> <css> <text>             click the element, then send the text
  webdriver.py visible <sid> <css>                 wait until the element is displayed
  webdriver.py text <sid> <css> <prefix|exact> <s> wait until the element's text matches
  webdriver.py delete <sid>                        end the session (errors are ignored)

Every wait is a poll with a deadline (--timeout seconds, default 5); exit 0 on success, 1 when
the deadline passed (the last value seen goes to stdout), 2 on a usage or transport error.
The server is http://127.0.0.1:$SMOKE_WD_PORT (default 4444).
"""

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request

ELEMENT = "element-6066-11e4-a52e-4f735466cecf"


def base():
    return "http://127.0.0.1:%s" % os.environ.get("SMOKE_WD_PORT", "4444")


def call(method, path, body=None, timeout=30):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(base() + path, data=data, method=method)
    req.add_header("Content-Type", "application/json; charset=utf-8")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as res:
            return json.loads(res.read().decode() or "{}").get("value")
    except urllib.error.HTTPError as e:
        try:
            value = json.loads(e.read().decode()).get("value") or {}
        except ValueError:
            value = {}
        raise WebDriverError(value.get("error", "http %d" % e.code), value.get("message", ""))


class WebDriverError(Exception):
    def __init__(self, error, message):
        super().__init__("%s: %s" % (error, message))
        self.error = error


def poll(timeout, probe):
    """Calls probe() every 200 ms until it returns (True, value) or the deadline passes."""
    deadline = time.monotonic() + timeout
    last = None
    while True:
        try:
            ok, last = probe()
            if ok:
                return True, last
        except (WebDriverError, OSError) as e:
            last = str(e)
        if time.monotonic() >= deadline:
            return False, last
        time.sleep(0.2)


def find(sid, css):
    value = call("POST", "/session/%s/element" % sid, {"using": "css selector", "value": css})
    return value[ELEMENT]


def cmd_session(a):
    caps = {"capabilities": {"alwaysMatch": {"tauri:options": {"application": a.app}}}}
    # One request starts the app: tauri-driver answers once the webview is up. The loop only
    # waits for a driver that is not listening yet; any other error is final (no retries).
    deadline = time.monotonic() + a.timeout
    while True:
        left = deadline - time.monotonic()
        try:
            value = call("POST", "/session", caps, timeout=max(left, 1))
            return True, value["sessionId"]
        except TimeoutError:
            return False, "no session within %g s" % a.timeout
        except urllib.error.URLError as e:
            if not isinstance(e.reason, ConnectionRefusedError) or left <= 0:
                return False, "no session: %s" % e
        time.sleep(0.2)


def cmd_title(a):
    return True, call("GET", "/session/%s/title" % a.sid)


def cmd_type(a):
    el = find(a.sid, a.css)
    call("POST", "/session/%s/element/%s/click" % (a.sid, el), {})
    call("POST", "/session/%s/element/%s/value" % (a.sid, el), {"text": a.text})
    return True, "typed %d chars" % len(a.text)


def cmd_visible(a):
    def probe():
        el = find(a.sid, a.css)
        shown = call("GET", "/session/%s/element/%s/displayed" % (a.sid, el))
        return bool(shown), "displayed=%s" % shown

    return poll(a.timeout, probe)


def cmd_text(a):
    def probe():
        el = find(a.sid, a.css)
        text = " ".join((call("GET", "/session/%s/element/%s/text" % (a.sid, el)) or "").split())
        ok = text == a.want if a.mode == "exact" else text.startswith(a.want)
        return ok, text

    return poll(a.timeout, probe)


def cmd_delete(a):
    try:
        call("DELETE", "/session/%s" % a.sid, timeout=5)
    except (WebDriverError, OSError):
        pass
    return True, "deleted"


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--timeout", type=float, default=5.0)
    sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("session")
    s.add_argument("app")
    s.set_defaults(fn=cmd_session)
    for name, fn in (("title", cmd_title), ("delete", cmd_delete)):
        s = sub.add_parser(name)
        s.add_argument("sid")
        s.set_defaults(fn=fn)
    s = sub.add_parser("type")
    s.add_argument("sid")
    s.add_argument("css")
    s.add_argument("text")
    s.set_defaults(fn=cmd_type)
    s = sub.add_parser("visible")
    s.add_argument("sid")
    s.add_argument("css")
    s.set_defaults(fn=cmd_visible)
    s = sub.add_parser("text")
    s.add_argument("sid")
    s.add_argument("css")
    s.add_argument("mode", choices=("prefix", "exact"))
    s.add_argument("want")
    s.set_defaults(fn=cmd_text)
    a = p.parse_args()
    try:
        ok, value = a.fn(a)
    except Exception as e:  # one line for the log, never a traceback
        print("error: %s: %s" % (type(e).__name__, e))
        return 2
    print(value)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
