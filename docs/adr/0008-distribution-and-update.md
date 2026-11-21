# ADR-0008 — Distribution and update

- **Status:** proposed
- **Date:** 2026-11-21
- **Related:** P1-13, P2-18, P4-11, P5-02

## Context

The browser must install and update without an account or telemetry. Each OS has
a different signing and packaging path. An update is a code-execution boundary:
a bad signature or a downgrade is a security incident.

## Decision

- **macOS:** notarized `.app` in a DMG, Developer ID signature, hardened runtime,
  stapled ticket. Update feed with a signed appcast and a SHA-256 digest.
- **Windows:** MSIX or Squirrel package, Authenticode signature, WebView2 runtime
  check with a clear message. No silent store dependency.
- **Linux:** Flatpak is the primary channel, with an AppImage fallback. Document
  the WebKitGTK version floor. No host-library patching.
- **One signed feed per channel.** No update without a valid signature and hash.
- **No auto-relaunch.** Stage the update, verify it, install on next launch, keep
  the previous version for rollback until the new one starts.
- **No account.** The update check carries no user identifier; it may be turned
  off. A failed check never blocks the browser.

## Consequences

- Release engineering owns keys, notarization, and a rollback path.
- Linux packaging must ship or depend on a compatible WebKitGTK; a Flatpak
  runtime is preferred over the host's library.
- A downgrade is refused unless the user explicitly reinstalls.

## Verification

- A tampered package is refused before installation.
- A valid update installs on next launch and the old bundle is swept after a
  successful start.
- Offline launch works; the update check times out quietly.
