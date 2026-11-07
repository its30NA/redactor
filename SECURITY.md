# Security policy

## Supported versions

| Version | Supported |
|---|---|
| 2.x (latest release) | Yes |
| 1.x (the Python command-line tool) | No. It is replaced by version 2. |

Fixes land in the latest 2.x release.

## Reporting a vulnerability

Please report security problems **privately**, through GitHub's private vulnerability reporting:

1. Open the repository's **Security** tab.
2. Choose **Report a vulnerability**.
3. Describe what you found, how to reproduce it, and which version you used.

Please do not open a public issue or a pull request for a vulnerability, and do not post secrets or real
personal data in a report. A made-up example that shows the problem is enough.

You can expect an acknowledgement and a first assessment, and a fix or a clear answer once the report is
understood. This is a small open-source project run by one person, so timing is best effort.

## What counts

Of most interest:

- a way for text sent through Redactor to leave the machine, or to be written to disk or a log;
- input that makes the engine miss a secret it is meant to catch in a documented format (a bypass);
- input that makes the engine crash, hang or use unbounded memory;
- a way for content in the window to reach the system outside the app's commands;
- a problem in the release process that could let a build differ from the published source.

Out of scope: the limits listed in the [threat model](docs/THREAT_MODEL.md) (for example, another program
running as the same user reading the window), missed secrets in formats Redactor does not claim to cover,
and the absence of code signing.

## Verifying a release

Each release has `SHA256SUMS` and build provenance attestations. See
[the README](README.md#verify-the-download).
