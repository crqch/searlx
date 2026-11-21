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

### 5.1 Tab benchmark (P0-08)

`crates/searlx-engine-linux/src/bin/bench.rs` creates N WebKitGTK views in a
`GtkNotebook`, loads the same deterministic local page in every tab, switches
pages for a fixed number of cycles, and sums `VmRSS` for the app plus every
descendant process. One process per tab count; X11; `example.com` is not
involved, so network variance is removed.

| Tabs | Create tabs | Window ready | All loads finished | Per-tab load avg (min–max) | RSS after load | Processes | Switch avg | Switch max | RSS after switching |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 79 ms | 100 ms | 103 ms | 103 ms | 400 MiB | 3 | 0.20 ms | 4.0 ms | 414 MiB |
| 10 | 87 ms | 110 ms | 188 ms | 175 ms (161–188) | 1.98 GiB | 12 | 0.93 ms | 8.0 ms | 2.20 GiB |
| 30 | 77 ms | 101 ms | 549 ms | 463 ms (362–549) | 5.44 GiB | 35 | 2.82 ms | 27.4 ms | 6.13 GiB |

RSS is `VmRSS` in KiB, converted here to MiB/GiB. It counts shared pages once
per process, so it overstates unique physical memory. The shape is the point.

Observations:

- WebKitGTK creates **one `WebKitWebProcess` per view** in this configuration:
  1 tab → 1 web process; 10 tabs → 10; 30 tabs → 30. Each web process is roughly
  178–209 MiB; the network process is ~65 MiB; the GTK app is ~165–180 MiB.
- Load time grows sub-linearly with concurrent tabs (103 ms → 549 ms for 30).
  The default page is small and local; a real site is heavier.
- Page switching is cheap at 1 tab and grows to 2.8 ms average at 30. The max
  (27 ms) is a missed frame at 60 Hz, so a 30-tab switch is not guaranteed
  smooth. This measures main-thread event drain, not presented frames.
- **30 live tabs cost ~6 GiB.** Sleeping tabs is not a nice-to-have; it is a
  Tier A requirement. A 30-tab session must park most views and keep a
  serialisable snapshot, or memory will be unacceptable on 8/16 GiB machines.
- The benchmark does not compare against the Swift app (not present). It is a
  Linux baseline, not a regression verdict. P0-08 stays in progress until the
  same page and tab counts are measured on macOS and Windows.

Reproduce:

```sh
cargo build -p searlx-engine-linux --bin searlx-bench-linux
for n in 1 10 30; do
  GDK_BACKEND=x11 DISPLAY=:0 XDG_RUNTIME_DIR=/tmp/run-1000 \
    ./target/debug/searlx-bench-linux --tabs "$n" --cycles 10 --seconds 60
done
```

### 5.2 Hard interactions (P0-07)

`crates/searlx-engine-linux/src/bin/interactions.rs` builds the five
interactions in the GTK shell. Its default mode drives the handlers
programmatically; `--external` waits for real X11 events (`xdotool` on a private
`Xvfb :5` 1280×900×24 display), logs widget rectangles, measures GTK frame clock
and input-to-next-frame latency (`add_tick_callback`), and samples in-page focus,
scroll, `requestAnimationFrame`, and `requestVideoFrameCallback` counters:

| Interaction | Programmatic & real X11 (`--external` + `xdotool`) | Measured latency / rate | What is still manual |
|---|---|---|---|
| Overlay menu above a page | `GtkOverlay` chip clicked via X11 pointer (`overlay_clicked count=1`); `GtkPopover` opened by clicking `Menu` (`menu_toggled active=true`) and dismissed with `Escape` (`popover_closed`). | Overlay click → next frame: **12.0 ms**; menu open → next frame: **6.1 ms**; menu close → next frame: **3.0 ms**. | Hover/selection inside popover items on a physical display. |
| Tab drag with a lift panel | Programmatic popup lift window shown at `(321, 140)` and hidden; real X11 pointer drag from `tab-right` `(104, 69)` to `tab-left` `(29, 69)` fired `tab_switched index=1` and `tab_reordered child=tab index=0`. | Tab reorder → next frame: **0.33 ms**. | Detaching a tab into a separate window across a window manager. |
| Split divider & window resize | Programmatic `560 → 374 → 224`; real X11 pointer drag moved divider `560 → 440` (`split_alloc width=1178 height=691 position=440`); `xdotool windowsize 900 620` resized the window to `900×620` (`split_alloc 898×531 position=335`). | Split divider notify → next frame: **0.05 ms**; window resize → next frame: **11.5 ms**. | 250 pt minimum clamp and 15 pt center snap in the custom split container. |
| IME / Compose / emoji & scroll | Real X11 typing `searlx `, Unicode key `U1F642` (`🙂`), and X11 Compose `Multi_key o c` produced `entry_preedit "·" → "o" → ""` and committed `"searlx 🙂©"`. Real X11 click focused left WebView (`hasFocus=true`) and 6 wheel events scrolled `scrollY: 0 → 468`. | First entry key → next frame: **14.2 ms**; GTK frame clock: **60.3 fps** (182 ticks / 3,016 ms); in-page `requestAnimationFrame`: **~62 fps** (`raf` 0 → 560 over 9 s). | Full CJK IME daemon (IBus/Fcitx5 candidate window) on an unlocked desktop. |
| Video fullscreen/PiP/floating | Local 640×360 VP9 WebM (`readyState=4`, `videoPaused=false`); `requestVideoFrameCallback` advanced `videoFrames: 1 → 112` (`videoTime` 0 → 1.21 s looped). Real X11 click + `f` key entered DOM fullscreen (`fullscreen=true`), `Escape` exited (`fullscreen=false`), `p` key reported `pip-unsupported`. **PiP is absent:** `pictureInPictureEnabled=false`, `requestPictureInPicture` missing. | ~30 fps video frame callbacks while active; self `VmRSS` ~194–202 MiB with 3 webviews + video. | Visible video frames on a physical monitor (X11 pixbuf readback over the video area is black), audio output, and a portable floating-player window. |

The program exits `ok` (3.4 s programmatic, 14.5 s external); self RSS with two
split pages, a second tab, and video was ~196–202 MiB. The important negative
result is **native PiP is not available on WebKitGTK 2.54.2**. The capability
descriptor reports `native_pip = false`; the UI must hide the PiP control on
Linux rather than offer a dead button.

The second negative result is that **window/page snapshots do not capture video
frames** even while `currentTime` and `requestVideoFrameCallback` advance. Video
evidence needs a real display or frame callback; a black PNG is not proof that
playback failed.

All five interactions are wired and verified through the real X11 event path on
`Xvfb`. Physical-display CJK IME and human visual inspection remain open for an
unlocked desktop session.

### Evidence

- [X11 page snapshot](evidence/x11-page.png)
- [X11 window snapshot](evidence/x11-window.png) — chrome, page, and the overlay
  chip are all visible.
- [X11 external interactions snapshot](evidence/x11-interactions-external.png) —
  after real X11 pointer/keyboard/drag/resize (`900×620`, tabs reordered, entry
  `"searlx 🙂©"`).
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
| Real X11 pointer/keyboard/wheel/drag/resize | Verified on `Xvfb :5` with `xdotool` (`--external`): overlay click (12.0 ms), popover toggle (6.1 ms), wheel scroll (`scrollY=468`), split drag (`560 → 440`, 0.05 ms), tab drag reorder (0.33 ms), window resize (`1180×780 → 900×620`, 11.5 ms). Physical unlocked display still recommended. |
| Real keyboard input / Compose / emoji | Verified on `Xvfb :5` (`searlx 🙂©` with `Multi_key o c` preedit `"·" → "o" → ""`); full CJK IME daemon (IBus/Fcitx5) still needs an unlocked desktop. |
| Frame rate during scroll or interaction | Measured on `Xvfb :5`: GTK frame clock **60.3 fps** (182 ticks / 3.016 s), in-page `requestAnimationFrame` **~62 fps** (`raf` 0 → 560 in 9 s), video **~30 fps** (`requestVideoFrameCallback` 112 frames). |
| Wayland focus/activation | Failed in this run; likely the locked surface. Retest unlocked. |
| Whole-window readback on Wayland | Black. Page snapshot still works. |
| Video, fullscreen, PiP | Local 640×360 VP9 plays (`readyState=4`, 112 decoded frames via `requestVideoFrameCallback`), DOM fullscreen toggles via `f`/`Escape`, **native PiP is absent** on WebKitGTK 2.54.2 (`pictureInPictureEnabled=false`). |
| Accessibility tree | Not tested. |
| macOS WKWebView embed | No macOS machine. P0-03 blocked. |
| Windows WebView2 embed | No Windows machine. P0-04 blocked. |
| 1 / 10 / 30 tab memory | Measured on Linux X11 (400 MiB / 1.98 GiB / 5.44 GiB; §5.1); macOS/Windows comparison open. |

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
