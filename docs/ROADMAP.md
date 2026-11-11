# Roadmap

What is planned or being considered, without dates. Shipped changes are in the [changelog](../CHANGELOG.md).

## Desktop app on macOS and Linux (planned)

Today the desktop app runs on Windows and the `scrub` CLI runs on Windows, Linux and macOS. The plan is to
bring the app to both other platforms in one release:

- **Linux:** X11 and Wayland, including the global hotkey and the in-place clipboard scrub through the
  desktop portals. Packages: AppImage, `.deb`, `.rpm` and Flatpak.
- **macOS:** one build for Apple Silicon and Intel. It will not be signed by Apple, so the first start
  needs "Open Anyway" in System Settings; the README will show how, and every file keeps its checksum and
  build provenance attestation.
- The same protections as on Windows where the platform allows them (clipboard copies marked private by a
  password manager left alone, wipe on lock and sleep, a hardened web view). Where a platform cannot do
  something, the [threat model](THREAT_MODEL.md) will say so.

## Detection: the known misses

The shapes in [Known misses](THREAT_MODEL.md#known-misses-210) are open. Each fix has to keep the false
positive count of the [accuracy corpus](ACCURACY.md) where it is:

- password-like values under a key that is not a secret name (`PRIVATE_NOTE="..."`);
- short random tokens in prose with no key name;
- a value that starts with `#` at the start of a line;
- pointer words (`siehe`, `see`, ...) that begin a real password.

## Research: an optional local-model judge

Some cases cannot be settled by rules, such as whether `Passwort: siehe E-Mail` points somewhere or is the
password itself. Version 1 had an optional pass that asked a local model (through Ollama) about such text;
version 2 does not. Its only machine learning is a small built-in classifier for random-looking tokens.

A judge for version 2 would keep the rules of the version 1 pass:

- off by default, and it runs on your machine only;
- it can only add redactions, never remove one;
- it accepts a proposal only if the text appears verbatim in the input, so it cannot invent a redaction;
- if the model is missing or fails, the scan goes on without it.

Open question: Redactor promises no network, no server and no open port. A model inside the app keeps that
promise but makes the download much larger; a model served by a local program such as Ollama keeps the app
small but means talking to a port on your machine. Nothing is decided yet.
