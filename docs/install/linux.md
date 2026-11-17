# Install Redactor on Linux

> **Preview.** The Linux app works on X11 desktops and is tested by machines, but there is no package to
> install yet. The AppImage and `.deb` packages come with the release, and Wayland support is being built.

## Which desktop are you on?

Open a terminal and type:

```bash
echo $XDG_SESSION_TYPE
```

- **x11** → the app runs here (once the packages are out, or built from source today, see below).
- **wayland** → most current Ubuntu and Fedora desktops. Not yet: the app says so when it starts. Wayland
  support is the next step.

## Today: the `scrub` command-line tool

`scrub` is released for Linux already and does the same redaction in a terminal. Install it as the
[README describes](../../README.md#install-scrub).

## Today, for developers: build the app

The app needs WebKitGTK 4.1 and, to hide in the tray, a desktop that shows tray icons. See
[CONTRIBUTING](../../CONTRIBUTING.md#build-and-test) for the build.

[Back to the overview](../../README.md#download)
