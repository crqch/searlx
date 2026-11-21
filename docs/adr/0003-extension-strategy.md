# ADR-0003 — Extension strategy

- **Status:** proposed
- **Date:** 2026-11-21
- **Related:** ADR-0001, ADR-0004, P4-01, P4-02, P4-03, P4-12

## Context

The original macOS app supports WebKit web extensions. The three engines
disagree:

| Engine | Extension support |
|---|---|
| WKWebView (macOS) | `WKWebExtension` controller, Safari-style extension model. |
| WebView2 (Windows) | Loads unpacked Chromium extensions; no Edge Add-ons store install path. |
| WebKitGTK (Linux) | None. There is no stable extension API. |

A full Chromium would give one extension model everywhere, but that is the
bundled engine ADR-0004 rejects. A shim that pretends to support extension APIs
would violate the "do not fake parity" rule.

## Decision

- **macOS first.** Implement `WKWebExtension` support, install/enable/remove,
  permissions, site access, and the shims listed in FEATURES.md §14.
- **Windows is bring-your-own and gated.** Load unpacked extensions only if the
  WebView2 runtime supports it and a real extension passes. No store installs,
  no promise of Chrome Web Store parity. Label it experimental.
- **Linux: none.** The capability flag is absent and the UI does not show an
  extensions surface. Revisit only if ADR-0002 moves Linux to CEF.
- **No extension shim may expose** the keychain, Apple Passwords, passkeys,
  private tabs (unless explicitly granted), settings, agent secrets, or another
  extension's pages.
- **Failures are loud.** An unsupported API returns an error to the extension;
  it does not silently resolve.

## Consequences

- Extensions remain a macOS differentiator, as in the original app.
- Windows users may get a subset; the UI must say so before installation.
- Linux users do not get extensions. The README and capability matrix say so.
- P4-12 is conditional on this ADR and on measured WebView2 behaviour.

## Evidence status

No extension has been installed in this environment. The macOS and Windows
paths are documented capabilities, not verified claims. P4-01/P4-02 must run
real extensions before this ADR becomes accepted.
