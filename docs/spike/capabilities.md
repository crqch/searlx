# Engine capability matrix

**Date:** 2026-11-21
**Scope:** Phase 0 decision support. This is not a promise; it is what each
engine can do and what still needs a person at a machine.

**Evidence markers**

| Mark | Meaning |
|---|---|
| `M` | Measured in this environment during Phase 0. |
| `D` | Documented by the engine/platform, not measured here. |
| `U` | Unknown or unverified; needs a device or an unlocked session. |
| `—` | Not available. |

The app-level features in Tier A (tabs, spaces, pins, split view, session,
history, bookmarks, reader, zoom, passwords, imports) are mostly
`searlx-core`/`searlx-ui` work. The table answers only what the engine must
provide for them.

## Matrix

| Capability | WKWebView (macOS) | WebView2 (Windows) | WebKitGTK (Linux) |
|---|---|---|---|
| Navigate, back/forward, reload, stop | Yes `D` | Yes `D` | Yes `M` |
| Execute script in page | Yes `D` | Yes `D` | Yes `M` |
| Isolated script world / content scripts | `WKContentWorld` `D` | Host scripts have no documented world; extension content scripts `D` | `WebKitScriptWorld` `D` |
| Persistent and ephemeral data stores | `WKWebsiteDataStore` `D` | User data folder / InPrivate `D` | `WebKitWebsiteDataManager`, ephemeral `D` |
| Per-space/per-profile store isolation | One data store per view/configuration `D` | One user data folder per profile `D` | One data manager per context `D` |
| Downloads | `WKDownload` `D` | `DownloadStarting` `D` | `WebKitDownload` + decide-policy `D` |
| Find in page | `find(_:configuration:completionHandler:)` `D` | `Find` API on recent runtime `U` | `WebKitFindController` `D` |
| Print | `printOperation` `D` | `ShowPrintUI` `D` | `webkit_print_operation_*` `D` |
| Export PDF | `createPDF` `D` | `PrintToPdfAsync` `D` | Needs a print-to-PDF path `U` |
| HTML fullscreen | Yes `D` | `ContainsFullScreenElementChanged` `D` | Yes `M` (DOM fullscreen entered; a scripted request is not reliable) |
| Native picture-in-picture | AVKit/WebKit `U` | Not exposed by WebView2 `U` | **—** `M` (no `pictureInPictureEnabled` / `requestPictureInPicture` in 2.54.2) |
| Camera/mic/screen permission prompts | `WKUIDelegate` `D` | `PermissionRequested` `D` | `permission-request` signal `D` |
| Geolocation | `WKUIDelegate` `D` | `PermissionRequested` `D` | `permission-request` signal `D` |
| Notifications | WebKit notification API `U` | `PermissionRequested` + web notifications `U` | WebKit notification API `U` |
| Content blockers | `WKContentRuleList` `D` | `WebResourceRequested` or extension `U` | `WebKitUserContentFilterStore` `D` |
| Extensions | `WKWebExtension` (macOS 14+) `D` | Unpacked Chromium extensions `D` | — |
| Passkeys / WebAuthn | System WebAuthn `D` | Windows Hello `D` | — |
| Input / IME | AppKit `NSTextInputClient` `D` | Win32/TSF `D` | GTK `IMContext` `D` |
| Accessibility | WebKit a11y bridge `D` | UI Automation `D` | AT-SPI `D` |
| Multi-process rendering | WebKit processes `D` | Browser + renderer `D` | WebKitNetworkProcess + WebKitWebProcess `M` |
| Strict per-site process isolation | Not promised `U` | Depends on runtime/site isolation `U` | Not promised `U` |
| Custom user agent | `customUserAgent` `D` | `Settings.UserAgent` `D` | `webkit_settings_set_user_agent*` `D` |
| Proxy configuration | `WKWebsiteDataStore.proxyConfigurations` `D` | Environment/Edge policy `U` | `GProxyResolver`/environment `U` |
| Snapshot of page | `takeSnapshot` `D` | `CapturePreviewAsync` `D` | `get_snapshot` `M` |
| Snapshot of host window | No (`paint_surface` is a gpui concern) `D` | No | X11 `M`; Wayland black `M` |

## Per-engine notes

### WKWebView

- Best match for the original app: WebKit content blockers, WKWebExtension,
  system passkeys, and the same engine family the Swift app used.
- gpui 0.2.2 has a macOS `paint_surface` path, so a native page can sit under
  gpui's overlay scene. This is the architecture P0-03 must prove.
- WebKit's process model is multi-process but not a promise of strict site
  isolation. Treat cross-site isolation as a research item, not a checkbox.
- Native PiP and fullscreen need a real video and a person; not verified here.

### WebView2

- The Windows runtime is assumed to be installed. The installer must check it
  and explain the download.
- Extensions are unpacked only; the Edge Add-ons store is not available to
  WebView2. Do not promise Chrome Web Store parity.
- A WebView2 child HWND may paint above gpui. P0-04 must test whether an
  overlay is visible and interactive above the page, and record the workaround
  (separate popup window or a composited surface).
- `Find`, notification, and site-isolation support depend on the runtime
  version. Pin a minimum and test it.

### WebKitGTK

- Measured: real HTTPS page, script scroll, overlay click, resize events,
  network/web processes, page snapshot. See `docs/spike/phase-0.md`.
- No extensions. No WebAuthn/passkeys. Both are hard gaps, not "later" items.
- gpui 0.2.2 has no Linux native-view API. Hosting WebKitGTK inside a gpui
  window is not possible today; ADR-0002 proposes a GTK-native shell instead.
- Wayland focus/activation and whole-window readback failed in a locked
  session. Native Wayland stays experimental until retested unlocked.
- P0-07 measured: HTML video loads and plays (`readyState=4`, 640×360,
  `currentTime` advances, `paused=false`), DOM fullscreen works, but **native
  PiP is absent** and window readback is black over the video surface. Text
  emoji works in the field and page; real IME composition is unmeasured.
- Frame rate and a11y are unmeasured.

## Tier A implications

| Tier A area | Engine dependency | Gap or risk |
|---|---|---|
| Field, tabs, pins, spaces, split, session | None beyond navigation/script/store | UI/core work |
| History, bookmarks | None | Core work |
| Downloads | Download events + destination control | Verify pause/resume on WebView2 and WebKitGTK |
| Find | Find API | Verify WebView2 runtime level |
| Reader, hiding elements, site CSS | Script injection at document start | Isolated worlds differ; test before-paint injection |
| Zoom | Engine zoom | All three support; verify per-tab persistence |
| Blocking | Content blocker format | WebKitGTK filters differ from WebKit rule lists; adapter needed |
| Private tabs | Ephemeral store + no app writes | Enforce in the store compositor, not per feature |
| Passwords | Script + origin check | App-owned; do not rely on engine autofill |
| Imports | None | Core parsing; treat files as hostile |
| Update | None | Platform packaging/signing |

## What this matrix does not prove

- That a page renders at 60 fps while scrolling or resizing.
- That IME composition and emoji work in the page and in the chrome.
- That a screen reader can reach the page.
- That overlays stay above the page on Windows.
- That private stores leave no trace on disk.
- That video fullscreen/PiP works.

Those are P0-03, P0-04, P0-07, P0-08, and P2-14/P2-20 work. Until they pass,
the matrix is a map of the work, not a release claim.
