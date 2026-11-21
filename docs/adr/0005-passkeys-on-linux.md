# ADR-0005 — Passkeys on Linux

- **Status:** proposed
- **Date:** 2026-11-21
- **Related:** ADR-0002, ADR-0004, P4-04, P5-04

## Context

Passkeys need a platform authenticator and a WebAuthn implementation in the
engine. macOS and Windows expose one (iCloud Keychain / Windows Hello). WebKitGTK
does not expose WebAuthn today. A browser cannot invent a secure authenticator
without owning a credential store and a user-verification path.

## Decision

- macOS: use the system WebAuthn/passkey path in WKWebView.
- Windows: use the WebView2/Windows Hello path.
- Linux: **no passkey support** in the default WebKitGTK build. The site falls
  back to its password flow; the UI does not offer a broken passkey button.
- Do not implement a custom software authenticator, a browser-owned key store,
  or an external-browser handoff that pretends to be passkeys.
- Revisit only if WebKitGTK ships WebAuthn or ADR-0002 moves Linux to CEF with a
  reviewed platform integration.

## Consequences

- A Linux user who has passkey-only accounts cannot sign in with those accounts
  in searlx. The README and the capability matrix say so before installation.
- P5-04's feature matrix lists passkeys as absent on Linux.
- No secret is ever logged; the absence of a feature is not a reason to degrade
  the password path.

## Verification

- A site with passkey and password paths shows the password path on Linux.
- A passkey-only site shows an explicit unsupported message, not a silent loop.
- macOS and Windows are verified in P4-04 on real hardware.
