# ADR-0002 — Linux engine and shell

- **Status:** proposed
- **Date:** 2026-11-21
- **Related:** ADR-0001, ADR-0004, P0-05, P0-06, P0-09

## Context

AGENTS.md targets WebKitGTK on Linux with X11/XWayland first and leaves Wayland
unresolved. The Phase 0 spike had to answer three separate questions:

1. Does WebKitGTK render and drive a real page on this machine?
2. Can it live inside a gpui window?
3. Which display backend is usable?

The spike uses GTK 3.24.52 and WebKitGTK 2.54.2 on an AMD Ryzen AI 5 340 with
Radeon 840M graphics, Artix Linux, kernel 7.2.14. It runs the same binary twice:
once with `GDK_BACKEND=x11` on Xwayland `:0`, once with `GDK_BACKEND=wayland`
on `wayland-0`.

## Findings

| Question | Result |
|---|---|
| Real HTTPS page renders | Yes on both backends. `https://example.com/` committed and finished in 296 ms (X11) and 223 ms (Wayland). |
| JavaScript and scrolling | Yes. `window.scrollTo(0, 240)` returned `scrollY: 240` on both. |
| Overlay above the page | Yes in a `GtkOverlay`; the chip received a click on both backends. |
| WebKit process isolation | Yes. `WebKitNetworkProcess` and `WebKitWebProcess` children were present. |
| Resize tracking | Yes. `size-allocate` fired and the page reflowed. |
| Focus | Works on X11 (`view.has_focus() == true` after `grab_focus`). Fails on Wayland in this run (`false`), likely because the session is locked and the surface is not activated. Needs a manual retest. |
| Whole-window readback | Works on X11; returns black on Wayland. Page snapshots work on both. |
| gpui embedding | **Not available.** gpui 0.2.2 exposes no Linux native-view API, so the spike cannot host a WebKitGTK page inside a gpui window. |
| IME, frame rate, video, a11y | Not measured. They need an unlocked display and a person. |

## Decision

Choose **WebKitGTK in a GTK-native shell** for the first Linux build:

- GTK 3 + WebKitGTK 4.1/2.54 or newer.
- X11/XWayland is the supported backend for Phase 5. Native Wayland stays
  experimental until focus, activation, and readback are verified on an
  unlocked session.
- The Linux shell owns its chrome with GTK widgets. It does **not** use gpui.
  The `Engine` trait, command/event boundary, and core models still apply.
- CEF off-screen is not chosen for the default build. It bundles Chromium,
  triggers ADR-0004, and has not been measured here. It remains the fallback
  only if the GTK shell fails the five hard interactions on real hardware and a
  new ADR accepts the cost.
- "No Linux" remains a valid outcome if the GTK shell cannot meet the security
  and quality bar (P5-11).

This is a deliberate deviation from ADR-0001's "gpui everywhere" for Linux. It
is recorded here and in the TODO decision log. gpui remains the macOS/Windows
chrome, where native page hosting is part of its design.

## Why not keep waiting for gpui

The Linux shell is experimental, not the reference platform. Blocking all Linux
work on an upstream gpui feature would leave the engine untested. A GTK shell
lets the team exercise WebKitGTK, the engine trait, data stores, and packaging
now. If gpui later exposes a Linux native-view API, the chrome can move without
changing the engine contract.

## Consequences

- Linux needs a second chrome implementation. Keep it small and treat Tier B
  features as optional there.
- The spike's GTK binary is evidence code, not the product shell. P5-01 must
  build the real GTK shell with tabs, field, overlay, split, and IME.
- Wayland is not promised. Test it on GNOME/KDE/Sway before claiming support.
- Revisit this ADR after P0-07 (hard interactions) and P5-01 (real shell).

## Verification still required

1. Run the GTK shell on an unlocked X11 session and repeat the focus, overlay,
   resize, IME, and video checks.
2. Repeat on a native Wayland compositor with a real seat and an unlocked
   session; record whether `grab_focus` activates the view and whether window
   readback is black.
3. Measure frame rate during scroll and resize, not just event delivery.
4. Try a 10-tab and 30-tab session (P0-08) and record memory.
5. If the GTK shell cannot pass, reopen this ADR with CEF-OSR measurements.
