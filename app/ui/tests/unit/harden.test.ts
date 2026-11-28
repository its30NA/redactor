import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, test } from 'vitest';
import { blockContextMenu, removeWebRtc, WEBRTC_GLOBALS } from '../../src/lib/harden';

describe('blockContextMenu', () => {
  test('cancels every contextmenu event that reaches the target', () => {
    const target = new EventTarget();
    blockContextMenu(target);
    const e = new Event('contextmenu', { cancelable: true });
    target.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(true);
    const other = new Event('click', { cancelable: true });
    target.dispatchEvent(other);
    expect(other.defaultPrevented).toBe(false);
  });
});

describe('removeWebRtc', () => {
  test('removes the WebRTC constructors, configurable or not', () => {
    class Fake {}
    const w: Record<string, unknown> = { RTCPeerConnection: Fake, webkitRTCPeerConnection: Fake, other: 1 };
    Object.defineProperty(w, 'RTCDataChannel', { value: Fake, writable: true, configurable: false });
    removeWebRtc(w);
    for (const name of WEBRTC_GLOBALS) expect(w[name], name).toBeUndefined();
    expect(w.other).toBe(1);
    // A script cannot put it back where it had to be overwritten.
    expect(() => {
      'use strict';
      w.RTCDataChannel = Fake;
    }).toThrow();
  });

  test('the start-up hardening also blocks the right-click menu', () => {
    const now = readFileSync(join(import.meta.dirname, '../../src/lib/harden-now.ts'), 'utf8');
    expect(now).toContain('blockContextMenu();');
  });

  test('runs before anything else at start-up', () => {
    const main = readFileSync(join(import.meta.dirname, '../../src/main.ts'), 'utf8');
    expect(main.split('\n')[0]).toBe("import './lib/harden-now';");
  });
});
