# ADR-0001 — gpui chrome with system web engines

- **Status:** accepted for macOS and Windows; provisional for Linux
- **Date:** 2026-11-21
- **Supersedes:** none
- **Related:** ADR-0002, ADR-0004

## Context

The browser needs a small, quiet chrome that can host arbitrary web pages. Two
families of design are available:

1. Bundle one engine and render pages inside the same toolkit everywhere.
2. Use the operating system's web engine per OS and build only the chrome.

Bundling Chromium gives identical pages and one capability set, but it adds a
second browser, a large binary, its own update channel, and a security surface
the project would own. The product promise is a quiet browser that uses what
the OS already maintains.

gpui is the chosen chrome toolkit. It is pre-1.0, but it gives the chrome one
Rust codebase and a drawing model that can place a native page surface between
the base and overlay scenes on macOS.

## Decision

- Use **gpui** for the chrome on macOS and Windows.
- Use **WKWebView** on macOS, **WebView2** on Windows, and **WebKitGTK** on
  Linux behind the `Engine` trait.
- Do not bundle Chromium in the default build (ADR-0004).
- Capability differences are explicit flags. The UI hides or disables a feature
  it cannot provide; it never silently does nothing.
- Linux is provisional because gpui 0.2.2 has no Linux native-view API. See
  ADR-0002.

## Consequences

- The binary stays small; engine security updates arrive with the OS.
- Three engines mean three capability sets and three test matrices. Contract
  tests and the capability matrix are mandatory, not optional.
- Page rendering, input, IME, accessibility, and process isolation are the
  engine's responsibility. The chrome must not assume web content is safe.
- gpui may change under us. Pin the version, isolate UI code in `searlx-ui`,
  and keep the engine boundary free of gpui types.

## Evidence

- The workspace builds and the gpui shell launches on Linux (P0-01/P0-02).
- The engine trait, fake engine, and contract test exist and pass.
- macOS and Windows embedding were **not** verified in this environment; P0-03
  and P0-04 remain blocked with manual steps.
- gpui 0.2.2's `Window::paint_surface` is macOS-only. The Linux spike therefore
  measures a GTK shell baseline, not a gpui-hosted page (P0-05/P0-06).
