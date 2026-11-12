#!/usr/bin/env python3
"""P5 helper: ask the GlobalShortcuts portal for one shortcut, as an unsandboxed app does.

Prints one line of key=value facts: the portal version, whether the host registry took an
app id, whether a session was created, and the BindShortcuts answer (0 = granted, 1 = the
user cancelled, 2 = refused, timeout = no answer: a dialog is waiting for a click).
"""

import sys

from gi.repository import Gio, GLib

DEST = "org.freedesktop.portal.Desktop"
PATH = "/org/freedesktop/portal/desktop"
IFACE = "org.freedesktop.portal.GlobalShortcuts"
ANSWER_WAIT_S = 15

bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
sender = bus.get_unique_name()[1:].replace(".", "_")
facts = []


def done():
    print(" ".join(facts))
    sys.exit(0)


def call(iface, method, params, reply):
    reply_type = GLib.VariantType.new(reply) if reply else None
    return bus.call_sync(DEST, PATH, iface, method, params, reply_type, Gio.DBusCallFlags.NONE, 30000, None)


def request(method, params, token):
    """Call a portal method and wait for its Request.Response; {} when none came."""
    path = f"/org/freedesktop/portal/desktop/request/{sender}/{token}"
    loop = GLib.MainLoop()
    got = {}

    def on_response(_conn, _sender, _path, _iface, _signal, answer):
        got["code"], got["results"] = answer.unpack()
        loop.quit()

    sub = bus.signal_subscribe(
        None, "org.freedesktop.portal.Request", "Response", path, None, Gio.DBusSignalFlags.NONE, on_response
    )
    try:
        call(IFACE, method, params, "(o)")
        GLib.timeout_add_seconds(ANSWER_WAIT_S, loop.quit)
        loop.run()
    finally:
        bus.signal_unsubscribe(sub)
    return got


# The host registry must come first on the connection (xdg-desktop-portal 1.19+).
try:
    call(
        "org.freedesktop.host.portal.Registry",
        "Register",
        GLib.Variant("(sa{sv})", ("io.github.redactor.Probe", {})),
        None,
    )
    facts.append("register=ok")
except GLib.Error as e:
    facts.append(f"register=error({e.message})")

try:
    version = call("org.freedesktop.DBus.Properties", "Get", GLib.Variant("(ss)", (IFACE, "version")), "(v)")
    facts.insert(0, f"version={version.unpack()[0]}")
except GLib.Error as e:
    facts.insert(0, f"version=none({e.message})")
    done()

try:
    created = request(
        "CreateSession",
        GLib.Variant(
            "(a{sv})",
            ({"handle_token": GLib.Variant("s", "probe1"), "session_handle_token": GLib.Variant("s", "probe")},),
        ),
        "probe1",
    )
except GLib.Error as e:
    facts.append(f"session=error({e.message})")
    done()
if not created:
    facts.append("session=timeout")
    done()
if created["code"] != 0:
    facts.append(f"session=code{created['code']}")
    done()
facts.append("session=ok")
session = created["results"]["session_handle"]

shortcuts = [
    (
        "redact",
        {
            "description": GLib.Variant("s", "Redactor probe"),
            "preferred_trigger": GLib.Variant("s", "CTRL+ALT+r"),
        },
    )
]
try:
    bound = request(
        "BindShortcuts",
        GLib.Variant(
            "(oa(sa{sv})sa{sv})",
            (session, shortcuts, "", {"handle_token": GLib.Variant("s", "probe2")}),
        ),
        "probe2",
    )
except GLib.Error as e:
    facts.append(f"bind=error({e.message})")
    done()
if not bound:
    facts.append("bind=timeout")
    done()
facts.append(f"bind={bound['code']}")
triggers = [opts.get("trigger_description", "?") for _id, opts in bound["results"].get("shortcuts", [])]
facts.append("triggers=" + (",".join(triggers) or "none"))
done()
