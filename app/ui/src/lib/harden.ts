/**
 * Removes browser APIs the app never uses and that reach the network outside the content
 * security policy: WebRTC is not governed by `connect-src`. Called by `harden-now.ts`, the
 * first import of main.ts, before any other module of the app runs. WebView2 also starts with
 * non-proxied WebRTC UDP disabled (app/src-tauri/src/browser_args.rs); this takes the
 * constructors away from page scripts.
 */
export const WEBRTC_GLOBALS = ['RTCPeerConnection', 'webkitRTCPeerConnection', 'RTCDataChannel'] as const;

export function removeWebRtc(target: object = globalThis): void {
  const t = target as Record<string, unknown>;
  for (const name of WEBRTC_GLOBALS) {
    if (!(name in t)) continue;
    try {
      delete t[name];
    } catch {
      // Not configurable: fall through to overwrite it below.
    }
    if (name in t) {
      try {
        Object.defineProperty(t, name, { value: undefined, writable: false, configurable: false });
      } catch {
        // Nothing more a page script can do about it.
      }
    }
  }
}
