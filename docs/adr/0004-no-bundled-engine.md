# ADR-0004 — No bundled browser engine

- **Status:** accepted
- **Date:** 2026-11-21
- **Related:** ADR-0001, ADR-0002, ADR-0003

## Context

A browser can bundle its own engine (Chromium/CEF) or use the system engine.
Bundling gives one rendering stack and one capability set. It also means:

- shipping and updating a second browser;
- owning engine security response;
- a binary hundreds of megabytes larger;
- a second copy of the user's cookies and credentials;
- pressure to reimplement the extension, passkey, and DRM stacks.

The product promise is a small, quiet browser that uses the engine the OS
already maintains.

## Decision

The default build **must not** bundle Chromium, CEF, or another browser engine.
Pages run in WKWebView, WebView2, or WebKitGTK.

A separate build that bundles an engine requires:

1. a new ADR that names the product reason and the support matrix;
2. an explicit update and security-response plan;
3. a separate binary name and channel so users are not surprised;
4. a review of the no-telemetry and credential boundaries.

CEF-OSR is a candidate only for Linux if ADR-0002's GTK shell fails. Choosing it
would trigger this ADR; it is not a silent fallback.

## Consequences

- Linux capabilities are bounded by WebKitGTK (no extensions, no WebAuthn).
- Windows and macOS get the engine security updates of their OS.
- The project owns three backends and a capability matrix, not an engine.
- "Search Max" or any bundled-engine variant is out of scope until a new ADR.

## Verification

- CI inspects the dependency graph for a bundled engine crate.
- Packaging checks the bundle size and linked frameworks before release.
- The privacy page states which engine each platform uses.
