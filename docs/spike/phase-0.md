# Phase 0 — feasibility spike report

**Date:** 2026-11-21
**Status:** partial. Evidence exists for Linux; macOS and Windows are not
verified. P0-G is **blocked**, not passed.
**Related:** [capabilities.md](capabilities.md),
[security-checklist.md](security-checklist.md), ADR-0002, ADR-0003.

## 1. Question

Can searlx host a real page inside its own chrome, keep the page alive, and put
its own controls above it — on macOS, Windows, and Linux? If Linux cannot do it
with WebKitGTK, should it use CEF off-screen or a GTK-native shell?

Phase 0 is timeboxed to 2–4 weeks and is not a browser build. This report
records what was actually measured, what was not, and what that means for
Phase 1.

## 2. Environment

| Item | Value |
|---|---|
| Machine | AMD Ryzen AI 5 340, 6 cores / 12 threads, Radeon 840M, 30 GiB RAM |
| OS | Artix Linux, kernel 7.2.14-artix1-1 |
| Session | Wayland (`XDG_SESSION_TYPE=wayland`), Xwayland `:0`, screen locked by `swaylock` |
| GTK | 3.24.52 |
| WebKitGTK | 2.54.2 (`webkit2gtk-4.1`) |
| Rust | 1.99.1, edition 2024 |
| gpui | 0.2.2 (no Linux native-view API; `Window::paint_surface` is macOS-only) |

The lock matters. A locked compositor can still render a GTK window into an
X11 pixmap, but it cannot prove what a person sees, and it blocks real pointer,
keyboard, and IME tests.

## 3. What was built

- `crates/searlx-engine-linux/src/bin/spike.rs`: a GTK 3 + WebKitGTK binary that
  loads a URL, logs load events, runs JavaScript, scrolls, snapshots the page,
  snapshots the host window on X11, lists WebKit child processes, reads
  `/proc/self/status`, and exits after a timeout.
- A `GtkOverlay` with a chip above the page. The chip is the stand-in for a
  browser overlay.
- `docs/spike/evidence/`: page snapshots and the X11 window snapshot.
- The workspace itself: `searlx-core`, `searlx-engine` (trait + fake),
  `searlx-platform` (traits + fakes), `searlx-ui` (gpui shell), `searlx-testkit`
  (contract test), and `searlx-engine-linux` (stub + spike).

The spike does **not** embed WebKitGTK in a gpui window. gpui 0.2.2 has no
Linux native-view API, so that experiment cannot be run as specified. The
spike measures the GTK-shell baseline that ADR-0002 compares against.

## 4. Reproduce

```sh
cargo build -p searlx-engine-linux --bin searlx-spike-linux

# X11 / Xwayland
GDK_BACKEND=x11 DISPLAY=:0 XDG_RUNTIME_DIR=/tmp/run-1000 \
  ./target/debug/searlx-spike-linux --seconds 30 \
  --page-png /tmp/spike-x11-page.png \
  --window-png /tmp/spike-x11-window.png

# Native Wayland
GDK_BACKEND=wayland WAYLAND_DISPLAY=wayland-0 XDG_RUNTIME_DIR=/tmp/run-1000 \
  ./target/debug/searlx-spike-linux --seconds 30 \
  --page-png /tmp/spike-wl-page.png \
  --window-png /tmp/spike-wl-window.png
```

The binary prints `SPIKE ...` lines. Exit code 0 means it completed its script,
not that every interaction passed.

## 5. Results

Both runs loaded `https://example.com/`.

| Measurement | X11 (`GDK_BACKEND=x11`) | Wayland (`GDK_BACKEND=wayland`) |
|---|---|---|
| Display type | `GdkX11Display` | `GdkWaylandDisplay` |
| Load committed | yes | yes |
| Load finished | 296 ms after start | 223 ms after start |
| Title | `Example Domain` | `Example Domain` |
| JavaScript scroll | `scrollY: 240`, body height 4827 | `scrollY: 240`, body height 4704 |
| Focus after `grab_focus` | `view=true window=true` | `view=false window=false` |
| Overlay chip click handler | fired (programmatic) | fired (programmatic) |
| Overlay visible in window image | yes | not readable (black) |
| Page snapshot | 74,093 bytes, 750×981 | 144,387 bytes, 750×489 |
| Window snapshot | 67 KiB PNG, 750×981 | 1.5 KiB black image, 750×489 |
| Process memory before snapshot | 162,736 kB VmRSS | 185,100 kB VmRSS |
| Process memory after snapshot | 185,548 kB VmRSS | 195,224 kB VmRSS |
| Child processes | `WebKitNetworkPr`, `WebKitWebProces` | `WebKitNetworkPr`, `WebKitWebProces` |
| Resize request | requested 900×620; logged allocations stayed 750×981 | same |

The load time is **not** cold start. It is the interval from the engine's
`Started` signal to `Finished`; process spawn and GTK startup are outside it.
One page, one run, no statistical treatment.

### Evidence

- [X11 page snapshot](evidence/x11-page.png)
- [X11 window snapshot](evidence/x11-window.png) — chrome, page, and the overlay
  chip are all visible.
- [Wayland page snapshot](evidence/wayland-page.png)

## 6. What worked

- WebKitGTK renders a real HTTPS page on both X11/Xwayland and native Wayland.
- JavaScript executes and a programmatic scroll changes the page.
- A `GtkOverlay` child draws above the page. The X11 window snapshot shows it.
- X11 gives the web view keyboard focus after `grab_focus`.
- WebKit spawns its network and web processes; the app is not a single-process
  monolith.
- Page snapshots work on both backends, independent of what the compositor
  shows on screen.
- Resize events are delivered (`size-allocate` logged repeatedly).

## 7. What did not work, or was not measured

| Item | Status |
|---|---|
| WebKitGTK inside gpui | Impossible with gpui 0.2.2; no Linux native-view API. |
| Real pointer click on the overlay | Not tested. The handler was invoked programmatically. |
| Real keyboard input / IME / emoji | Not tested. Locked session. |
| Frame rate during scroll or resize | Not measured. Event delivery is not a frame rate. |
| Resize actually changing the window | Requested 900×620, but the logged allocation stayed 750×981. Unverified under this window manager. |
| Wayland focus/activation | Failed in this run; likely the locked surface. Retest unlocked. |
| Whole-window readback on Wayland | Black. Page snapshot still works. |
| Video, fullscreen, PiP | Not tested. No video page, no unlocked display. |
| Accessibility tree | Not tested. |
| macOS WKWebView embed | No macOS machine. P0-03 blocked. |
| Windows WebView2 embed | No Windows machine. P0-04 blocked. |
| 1 / 10 / 30 tab memory | No benchmark yet. P0-08 open. |

## 8. Capability conclusions

The detailed matrix is in [capabilities.md](capabilities.md). The short version:

- macOS has the strongest match to the original app: WebKit, content blockers,
  WKWebExtension, system passkeys.
- Windows can do the Tier A engine work, but extensions are bring-your-own and
  a WebView2 child window may cover gpui overlays. P0-04 must measure that.
- Linux can render and script a page, but cannot do extensions or passkeys, and
  cannot host the page in gpui today.

## 9. Recommendation

### Linux engine

Use **WebKitGTK in a GTK-native shell** for the first Linux build, with
X11/XWayland as the supported backend and native Wayland experimental. Do not
choose CEF off-screen now: it bundles Chromium, triggers ADR-0004, and has no
measurements. Reopen ADR-0002 if the GTK shell fails P0-07 on real hardware.

This is a departure from "gpui everywhere". It is recorded in ADR-0002 and the
TODO decision log. Linux is experimental; the GTK shell keeps the engine and
core work moving while gpui lacks a Linux native-view API.

### Extension strategy

macOS-only WKWebExtension first. Windows unpacked extensions only if a real
extension passes and the UI labels it experimental. Linux none. See ADR-0003.

### Phase 1

**No-go until P0-G passes.** The blockers are evidence, not code:

1. macOS WKWebView embed and overlay/focus/resize (P0-03).
2. Windows WebView2 embed and overlay/focus/resize (P0-04).
3. Real pointer, keyboard, IME, video, and frame-rate checks on Linux (P0-05,
   P0-07).
4. A 1/10/30-tab memory and timing baseline (P0-08).

If the product decision is to start Phase 1 on macOS only before Windows
evidence exists, record that as a deviation. Do not claim two-platform parity.

## 10. Security checklist

[security-checklist.md](security-checklist.md) threat-models private stores,
password fill, agent tabs, extensions, update, imports, URL redaction,
quarantine, and logging. Every item has a verification method and an owner task.
The Phase 0 shell does not yet store credentials or run extensions; the
checklist is a Phase 1/2 requirement, not a claim about the spike binary.

## 11. Risks carried into Phase 1

| Risk | Evidence | Mitigation |
|---|---|---|
| gpui cannot host a native page on Linux | No Linux native-view API in 0.2.2 | GTK shell (ADR-0002); revisit if gpui adds one |
| WebView2 covers gpui overlays | Not tested | P0-04; popup-window fallback |
| Wayland focus/readback | Failed in a locked session | Retest unlocked; keep X11 first |
| Three engines, three capability sets | Matrix has gaps | Contract tests, capability flags, gate features |
| Phase 0 evidence is thin | One page, one run, locked screen | Repeat on real hardware before Phase 1 gate |
| Data formats are inferred | No Swift source or real files | Replace fixtures when real files exist (P1-12) |

## 12. Next actions

| Task | Owner | Exact check |
|---|---|---|
| P0-03 | macOS machine | Embed WKWebView in gpui; two tabs; overlay above page; focus; resize; screenshot. |
| P0-04 | Windows 11 machine | Embed WebView2; same checks; record whether the child HWND covers the overlay. |
| P0-05 | Linux, unlocked X11 | Real pointer click on the overlay; IME/emoji; resize; frame rate; video. |
| P0-06 | Linux, unlocked Wayland | Repeat focus and readback; record compositor and versions. |
| P0-07 | All | Five hard interactions with latency numbers. |
| P0-08 | All | 1/10/30 tabs; cold open, new tab, switch, resize, scroll, memory. |
| P0-G | Lead | Re-run the gate only when the rows above have evidence. |
