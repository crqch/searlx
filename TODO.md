# TODO — searlx rewrite

**Spec:** [FEATURES.md](FEATURES.md)
**Rules:** [AGENTS.md](AGENTS.md)
**Status:** Phase 0 — not started
**Last updated:** 2026-11-20

This is the live tracker. Update it in the same commit as the work. A task is done when its **Done when** line is true on every named platform, not when it compiles.

> Note: this checkout currently contains only the planning documents. The original Swift source is not present; `FEATURES.md` is the behavioural source of truth. If the old app is restored, treat it as read-only reference, not as code to port file-by-file.

## Legend

| Mark | Meaning |
|---|---|
| `[ ]` | Todo |
| `[~]` | In progress |
| `[x]` | Done |
| `[!]` | Blocked (reason in Notes) |
| `[-]` | Dropped (reason + ADR) |

Platform tags: `[all]`, `[mac]`, `[win]`, `[linux]`, `[mac,win]`, `[mac,linux]`.

---

## Progress

| Phase | Done | Total | Status |
|---|---:|---:|---|
| 0 — Feasibility spike | 0 | 13 | Not started |
| 1 — Foundation | 0 | 16 | Not started |
| 2 — Daily driver | 0 | 23 | Not started |
| 3 — Identity and power | 0 | 20 | Not started |
| 4 — Platform and parity | 0 | 16 | Not started |
| 5 — Linux and hardening | 0 | 11 | Not started |
| Continuous | 0 | 8 | Not started |
| **Total** | **0** | **107** | |

Gates count as tasks. The "Now" section mirrors six Phase 0 items; they are not counted twice.

## Now

Current focus (status lives in Phase 0 below; this is only the order):

1. **P0-01** — Scaffold the Rust workspace.
2. **P0-02** — gpui window shell.
3. **P0-03** — macOS WKWebView embed.
4. **P0-04** — Windows WebView2 embed.
5. **P0-05** — Linux WebKitGTK X11 spike.
6. **P0-06** — Linux Wayland decision spike.

**Next gate:** P0-G — architecture confirmed. Do not start Phase 1 until P0-09 and P0-G are done.

---

## Phase 0 — Feasibility spike

**Timebox:** 2–4 weeks. **Goal:** prove the hard interactions, not build a browser. Write everything down in `docs/spike/phase-0.md`.

- [ ] **P0-01 — Scaffold the Rust workspace `[all]`**
  Create the workspace from AGENTS.md §3: `searlx-core`, `searlx-engine`, `searlx-ui`, `searlx-platform`, `searlx-testkit`, `apps/searlx`, `docs/adr`, `docs/spike`. Add `rust-toolchain.toml`, `rustfmt.toml`, `.gitignore`, CI skeleton.
  **Done when:** `cargo check --workspace` passes; `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass; an empty gpui app launches.

- [ ] **P0-02 — gpui window shell `[all]`**
  One window, hidden title bar, 1180×780 default, 640×420 minimum, a row of 2 fake tabs, an address field, and a page placeholder. No real engine yet.
  **Done when:** it opens on macOS, Windows, and Linux (X11); tab click switches; field takes IME/emoji; reduce-motion and light/dark are respected.

- [ ] **P0-03 — macOS WKWebView embed `[mac]`**
  Embed a real `WKWebView` as a native child of the gpui window via `objc2`/`objc2-web-kit`. Load two tabs; keep each page alive across switching. Place a gpui overlay above the page.
  **Done when:** pages load, tab switching does not reload, the overlay is visible and interactive above the page, input/focus is correct, and resizing tracks without flicker.

- [ ] **P0-04 — Windows WebView2 embed `[win]`**
  Embed WebView2 via `windows-rs` or `wry` behind the engine trait. Load two tabs; keep them alive. Place a gpui overlay above the page.
  **Done when:** same as P0-03 on Windows 11; note whether the WebView2 child window covers gpui overlays and what workaround is needed; record input latency.

- [ ] **P0-05 — Linux WebKitGTK X11 spike `[linux]`**
  Embed WebKitGTK in a gpui window on X11/XWayland. Load a real page. Test overlay, focus, resize, IME, video.
  **Done when:** a real page renders and scrolls at a usable frame rate; overlays and focus are possible or the limitation is measured and written down.

- [ ] **P0-06 — Linux Wayland decision spike `[linux]`**
  Try the same on a native Wayland session. Compare: WebKitGTK child surface, XWayland, GTK-native shell, CEF OSR. Do not build a production integration yet.
  **Done when:** `docs/spike/phase-0.md` has a recommendation and the reasons; ADR-0002 is updated.

- [ ] **P0-07 — Hard interactions prototype `[all]`**
  Build the five interactions that decide the architecture: (1) overlay menu above a page; (2) tab drag with a floating lift panel; (3) split-view divider resize; (4) IME/emoji in a page and in the field; (5) video fullscreen/PiP/floating.
  **Done when:** each interaction works or has a measured fallback; latency numbers are recorded per OS.

- [ ] **P0-08 — Performance baseline `[all]`**
  Measure cold open, new tab, tab switch, split resize, scroll, and memory with 1/10/30 tabs. Use the same page and hardware per OS. Record in `docs/spike/phase-0.md`.
  **Done when:** numbers are comparable to the Swift app and any regressions have a cause.

- [ ] **P0-09 — Spike report `[all]`**
  Write `docs/spike/phase-0.md`: what worked, what did not, per-OS capability table, recommended Linux engine, recommended extension strategy, risks, and a go/no-go for Phase 1.
  **Done when:** AGENTS.md platform matrix and TODO ADRs are updated from evidence.

- [ ] **P0-10 — Capability matrix `[all]`**
  Write `docs/spike/capabilities.md`: WKWebView vs WebView2 vs WebKitGTK for navigation, scripts/worlds, data stores, downloads, find, PDF/print, fullscreen/PiP, permissions, extensions, passkeys, content blockers, input/IME, a11y, and process isolation.
  **Done when:** every Tier A feature has a per-engine answer or an explicit gap.

- [ ] **P0-11 — Data compatibility fixture `[all]`**
  Create a small corpus of existing Search data files (session, history, bookmarks, pins, spaces, hidden, styles) and a round-trip reader in `searlx-core`. No UI yet.
  **Done when:** fixtures parse; unknown fields survive; corrupt files quarantine; tests pass.

- [ ] **P0-12 — Phase 0 security checklist `[all]`**
  Threat-model the new shell: private stores, password fill, agent tabs, extension boundaries, update signing, imported files, URL redaction.
  **Done when:** checklist is in `docs/spike/phase-0.md` and every item has an owner or a Phase 1 task.

- [ ] **P0-G — Gate: architecture confirmed `[all]`**
  Confirm ADR-0001 (gpui + system webviews), ADR-0002 (Linux engine), ADR-0003 (extension strategy), and ADR-0004 (no bundled Chromium by default). If the gate fails, stop and revise the plan before Phase 1.

---

## Phase 1 — Foundation

**Goal:** a real two-platform browser skeleton with tabs, a field, a session, and settings.

- [ ] **P1-01 — `searlx-core` domain types `[all]`**
  URLs/identity, `TabId`, `SpaceId`, `ProfileId`, history visit, bookmark tree, pin, group, split, session shape, permission key, engine id. No I/O.
  **Done when:** unit tests cover URL identity, migrations, and serialization.

- [ ] **P1-02 — Data stores and migrations `[all]`**
  Read/write the existing JSON formats from FEATURES.md §25. Atomic temp+rename writes, serialized writes, quarantine on bad decode, versioned schema.
  **Done when:** the P0-11 fixture round-trips byte-equivalently where expected and old files load.

- [ ] **P1-03 — Engine trait and events `[all]`**
  Finalise `Engine`, `TabOptions`, `EngineEvent`, `Capabilities`, `DataStoreHandle`, `ScriptWorld`, `InjectAt`, `FindOptions`, `FindState`. Add a fake engine for tests.
  **Done when:** `searlx-testkit` can drive the fake through create/load/back/forward/close and assert the event stream.

- [ ] **P1-04 — macOS backend `[mac]`**
  WKWebView behind `Engine`: create/close/navigate/back/forward/reload/stop/eval/inject/set_bounds/visible/focus/snapshot/find/zoom/mute/data store/permissions/downloads/events.
  **Done when:** contract tests pass against real WKWebView; two tabs switch without reload; private store is ephemeral.

- [ ] **P1-05 — Windows backend `[win]`**
  WebView2 behind `Engine`, same contract. User data folder per profile/space; ephemeral profile for private tabs.
  **Done when:** contract tests pass on Windows 11; two tabs switch without reload; private profile leaves no data.

- [ ] **P1-06 — Tab model and row `[all]`**
  Tab state, active tab, pinned flag, title/icon, loading, audio, sleep placeholder. Side and top layouts, icons-only, fold. Use one drag session.
  **Done when:** create/close/reorder/pin/unpin works on macOS and Windows; a tab drag is smooth at 60+ fps.

- [ ] **P1-07 — Address field and parsing `[all]`**
  Port FEATURES.md §4.2 parsing rules, Return/⌘Return/⇧⌘Return, completion, suggestions from history/bookmarks/open tabs, site chip, commands. Invalid input shakes.
  **Done when:** parser unit tests mirror the spec; manual cases: `localhost:3000`, `~/file`, `example.com`, `hello world`, `mailto:`, `[::1]:3000`, look-alike hosts.

- [ ] **P1-08 — Session and restore `[all]`**
  Write/read session and windows files. Restore tabs asleep; active tab wakes; window frames; fresh-start mode; quarantine.
  **Done when:** quit/relaunch restores tabs without loading every page; a 30-tab session opens fast; corrupt session is quarantined.

- [ ] **P1-09 — Settings shell `[all]`**
  Settings as a tab (`search://settings`), 11 pages, search field, typed settings with defaults. Start with General/Appearance/Tabs/Browsing/Privacy.
  **Done when:** settings persist; changing one redraws the relevant UI; search finds rows and synonyms.

- [ ] **P1-10 — Theme, metrics, icons `[all]`**
  Light/dark/system palette, metrics module, tab glyphs (letters/favicons), high contrast, reduce motion. No magic numbers scattered.
  **Done when:** appearance matches the design intent on both OSes; text scales; icons render at 1x/2x.

- [ ] **P1-11 — Command and shortcut layer `[all]`**
  One command registry, default keys, user overrides, menu/action routing, extension commands later. `⌘S` folds, does not save.
  **Done when:** every Phase 1 action is reachable by keyboard and menu; conflicts are rejected.

- [ ] **P1-12 — Import existing Search data `[all]`**
  Import the old app's session/history/bookmarks/pins/spaces/hidden/styles files into `searlx-core`. No browser imports yet.
  **Done when:** a user can point at an old Application Support folder and get their tabs/pins/bookmarks in the new app.

- [ ] **P1-13 — Packaging skeleton `[mac,win]`**
  macOS `.app` bundle + ad-hoc signing; Windows `.exe` + WebView2 runtime check; dev run command. No notarization yet.
  **Done when:** a fresh machine can run the build; the macOS bundle opens; Windows shows a clear message if WebView2 is missing.

- [ ] **P1-14 — CI `[all]`**
  GitHub Actions: fmt, clippy, test, build on macOS/Windows/Linux; cache Rust; artifact upload. Keep Linux non-blocking until Phase 5.
  **Done when:** a PR gets a green/red signal; test time is reasonable; warnings are denied.

- [ ] **P1-15 — Phase 1 dogfood `[all]`**
  Use it for a day: open 10 sites, switch tabs, restore, search, bookmark, download. File every issue as a TODO item.
  **Done when:** a written dogfood report exists and the top 10 issues are scheduled.

- [ ] **P1-G — Gate: two-platform alpha `[mac,win]`**
  Tabs, field, session, settings, history, bookmarks, and downloads work on macOS and Windows. If not, do not start Phase 2.

---

## Phase 2 — Daily driver

**Goal:** replace the old browser for ordinary browsing.

- [ ] **P2-01 — Navigation and history `[all]`**
  Back/forward, long-press history menu, reload/stop/origin, redirects, errors, per-tab history, internal-page trail, scroll restoration.
  **Done when:** a 50-link session behaves like the spec; Back from the first page does nothing; errors are in-page.

- [ ] **P2-02 — Find on page `[all]`**
  Pill, count, next/prev, match case, whole words, PDF fallback, 1,000 limit, isolated world, search settings on the settings tab.
  **Done when:** matches the spec's status strings; a 1,000-match page stays responsive.

- [ ] **P2-03 — History page `[all]`**
  Grouped by day, search, remove one, clear all/scope, 2,000 cap, frecency suggestions, no date range.
  **Done when:** clearing history behaves exactly as FEATURES.md §10 says, including archive/favicons/site searches.

- [ ] **P2-04 — Bookmarks `[all]`**
  Tree, add/edit/delete/move, bar, menu, page, search, suggestions, import merge, no export writer.
  **Done when:** nested folders and 1,500 imported bookmarks open quickly; drag/drop reorder works.

- [ ] **P2-05 — Downloads `[all]`**
  Tab/popover/panel, destination/ask, pause/resume/retry/cancel, naming, progress, no Notification Center, private unlisted.
  **Done when:** a 1 GB download pauses/resumes; a POST download says it cannot pause; a private download is not listed.

- [ ] **P2-06 — Reader `[all]`**
  Extraction heuristic, Markdown twin, light/dark/system, images/tables/code, reload on exit.
  **Done when:** 10 representative articles render cleanly; a code-heavy page keeps its code; exiting restores the page.

- [ ] **P2-07 — Blocking `[all]`**
  Content blocker rules (WebKit format), YouTube ads, per-site pause, first-load wait, cosmetic selectors. Windows/Linux adapters behind capability.
  **Done when:** a tracker-heavy page is lighter; YouTube ads are removed or the fallback mutes/skips; per-site off works.

- [ ] **P2-08 — Hide elements `[all]`**
  Picker, selector synthesis, per-site list, before-first-paint injection, undo, review, private memory-only.
  **Done when:** hiding a cookie banner survives reload with no flash; review restores one/all.

- [ ] **P2-09 — Site CSS `[all]`**
  CSS editor, live apply, per-site file, before-paint style, off keeps styles, private read-only.
  **Done when:** a site style survives reload; no script is executed.

- [ ] **P2-10 — Darkening `[all]`**
  Measurement, dark twins/filter fallback, per-site toggle, pause, private page-only, budgets.
  **Done when:** a light article darkens without touching photos; a dark site is untouched; budget gives up cleanly.

- [ ] **P2-11 — Translate `[all]`**
  On-device where available; target/always lists; DOM text nodes/attributes; frames; show original; private.
  **Done when:** a French article translates in place; no element is added/removed; Show Original restores exact text.

- [ ] **P2-12 — Capture `[all]`**
  Area, element, whole page, clipboard + PNG, limits, private transient, no upload.
  **Done when:** a 20,000 pt page captures without a white strip; an element capture outlines correctly.

- [ ] **P2-13 — Site card `[all]`**
  Connection, certificate, copy/print, zoom, blocker, permissions, autoplay, float-off, darken, keep-awake.
  **Done when:** every row in FEATURES.md §5.8 is present and gated correctly.

- [ ] **P2-14 — Permissions and private tabs `[all]`**
  Camera/mic/location/notifications/screen; per-origin; private refusal; forget choices; ephemeral store; no history/session/favicon/password offer.
  **Done when:** a private tab leaves no trace after close; permissions persist per site only in normal tabs.

- [ ] **P2-15 — Password vault `[all]`**
  Credential-store trait + OS backends; save/fill; account list; Touch ID/Hello/Secret Service; show/copy gates; clear-http rules; no export.
  **Done when:** a sign-in saves after success, fills only on the same origin, and never fills a password saved from https on http.

- [ ] **P2-16 — Keyboard and context menus `[all]`**
  All Phase 1/2 commands, default keys, user overrides, tab/page/image menus, reduced/disabled entries.
  **Done when:** the shortcut table in FEATURES.md §22 is covered or explicitly dropped; menu items disable correctly.

- [ ] **P2-17 — Internal pages `[all]`**
  `search://settings|history|downloads|bookmarks|passwords|archive|import|release-notes`, one tab per kind, Back/Forward trail.
  **Done when:** asking twice brings the existing tab forward; private tabs never become internal pages.

- [ ] **P2-18 — Update mechanism `[mac,win]`**
  Signed feed, hash/signature verification, staged install, no auto-relaunch, rollback behavior.
  **Done when:** a bad signature is refused; a good update installs on next launch; the old bundle is swept.

- [ ] **P2-19 — Crash and error recovery `[all]`**
  Process crash, reload loops, in-page errors, quarantine, crash log.
  **Done when:** a crashing page shows a retry after two failures; a corrupt file is set aside, not overwritten.

- [ ] **P2-20 — Accessibility baseline `[all]`**
  Names/roles/focus for chrome; page content reachable; keyboard-only pass; reduced motion; high contrast.
  **Done when:** a screen reader can list tabs, use the field, and reach the page on macOS and Windows.

- [ ] **P2-21 — Performance budget `[all]`**
  Cold start, new tab, switch, 30 tabs, scroll, memory. Add a benchmark harness.
  **Done when:** numbers are within the Phase 0 baseline; a regression fails CI or is reported.

- [ ] **P2-22 — Phase 2 dogfood `[all]`**
  Two weeks of daily use on macOS and Windows. File issues, fix the top 10, then gate.
  **Done when:** the old browser is not needed for a normal day.

- [ ] **P2-G — Gate: daily driver `[mac,win]`**
  If the browser cannot be used for a normal day without falling back, do not start Phase 3.

---

## Phase 3 — Identity and power

**Goal:** spaces, profiles, pins, groups, splits, and the signature interactions.

- [ ] **P3-01 — Spaces `[all]`**
  Per-space tabs, parked rows, own/shared store, history routing, create/rename/delete/reorder, colors/gradients/icons, downloads folder, routes.
  **Done when:** two spaces can be signed into different accounts; switching parks tabs and returns them intact; delete cleans the store with retries.

- [ ] **P3-02 — Profiles `[all]`**
  Per-profile spaces, cookies, history, bookmarks, archive, windows; shared settings/extensions/passwords/downloads.
  **Done when:** two profiles do not share cookies/history/bookmarks; deleting one removes its files/stores; a window never changes profile.

- [ ] **P3-03 — Profiles in one window `[all]`**
  Swap whole profile windows in place; tuck siblings; one untucked; close as a group; unmerge on setting off.
  **Done when:** switching profile spaces never reuses the old profile's stores; closing the visible window closes the group; relaunch restores the merge.

- [ ] **P3-04 — Pins `[all]`**
  Squares/rows, identity across windows, home, peek, command-click, letter, rehome, shared pins, drag boundary.
  **Done when:** a pin is the same in every window; ⌘W puts it down and it wakes at home; shared pins appear in every space.

- [ ] **P3-05 — Pin folders and live folders `[all]`**
  3-level folders, drag/nest, rename/delete, live RSS/Atom folders, 12 posts, 30 min refresh, 2 MB cap, no cookies.
  **Done when:** a feed folder updates and opens posts in new tabs; shared/squares/private cannot enter a folder.

- [ ] **P3-06 — Tab groups `[all]`**
  Name/color/collapse, drag in/out, close group + undo, fold all, move group to space, extension id.
  **Done when:** closing a group and ⇧⌘T restores it whole; an empty group disappears.

- [ ] **P3-07 — Split View `[all]`**
  2–4 panes, 7 pt gutter, 250 min, 15 pt snap, divider, focus, swap, separate, close all, persistence.
  **Done when:** four panes resize and persist; closing one leaves the partner; a pair restores as a pair.

- [ ] **P3-08 — Tab switcher `[all]`**
  0.15 s delay, 5 columns/10 cards, moons, recency, ⌃W/R/M/O, previews, backwards.
  **Done when:** a quick ⌃Tab returns to the previous tab without drawing the grid; moons bring little windows back.

- [ ] **P3-09 — Peek `[all]`**
  Shift-click panel, keep as tab/beside, stays-with-tab, pause hidden media, pin peek.
  **Done when:** Escape returns to the exact page state; keeping a peek does not reload the page.

- [ ] **P3-10 — Small windows `[all]`**
  Link windows, cascade, Open in Search, state, moons, popup reuse.
  **Done when:** a link from another app opens a small window without forcing the browser window forward; ⌘O moves it in.

- [ ] **P3-11 — Floating video `[all]`**
  Reparent/live page or a portable fallback, triggers, controls, flick/dock, minimize, return, system PiP where available.
  **Done when:** video keeps playing while switching apps/tabs; returning restores the page; private-tab policy is decided.

- [ ] **P3-12 — Gestures `[all]`**
  Back/forward swipe + hold history, space swipe + wheel/buttons, peek pinch, autoscroll, tab give, fold edge.
  **Done when:** the thresholds in FEATURES.md §23 behave; a horizontal carousel is not stolen; haptics are optional.

- [ ] **P3-13 — Tab lifecycle `[all]`**
  Sleep (10/30/120 min, memory pressure), auto-close (12h/1d/1w/1m), exceptions, archive, reopen, twins.
  **Done when:** a 30-tab session sleeps and wakes with scroll; an auto-closed tab is in the archive and reopens.

- [ ] **P3-14 — Folder sync `[all]`**
  `Search Sync/<scope>/<device>.json`, 17 settings, spaces/pins/bookmarks, merge/tombstones/365-day deletion, profile UUIDs.
  **Done when:** two machines converge; deleting an item does not resurrect; pin folders stay local.

- [ ] **P3-15 — Handoff `[mac]`**
  NSUserActivity/Handoff; eligible page rules; incoming default-browser links.
  **Done when:** a page appears on an iPhone; private/signed-out pages never do.

- [ ] **P3-16 — Side panel `[all]`**
  Website/extension/downloads panels, 300–640 width, per-space/per-tab, private hold-back, no `⌘W` close.
  **Done when:** Claude/ChatGPT work in the panel; switching space reloads in the right store.

- [ ] **P3-17 — Launcher `[all]`**
  Global hotkey, floating field, recent/bookmarks/history, open tab vs small window, key recording.
  **Done when:** the key works over another app without activating Search; Escape returns to the app.

- [ ] **P3-18 — Space routes and link chooser `[all]`**
  Site→space rules, ask-which-space panel, number keys/Always, last choice per site, small-window choice.
  **Done when:** a link from Mail opens in the chosen space with the right sign-ins; typed addresses are not routed.

- [ ] **P3-19 — Phase 3 dogfood `[all]`**
  Live in spaces/profiles/pins/splits for two weeks. Fix the top issues.
  **Done when:** the signature workflow is not a regression from the old app.

- [ ] **P3-G — Gate: identity complete `[all]`**
  Spaces, profiles, pins, groups, splits, switcher, peek, small windows, floating video, and sync work. If not, do not start Phase 4.

---

## Phase 4 — Platform and parity

**Goal:** the features that define the macOS app and the remaining platform integrations.

- [ ] **P4-01 — Extensions foundation `[mac]`**
  `WKWebExtension` controller, install from CRX/store, unpacked, enable/disable/remove/reload, permissions, site access, installed list.
  **Done when:** a real Chrome extension installs, runs a content script, and opens a popup; an update asking for more prompts.

- [ ] **P4-02 — Extension shims `[mac]`**
  Bookmarks/history/downloads/sidePanel/offscreen/identity/tabGroups/contextMenus/native messaging/OAuth/WebSocket/credentialed fetch.
  **Done when:** the Tier A extension APIs in FEATURES.md §14.3 work; unsupported APIs fail loudly, not silently.

- [ ] **P4-03 — Extension security `[mac]`**
  CRX3 signature, path confinement, shim rewrite after verification, permission diff, private tabs, other-extension pages, agent tabs.
  **Done when:** a tampered CRX is refused; an extension cannot read private tabs without permission; no path escape.

- [ ] **P4-04 — Passkeys `[mac,win]`**
  Platform WebAuthn, conditional mediation, create/get, security keys, PRF where available, private-tab policy, fallback.
  **Done when:** a site's passkey sign-in works on macOS and Windows; when unavailable the site falls back to password without wedging.

- [ ] **P4-05 — Apple Passwords `[mac]`**
  Chrome-helper native messaging, SRP pairing, names/codes, 5+5 cap, https/private rules, offer-once.
  **Done when:** a paired helper offers accounts under a sign-in; quitting requires re-pairing; no secret is logged.

- [ ] **P4-06 — Apple integrations `[mac]`**
  AppleScript read-only, iCloud sync, Handoff, Contacts fill, Touch ID, default browser/email, share sheet, Dock menu, media keys.
  **Done when:** each is gated, documented, and has a test or a manual checklist; the app still works with each off.

- [ ] **P4-07 — AI add-on `[all]`**
  Providers, keys, streaming, page extraction, summary/ask, on-device engine, OpenRouter sign-in. Keep `AI.shipped` equivalent off until ready.
  **Done when:** a local and a cloud provider both answer; private tabs are local-only; keys are never logged.

- [ ] **P4-08 — Coding agents `[all]`**
  Local socket, MCP client, consent, trust, agent tabs, read/click/type/scroll/wait/screenshot, redaction, Stop.
  **Done when:** Claude Code can browse in its own tabs; password/card/OTP values never leave the page; Stop is immediate.

- [ ] **P4-09 — Bench/test harness `[all]`**
  Local socket, test worlds, `SEARCH_PROBE` equivalent, hidden probe, UI automation, screenshots, settings read/write.
  **Done when:** the harness can drive a hidden app; no window reaches a screen; a test run never touches real data.

- [ ] **P4-10 — iCloud sync `[mac]`**
  CloudKit records, CKSyncEngine, merge rules, Personal-only, delete-data, mutual exclusion with folder sync.
  **Done when:** two Macs converge; private/shared pins/history never sync; delete-data turns sync off.

- [ ] **P4-11 — Packaging and signing per OS `[all]`**
  macOS notarization/stapling, Windows MSIX or Squirrel, Linux Flatpak/AppImage, update feeds, code-signature checks.
  **Done when:** a clean install, update, and uninstall work on each OS; a bad signature is refused.

- [ ] **P4-12 — Extension strategy outside macOS `[win,linux]`**
  Implement only if ADR-0003 says so. WebView2 BYO unpacked extensions; Linux none unless CEF. Do not promise store installs.
  **Done when:** the capability is gated and labelled; no fake API surface.

- [ ] **P4-13 — Accessibility parity `[all]`**
  Full chrome and page a11y, screen reader passes, focus order, high contrast, reduced motion, text scaling.
  **Done when:** a screen-reader user can browse, switch tabs, and use the field on each supported OS.

- [ ] **P4-14 — Security review `[all]`**
  External or internal review of the engine boundary, passwords, passkeys, extensions, agents, update, imports.
  **Done when:** every high/critical finding is fixed or has a dated plan; the threat model is updated.

- [ ] **P4-15 — Phase 4 dogfood `[all]`**
  Use extensions/passkeys/sync/agents for two weeks. Fix top issues.
  **Done when:** the macOS app is not obviously better for the target workflows.

- [ ] **P4-16 — Phase 4 gate `[mac,win]`**
  If extensions/passkeys/sync/agents are not stable, do not advertise parity. Ship with gated features instead.

---

## Phase 5 — Linux and hardening

**Goal:** a credible Linux build, or an explicit decision not to ship one.

- [ ] **P5-01 — Execute ADR-0002 `[linux]`**
  Implement the chosen Linux engine path: WebKitGTK X11/XWayland, CEF OSR, or GTK-native shell. Record why.
  **Done when:** a real page loads, overlays work, input/IME is usable, and the app starts on a current Fedora/Ubuntu.

- [ ] **P5-02 — Linux packaging `[linux]`**
  Flatpak (preferred), AppImage, .deb. WebKitGTK runtime dependency and version floor documented.
  **Done when:** a clean VM installs and runs; updates work; no host library conflicts.

- [ ] **P5-03 — Linux integration `[linux]`**
  Secret Service, libnotify, X11/Wayland shortcuts, proxy, file dialogs, MIME/default browser, power.
  **Done when:** passwords save/fill via Secret Service; notifications work; the app can be the default browser.

- [ ] **P5-04 — Linux feature matrix `[linux]`**
  Passkeys (likely none), extensions (likely none), DRM/codecs, screen share, PiP, hardware video. Gate each.
  **Done when:** the matrix is in the README and the UI hides unsupported features.

- [ ] **P5-05 — Performance tuning `[all]`**
  Cold start, tab switch, scroll, memory, 120 Hz, process count, snapshot cost, engine pump.
  **Done when:** budgets are met on reference hardware; regressions are caught.

- [ ] **P5-06 — Fuzzing `[all]`**
  URL parser, data-file decoders, import parsers, CRX/manifest parser, agent/extension message parsers.
  **Done when:** fuzz targets run in CI for a fixed time; crashes are fixed.

- [ ] **P5-07 — Security hardening `[all]`**
  Sandboxing, site isolation, permission defaults, CSP for internal pages, IPC validation, update channel.
  **Done when:** the threat model's mitigations are implemented and tested.

- [ ] **P5-08 — Release candidate `[all]`**
  Feature freeze, bug bash, docs, privacy page, changelog, support matrix.
  **Done when:** a release candidate passes the full manual matrix on each OS.

- [ ] **P5-09 — 1.0 release `[all]`**
  Sign, notarize/package, publish, update feed, rollback plan.
  **Done when:** a new user can download, install, browse, update, and uninstall.

- [ ] **P5-10 — Post-launch `[all]`**
  Crash triage, CVE response, engine updates, regression tests.
  **Done when:** a maintenance cadence exists and the first patch ships.

- [ ] **P5-11 — Phase 5 gate `[linux]`**
  If Linux cannot meet the security/quality bar, do not ship it as a supported platform. Say so.

---

## Continuous

- [ ] **C-01 — Security review each phase** `[all]` — threat model, secrets, permissions, update, imports.
- [ ] **C-02 — Performance budget each phase** `[all]` — cold start, tab switch, scroll, memory.
- [ ] **C-03 — Accessibility check each phase** `[all]` — focus, names, roles, scaling, reduced motion.
- [ ] **C-04 — Data migration test each phase** `[all]` — old Search files still load.
- [ ] **C-05 — Docs** `[all]` — README, privacy, support matrix, ADRs, changelog.
- [ ] **C-06 — Dependency review** `[all]` — licenses, advisories, size.
- [ ] **C-07 — Engine matrix** `[all]` — capability table per engine/OS stays current.
- [ ] **C-08 — Dogfood** `[all]` — use the build daily; top issues become TODO items.

---

## Decision log (ADRs)

Create `docs/adr/NNNN-title.md` for each. Status: proposed/accepted/superseded.

### ADR-0001 — gpui + system webviews
**Status:** proposed. **Decision:** gpui for chrome; WKWebView/WebView2/WebKitGTK for pages; no bundled Chromium by default.
**Why:** product feel, small binaries, no second engine, platform-native security updates.
**Risk:** three capability sets; Linux embedding; gpui pre-1.0.
**Decided by:** P0-09/P0-G.

### ADR-0002 — Linux engine
**Status:** open. **Options:** (a) WebKitGTK X11/XWayland; (b) CEF off-screen; (c) GTK-native shell; (d) no Linux.
**Decision:** pending P0-05/P0-06.

### ADR-0003 — Extension strategy
**Status:** open. **Options:** (a) macOS-only WKWebExtension; (b) macOS-only + WebView2 BYO; (c) CEF everywhere; (d) drop.
**Decision:** pending P0-10.

### ADR-0004 — Bundled engine
**Status:** proposed no. The default build must not bundle Chromium. A separate "Search Max" build would need a new ADR.

### ADR-0005 — Passkeys on Linux
**Status:** open. WebKitGTK has no WebAuthn today. Options: no passkeys; CEF; external-browser handoff (not a real solution).

### ADR-0006 — Sync backend
**Status:** open. Folder sync is the portable baseline; iCloud is macOS-only; a server contradicts the no-account promise.

### ADR-0007 — Existing Swift app
**Status:** proposed keep. The Swift macOS app remains the reference and can ship while the Rust app matures. Do not rewrite it in gpui.

### ADR-0008 — Distribution and update
**Status:** open. macOS notarized DMG/Sparkle-like; Windows MSIX/Squirrel; Linux Flatpak/AppImage. One signed feed per channel.

---

## Risks and blockers

| Risk | Impact | Mitigation | Owner |
|---|---|---|---|
| Linux Wayland cannot host a WebKitGTK page cleanly | Linux may need CEF or a GTK shell | P0-06, ADR-0002; timebox, do not commit early | P0 |
| WebView2 child window covers gpui overlays | Menus/popovers above page break on Windows | P0-04; test composition; fallback to a popup window | P0 |
| Floating video reparenting differs per engine | Signature feature breaks | P0-07; design a portable player fallback | P0 |
| Extension parity is impossible outside macOS | Product promise weakens | ADR-0003; gate and label; don't fake | P4 |
| Passkeys absent on Linux | Sign-in parity gap | ADR-0005; document; no fake shim | P4 |
| gpui pre-1.0 churn | Maintenance cost | Pin a git revision; isolate UI; track upstream | all |
| Three engines = three QA matrices | Slow releases | Capability matrix, contract tests, CI per OS | all |
| Security review finds a critical issue | Launch delay | Phase 2/4 reviews; fix before 1.0 | all |
| Data formats drift from the old app | Users lose tabs/bookmarks | P1-02/P1-12; migration tests | P1 |
| Solo/small-team capacity | Slipped phases | Cut Tier B first; ship gated features | all |

## Blockers

_None yet._

## Progress log

- **2026-11-20** — Created `AGENTS.md` and `TODO.md`. Phase 0 not started. The original Swift checkout is no longer present in this working tree; `FEATURES.md` is the behavioural source of truth.

---

## Definition of done (per task)

- [ ] Behaviour matches the FEATURES.md section (or the deviation is recorded).
- [ ] Unit/contract/UI tests added or updated.
- [ ] `cargo fmt`, `cargo clippy -D warnings`, `cargo test --workspace` pass.
- [ ] Manual check on every named platform.
- [ ] Accessibility/security implications checked.
- [ ] TODO status and notes updated; ADR/spec updated if needed.
- [ ] No secrets, large binaries, certificates, or build output committed.
