// Tauri isolation pattern: every IPC message from the UI passes through this sandboxed frame,
// which checks it and encrypts it before it reaches the shell. A script that sneaks into the UI
// can neither forge an encrypted message nor get one past this allowlist.
//
// The hook gets the message exactly as Tauri 2 builds it (scripts/core.js `invoke` and ipc.js,
// Tauri 2.12): { cmd, callback, error, payload, options }. It lets through only:
//   - the app's 14 commands (app/ui/src/lib/api.ts, app/src-tauri/src/ipc_names.rs), each with
//     its exact argument names, types and size limits;
//   - the plugin commands the UI really uses: event listen/unlisten for the app's events and
//     the window's resize event, and the title bar's window controls for the "main" window.
// Anything else is replaced by a call to a command that does not exist, with no arguments: the
// caller's promise rejects with Tauri's "not found" error, and nothing of the rejected message
// reaches the shell. app/ui/tests/unit/isolation.test.ts pins this against every call the UI
// makes. Which commands the shell accepts at all is still decided by its capabilities
// (app/src-tauri/capabilities/main.json).
(function () {
  'use strict';

  /** The UI never sends more than the 20 MiB input cap as text to scan (UTF-16 units). */
  const MAX_INPUT = 20 * 1024 * 1024;
  /** Sanitized output (copy, save) can be longer than its input: placeholders. */
  const MAX_OUTPUT = 64 * 1024 * 1024;
  const MAX_SHORT = 1024;
  const MAX_SECRET = 64 * 1024;
  const MAX_LIST = 10000;

  const SENSITIVITY = ['relaxed', 'balanced', 'paranoid'];
  const APP_EVENTS = ['clipboard-scrubbed', 'wipe', 'session-changed', 'settings-changed'];
  const WINDOW_EVENTS = ['tauri://resize'];
  const WINDOW = 'main';

  const isObject = (v) => typeof v === 'object' && v !== null && !Array.isArray(v);
  const isBool = (v) => typeof v === 'boolean';
  const isUint = (v) => Number.isSafeInteger(v) && v >= 0;
  const isStr = (max) => (v) => typeof v === 'string' && v.length <= max;
  const oneOf = (values) => (v) => values.includes(v);
  const arrayOf = (max, item) => (v) => Array.isArray(v) && v.length <= max && v.every(item);

  /** An object with exactly these keys (all required), each passing its check. */
  const exactly = (shape) => (v) => {
    if (!isObject(v)) return false;
    const keys = Object.keys(v);
    const want = Object.keys(shape);
    return keys.length === want.length && want.every((k) => Object.hasOwn(v, k) && shape[k](v[k]));
  };

  const none = exactly({});
  const range = exactly({ start: isUint, end: isUint });

  const scanRequest = (v) =>
    exactly({
      text: isStr(MAX_INPUT),
      sensitivity: oneOf(SENSITIVITY),
      pii: isBool,
      stripInvisible: isBool,
      keep: arrayOf(MAX_LIST, isStr(64)),
      manual: arrayOf(MAX_INPUT, range),
    })(v) && v.manual.every((r) => r.start <= r.end && r.end <= v.text.length);

  const settings = exactly({
    theme: oneOf(['system', 'dark', 'light']),
    sensitivity: oneOf(SENSITIVITY),
    pii: isBool,
    stripInvisible: isBool,
    hotkey: isStr(MAX_SHORT),
    trayMode: isBool,
    autoWipeMinutes: isUint,
    wipeOnLock: isBool,
    contentProtection: isBool,
    placeholderStyle: oneOf(['labeled', 'generic']),
    reducedMotion: oneOf(['system', 'on', 'off']),
    alwaysRedact: arrayOf(MAX_LIST, isStr(MAX_SHORT)),
    firstRunDone: isBool,
  });

  const target = (v) =>
    exactly({ kind: oneOf(['Any']) })(v) ||
    exactly({ kind: oneOf(['Window']), label: oneOf([WINDOW]) })(v);

  const windowOnly = exactly({ label: oneOf([WINDOW]) });

  /** Command name -> check of its arguments. */
  const COMMANDS = {
    scan: exactly({ req: scanRequest }),
    scrub_clipboard: none,
    read_clipboard: none,
    copy_text: exactly({ text: isStr(MAX_OUTPUT) }),
    open_text_file: none,
    save_text_file: exactly({ text: isStr(MAX_OUTPUT), suggestedName: isStr(MAX_SHORT) }),
    session_list: none,
    session_remember: (v) =>
      exactly({ text: isStr(MAX_INPUT), start: isUint, end: isUint, label: isStr(MAX_SHORT) })(v) &&
      v.start <= v.end &&
      v.end <= v.text.length,
    session_add: exactly({ secret: isStr(MAX_SECRET), label: isStr(MAX_SHORT) }),
    session_forget: exactly({ id: isStr(MAX_SHORT) }),
    session_clear: none,
    get_settings: none,
    set_settings: exactly({ settings }),
    app_info: none,
    // `onEvent` (the app's events) and `Window.onResized` (target: the main window).
    'plugin:event|listen': (v) =>
      exactly({ event: isStr(MAX_SHORT), target, handler: isUint })(v) &&
      (v.target.kind === 'Any' ? APP_EVENTS : WINDOW_EVENTS).includes(v.event),
    'plugin:event|unlisten': exactly({
      event: oneOf(APP_EVENTS.concat(WINDOW_EVENTS)),
      eventId: isUint,
    }),
    // The title bar's buttons.
    'plugin:window|minimize': windowOnly,
    'plugin:window|toggle_maximize': windowOnly,
    'plugin:window|close': windowOnly,
    'plugin:window|is_maximized': windowOnly,
    // `data-tauri-drag-region`: Tauri's drag script, no arguments.
    'plugin:window|start_dragging': none,
    'plugin:window|internal_toggle_maximize': none,
  };

  const ENVELOPE = ['cmd', 'callback', 'error', 'payload', 'options'];

  function allowed(message) {
    if (!isObject(message)) return false;
    if (!Object.keys(message).every((k) => ENVELOPE.includes(k))) return false;
    const { cmd, callback, error, payload, options } = message;
    if (typeof cmd !== 'string' || !Object.hasOwn(COMMANDS, cmd)) return false;
    if (!isUint(callback) || !isUint(error) || options !== undefined) return false;
    return COMMANDS[cmd](payload);
  }

  /** The command a rejected message turns into: unknown to the shell, so the call fails. */
  const REJECTED = 'isolation_rejected';

  function hook(message) {
    if (allowed(message)) return message;
    if (!isObject(message) || !isUint(message.callback) || !isUint(message.error)) {
      // Not a message the UI could have sent at all: drop it.
      throw new Error('IPC message rejected by the isolation allowlist');
    }
    const rejected = Object.create(null);
    rejected.cmd = REJECTED;
    rejected.callback = message.callback;
    rejected.error = message.error;
    rejected.payload = {};
    rejected.options = undefined;
    return rejected;
  }

  Object.defineProperty(window, '__TAURI_ISOLATION_HOOK__', {
    value: Object.freeze(hook),
    writable: false,
    configurable: false,
  });
})();
