#!/usr/bin/python3
"""A stand-in for the desktop's GlobalShortcuts portal, for tools/ci/wayland-tests.sh.

Owns org.freedesktop.portal.Desktop on the script's private session bus and answers as
xdg-desktop-portal does: CreateSession and BindShortcuts return a Request path at once and send
its Response afterwards, sessions are objects with Close, and the version property is 1.

MOCK_PORTAL_BIND says how BindShortcuts ends: ok (code 0), refuse (code 1, the user said no),
never (no Response, the dialog was left open) or silent (the method call itself never replies,
a hung portal). Test-only methods on org.redactor.Mock:
  Press(s id)            sends Activated(session, id) for every open session that bound id
  SetBind(s mode)        changes how the next BindShortcuts ends
  Sessions(s owner)      the owner's open sessions and the trigger each one asked for
  CloseSessions(s owner) closes the owner's sessions as the desktop does: Closed, then gone
  Restart()              drops the name and every session and owns the name again, as a
                         restarted xdg-desktop-portal does; replies once the name is back

Started as `/usr/bin/python3 tools/ci/mock_portal.py` (a pyenv python3 has no gi). Refuses to
run unless DBUS_SESSION_BUS_ADDRESS is the script's private bus. Prints "ready" once it owns the
name. Logs no message contents.
"""

import os
import sys

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

BUS_NAME = "org.freedesktop.portal.Desktop"
PATH = "/org/freedesktop/portal/desktop"
SHORTCUTS = "org.freedesktop.portal.GlobalShortcuts"
REQUEST = "org.freedesktop.portal.Request"
SESSION = "org.freedesktop.portal.Session"
MOCK = "org.redactor.Mock"
DBUS = "org.freedesktop.DBus"
BIND_MODES = ("ok", "refuse", "never", "silent")

PORTAL_XML = """
<node>
  <interface name="org.freedesktop.portal.GlobalShortcuts">
    <method name="CreateSession">
      <arg type="a{sv}" name="options" direction="in"/>
      <arg type="o" name="handle" direction="out"/>
    </method>
    <method name="BindShortcuts">
      <arg type="o" name="session_handle" direction="in"/>
      <arg type="a(sa{sv})" name="shortcuts" direction="in"/>
      <arg type="s" name="parent_window" direction="in"/>
      <arg type="a{sv}" name="options" direction="in"/>
      <arg type="o" name="request_handle" direction="out"/>
    </method>
    <signal name="Activated">
      <arg type="o" name="session_handle"/>
      <arg type="s" name="shortcut_id"/>
      <arg type="t" name="timestamp"/>
      <arg type="a{sv}" name="options"/>
    </signal>
    <property name="version" type="u" access="read"/>
  </interface>
  <interface name="org.redactor.Mock">
    <method name="Press">
      <arg type="s" name="id" direction="in"/>
    </method>
    <method name="SetBind">
      <arg type="s" name="mode" direction="in"/>
    </method>
    <method name="Sessions">
      <arg type="s" name="owner" direction="in"/>
      <arg type="a(os)" name="sessions" direction="out"/>
    </method>
    <method name="CloseSessions">
      <arg type="s" name="owner" direction="in"/>
    </method>
    <method name="Restart"/>
  </interface>
</node>
"""

SESSION_XML = """
<node>
  <interface name="org.freedesktop.portal.Session">
    <method name="Close"/>
    <signal name="Closed">
      <arg type="a{sv}" name="details"/>
    </signal>
    <property name="version" type="u" access="read"/>
  </interface>
</node>
"""


def private_bus_or_exit():
    """Never owns the portal name on a real session bus."""
    address = os.environ.get("DBUS_SESSION_BUS_ADDRESS", "")
    runtime = os.environ.get("XDG_RUNTIME_DIR", "")
    try:
        with open(os.path.join(runtime, "redactor-test-bus"), encoding="utf-8") as f:
            mark = f.read().rstrip("\n")
    except OSError:
        mark = None
    if not os.environ.get("REDACTOR_TEST_BUS") or not address or mark != address:
        print("mock_portal: not the private bus of tools/ci/wayland-tests.sh", file=sys.stderr)
        sys.exit(2)


def bus_call(conn, method, args, reply):
    """A method call on the bus itself; its one return value."""
    return conn.call_sync(
        DBUS, "/org/freedesktop/DBus", DBUS, method, args, GLib.VariantType(reply),
        Gio.DBusCallFlags.NONE, -1, None,
    ).unpack()[0]


def own_name(conn):
    """Owns the portal name, or says why not. 4: do not queue; 1: now the primary owner."""
    if bus_call(conn, "RequestName", GLib.Variant("(su)", (BUS_NAME, 4)), "(u)") != 1:
        print("mock_portal: could not own %s" % BUS_NAME, file=sys.stderr, flush=True)
        sys.exit(1)


def sender_part(sender):
    """':1.42' -> '1_42', as the portal names request and session paths."""
    return sender.lstrip(":").replace(".", "_")


class Portal:
    def __init__(self, conn, bind_mode):
        self.conn = conn
        self.bind_mode = bind_mode
        self.counter = 0
        # BindShortcuts calls left without a reply (bind mode silent), kept so none is answered.
        self.unanswered = []
        # session path -> {"owner": unique name, "reg": registration id, "bound": {id: trigger}}
        self.sessions = {}
        self.session_iface = Gio.DBusNodeInfo.new_for_xml(SESSION_XML).interfaces[0]

    def token(self, options, key):
        value = options.get(key)
        if value:
            return value
        self.counter += 1
        return "mock_%d" % self.counter

    def respond_later(self, sender, request, code, results):
        """The Response goes out after the method's reply, as the portal sends it."""

        def send():
            self.conn.emit_signal(
                sender, request, REQUEST, "Response", GLib.Variant("(ua{sv})", (code, results))
            )
            return False

        GLib.idle_add(send)

    def on_call(self, conn, sender, path, iface, method, params, invocation):
        args = params.unpack()
        if iface == SHORTCUTS and method == "CreateSession":
            self.create_session(sender, args[0], invocation)
        elif iface == SHORTCUTS and method == "BindShortcuts":
            self.bind(sender, *args, invocation)
        elif iface == MOCK and method == "Press":
            self.press(args[0])
            invocation.return_value(None)
        elif iface == MOCK and method == "SetBind":
            if args[0] not in BIND_MODES:
                invocation.return_dbus_error(
                    "org.freedesktop.DBus.Error.InvalidArgs", "mode is ok, refuse, never or silent"
                )
                return
            self.bind_mode = args[0]
            invocation.return_value(None)
        elif iface == MOCK and method == "Sessions":
            found = [
                (p, s["bound"].get("scrub-clipboard", ""))
                for p, s in sorted(self.sessions.items())
                if s["owner"] == args[0]
            ]
            invocation.return_value(GLib.Variant("(a(os))", (found,)))
        elif iface == MOCK and method == "CloseSessions":
            for session, state in sorted(self.sessions.items()):
                if state["owner"] == args[0]:
                    # Unicast to the owner, as xdg-desktop-portal sends it.
                    self.conn.emit_signal(
                        state["owner"], session, SESSION, "Closed", GLib.Variant("(a{sv})", ({},))
                    )
                    self.drop(session)
            invocation.return_value(None)
        elif iface == MOCK and method == "Restart":
            bus_call(self.conn, "ReleaseName", GLib.Variant("(s)", (BUS_NAME,)), "(u)")
            for session in list(self.sessions):
                self.drop(session)
            own_name(self.conn)
            invocation.return_value(None)
        else:
            invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", method)

    def create_session(self, sender, options, invocation):
        request = "%s/request/%s/%s" % (PATH, sender_part(sender), self.token(options, "handle_token"))
        session = "%s/session/%s/%s" % (
            PATH,
            sender_part(sender),
            self.token(options, "session_handle_token"),
        )
        if session in self.sessions:
            invocation.return_dbus_error("org.freedesktop.portal.Error.Failed", "session exists")
            return
        reg = self.conn.register_object(session, self.session_iface, self.on_session_call, self.on_get, None)
        self.sessions[session] = {"owner": sender, "reg": reg, "bound": {}}
        invocation.return_value(GLib.Variant("(o)", (request,)))
        # A string, not an object path: the portal's documented type for this result.
        self.respond_later(sender, request, 0, {"session_handle": GLib.Variant("s", session)})

    def bind(self, sender, session, shortcuts, _parent, options, invocation):
        state = self.sessions.get(session)
        if state is None or state["owner"] != sender:
            invocation.return_dbus_error("org.freedesktop.portal.Error.InvalidArgument", "no such session")
            return
        if self.bind_mode == "silent":
            self.unanswered.append(invocation)
            return
        request = "%s/request/%s/%s" % (PATH, sender_part(sender), self.token(options, "handle_token"))
        invocation.return_value(GLib.Variant("(o)", (request,)))
        if self.bind_mode == "never":
            return
        if self.bind_mode == "refuse":
            self.respond_later(sender, request, 1, {})
            return
        bound = []
        for shortcut_id, props in shortcuts:
            trigger = props.get("preferred_trigger", "")
            state["bound"][shortcut_id] = trigger
            bound.append(
                (
                    shortcut_id,
                    {
                        "description": GLib.Variant("s", props.get("description", "")),
                        "trigger_description": GLib.Variant("s", trigger),
                    },
                )
            )
        self.respond_later(sender, request, 0, {"shortcuts": GLib.Variant("a(sa{sv})", bound)})

    def press(self, shortcut_id):
        for session, state in sorted(self.sessions.items()):
            if shortcut_id in state["bound"]:
                self.conn.emit_signal(
                    state["owner"],
                    PATH,
                    SHORTCUTS,
                    "Activated",
                    GLib.Variant(
                        "(osta{sv})", (session, shortcut_id, GLib.get_real_time() // 1000, {})
                    ),
                )

    def drop(self, session):
        """The session object is gone; nothing more is sent for it."""
        self.conn.unregister_object(self.sessions.pop(session)["reg"])

    def on_session_call(self, conn, sender, path, iface, method, params, invocation):
        state = self.sessions.get(path)
        if method != "Close" or state is None or state["owner"] != sender:
            invocation.return_dbus_error("org.freedesktop.DBus.Error.AccessDenied", "not your session")
            return
        self.drop(path)
        invocation.return_value(None)

    def on_get(self, conn, sender, path, iface, prop):
        if prop == "version":
            return GLib.Variant("u", 1)
        return None


def main():
    private_bus_or_exit()
    bind_mode = os.environ.get("MOCK_PORTAL_BIND", "ok")
    if bind_mode not in BIND_MODES:
        print("mock_portal: MOCK_PORTAL_BIND must be ok, refuse, never or silent", file=sys.stderr)
        sys.exit(2)
    conn = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    portal = Portal(conn, bind_mode)
    for iface in Gio.DBusNodeInfo.new_for_xml(PORTAL_XML).interfaces:
        conn.register_object(PATH, iface, portal.on_call, portal.on_get, None)
    # Owned by hand, not through Gio's name watcher, so Restart can drop and take it again.
    own_name(conn)
    print("ready", flush=True)
    GLib.MainLoop().run()


if __name__ == "__main__":
    main()
