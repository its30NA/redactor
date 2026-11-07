import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, test } from 'vitest';
import { removeWebRtc, WEBRTC_GLOBALS } from '../../src/lib/harden';

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

  test('runs before anything else at start-up', () => {
    const main = readFileSync(join(import.meta.dirname, '../../src/main.ts'), 'utf8');
    expect(main.split('\n')[0]).toBe("import './lib/harden-now';");
  });
});
