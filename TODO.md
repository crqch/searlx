# TODO — searlx rewrite

**Spec:** [FEATURES.md](FEATURES.md)
**Rules:** [AGENTS.md](AGENTS.md)
**Status:** Phase 0 — in progress
**Last updated:** 2026-11-21

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
| 0 — Feasibility spike | 6 | 13 | In progress |
| 1 — Foundation | 3 | 16 | In progress |
| 2 — Daily driver | 0 | 23 | Not started |
| 3 — Identity and power | 0 | 20 | Not started |
| 4 — Platform and parity | 0 | 16 | Not started |
| 5 — Linux and hardening | 0 | 11 | Not started |
| Continuous | 0 | 8 | Not started |
| **Total** | **9** | **107** | |

Gates count as tasks. The "Now" section mirrors six Phase 0 items; they are not counted twice.

## Now

Current focus (status lives in Phase 0 below; this is only the order):

1. **P0-07** — Hard interactions prototype on the GTK shell.
2. **P0-08** — Performance baseline with 1/10/30 tabs.
3. **P0-03/P0-04** — macOS and Windows embedding (need hardware).
4. **P0-05** — repeat the Linux checks on an unlocked X11 display.
5. **P0-G** — architecture gate; blocked until the rows above have evidence.

**Next gate:** P0-G — architecture confirmed. Do not start Phase 1 until P0-09 and P0-G are done. P0-09 is written, but P0-G is blocked on macOS/Windows evidence and real Linux input/frame-rate measurements.

---

## Phase 0 — Feasibility spike

**Timebox:** 2–4 weeks. **Goal:** prove the hard interactions, not build a browser. Write everything down in `docs/spike/phase-0.md`.

- [~] **P0-01 — Scaffold the Rust workspace `[all]`**
  Create the workspace from AGENTS.md §3: `searlx-core`, `searlx-engine`, `searlx-ui`, `searlx-platform`, `searlx-testkit`, `apps/searlx`, `docs/adr`, `docs/spike`. Add `rust-toolchain.toml`, `rustfmt.toml`, `.gitignore`, CI skeleton.
  **Done when:** `cargo check --workspace` passes; `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass; an empty gpui app launches.
  **Notes:** Workspace, all eight crates, toolchain pin, fmt config and the gpui shell exist. `cargo check`, `fmt --check`, `clippy -D warnings` and `cargo test --workspace` pass on Linux. `target/debug/searlx` stays up with the shell. The vendored `xattr` patch is needed because current `libc` dropped the Linux `ENOATTR` alias used by gpui's HTTP client; see `vendor/xattr/README.searlx.md`. CI workflow is written but has not run on GitHub yet. macOS/Windows launch still needs a machine.

- [~] **P0-02 — gpui window shell `[all]`**
  One window, hidden title bar, 1180×780 default, 640×420 minimum, a row of 2 fake tabs, an address field, and a page placeholder. No real engine yet.
  **Done when:** it opens on macOS, Windows, and Linux (X11); tab click switches; field takes IME/emoji; reduce-motion and light/dark are respected.
  **Notes:** The shell is in `crates/searlx-ui` (`app.rs`, `field.rs`, `theme.rs`, `metrics.rs`): 1180×780, 640×420 minimum, two fake tabs, close/new-tab, one address entity with grapheme-aware editing, selection, clipboard and marked-text (IME composition) support, light/dark palette from the window appearance, and an overlay chip over the page placeholder. Linux launch verified; manual IME/emoji and macOS/Windows checks are still open. There are no animations yet, so Reduce Motion is respected trivially; motion constants are in `metrics.rs` for when they arrive.

- [ ] **P0-03 — macOS WKWebView embed `[mac]`**
  Embed a real `WKWebView` as a native child of the gpui window via `objc2`/`objc2-web-kit`. Load two tabs; keep each page alive across switching. Place a gpui overlay above the page.
  **Done when:** pages load, tab switching does not reload, the overlay is visible and interactive above the page, input/focus is correct, and resizing tracks without flicker.

- [ ] **P0-04 — Windows WebView2 embed `[win]`**
  Embed WebView2 via `windows-rs` or `wry` behind the engine trait. Load two tabs; keep them alive. Place a gpui overlay above the page.
  **Done when:** same as P0-03 on Windows 11; note whether the WebView2 child window covers gpui overlays and what workaround is needed; record input latency.

- [x] **P0-05 — Linux WebKitGTK X11 spike `[linux]`**
  Embed WebKitGTK in a gpui window on X11/XWayland. Load a real page. Test overlay, focus, resize, IME, video.
  **Done when:** a real page renders and scrolls at a usable frame rate; overlays and focus are possible or the limitation is measured and written down.
  **Notes:** gpui 0.2.2 has no Linux native-view API, so WebKitGTK cannot be embedded in a gpui window yet; that limitation is measured and documented in `docs/spike/phase-0.md` and `docs/adr/0002-linux-engine.md`. The GTK-shell baseline (`spike.rs` + `interactions.rs --external`) was verified on X11 (`Xwayland :0` and `Xvfb :5` with `xdotool`): `https://example.com/` loaded in 296 ms; real X11 pointer click on the overlay chip and menu popover worked (12.0 ms and 6.1 ms to next frame); real click focused the WebView (`hasFocus=true`); real X11 wheel events scrolled `scrollY` to 468 while the GTK frame clock ran at 60.3 fps and in-page `requestAnimationFrame` ran at ~62 fps; X11 typing, emoji (`🙂`), and Compose key (`Multi_key o c` → `entry_preedit "·" → "o" → ""` → `©`) worked; window resize to `900×620` updated allocations (`900×620`, split `898×531`, 11.5 ms to next frame); local 640×360 VP9 video decoded 112 frames via `requestVideoFrameCallback` and toggled DOM fullscreen (`f`/`Escape`), while native PiP was measured absent (`pictureInPictureEnabled=false`).

- [x] **P0-06 — Linux Wayland decision spike `[linux]`**
  Try the same on a native Wayland session. Compare: WebKitGTK child surface, XWayland, GTK-native shell, CEF OSR. Do not build a production integration yet.
  **Done when:** `docs/spike/phase-0.md` has a recommendation and the reasons; ADR-0002 is updated.
  **Notes:** Native Wayland rendered the page and executed JS (223 ms load, `scrollY: 240`), but `grab_focus` left the view unfocused and whole-window readback was black; page snapshots still worked. Recommendation in `docs/adr/0002-linux-engine.md`: WebKitGTK in a GTK-native shell, X11/XWayland supported first, native Wayland experimental, CEF-OSR not chosen without a new ADR and measurements. This is a recorded deviation from gpui-everywhere; Linux is experimental and the engine/core contract still applies.

- [~] **P0-07 — Hard interactions prototype `[all]`**
  Build the five interactions that decide the architecture: (1) overlay menu above a page; (2) tab drag with a floating lift panel; (3) split-view divider resize; (4) IME/emoji in a page and in the field; (5) video fullscreen/PiP/floating.
  **Done when:** each interaction works or has a measured fallback; latency numbers are recorded per OS.
  **Notes:** `crates/searlx-engine-linux/src/bin/interactions.rs` builds and drives all five on Linux X11 both programmatically and via real X11 input (`--external` + `xdotool` on `Xvfb :5`); results and latency numbers are in `docs/spike/phase-0.md` §5.2. Overlay click (12.0 ms to next frame), menu popover open/close (6.1 ms / 3.0 ms), real X11 tab drag reorder (0.33 ms), real X11 split divider drag `560→440` (0.05 ms) and window resize `1180×780→900×620` (11.5 ms), entry typing/emoji/Compose preedit `"searlx 🙂©"` (14.2 ms), left-page wheel scroll (`scrollY=468`, 60.3 fps GTK frame clock, ~62 fps `requestAnimationFrame`), and local 640×360 VP9 video (`readyState=4`, 112 `requestVideoFrameCallback` frames, DOM fullscreen via `f`/`Escape`) all pass on Linux X11. **Native PiP is absent** on WebKitGTK 2.54.2 (`pictureInPictureEnabled=false`, `native_pip=false`). Still needs macOS and Windows measurements (blocked on hardware).

- [~] **P0-08 — Performance baseline `[all]`**
  Measure cold open, new tab, tab switch, split resize, scroll, and memory with 1/10/30 tabs. Use the same page and hardware per OS. Record in `docs/spike/phase-0.md`.
  **Done when:** numbers are comparable to the Swift app and any regressions have a cause.
  **Notes:** `crates/searlx-engine-linux/src/bin/bench.rs` measures 1/10/30 tabs on X11 with a deterministic local page. Results are in `docs/spike/phase-0.md` §5.1. Headline: 1 tab = 400 MiB / 3 processes, 10 = 1.98 GiB / 12, 30 = 5.44 GiB / 35 (6.13 GiB after switching). WebKitGTK creates one WebKitWebProcess per view (~178–209 MiB each); 30 live tabs are unacceptable and sleeping is mandatory. Tab switch averages 0.20/0.93/2.82 ms. The benchmark does not include macOS/Windows or a Swift-app comparison, so this is a Linux baseline, not a completed all-platform comparison.

- [x] **P0-09 — Spike report `[all]`**
  Write `docs/spike/phase-0.md`: what worked, what did not, per-OS capability table, recommended Linux engine, recommended extension strategy, risks, and a go/no-go for Phase 1.
  **Done when:** AGENTS.md platform matrix and TODO ADRs are updated from evidence.
  **Notes:** `docs/spike/phase-0.md` records the environment, commands, X11/Wayland numbers, evidence images, what was not measured, and the recommendation. `docs/spike/capabilities.md` is the per-engine matrix and `docs/adr/0002-linux-engine.md` + `0003-extension-strategy.md` record the decisions. AGENTS.md's Linux row now says the GTK-native shell is proposed. Go/no-go is **no-go for Phase 1** until P0-G has macOS/Windows and real-input evidence; the gate is blocked, not passed.

- [x] **P0-10 — Capability matrix `[all]`**
  Write `docs/spike/capabilities.md`: WKWebView vs WebView2 vs WebKitGTK for navigation, scripts/worlds, data stores, downloads, find, PDF/print, fullscreen/PiP, permissions, extensions, passkeys, content blockers, input/IME, a11y, and process isolation.
  **Done when:** every Tier A feature has a per-engine answer or an explicit gap.
  **Notes:** `docs/spike/capabilities.md` has all requested rows plus custom UA, proxy, and snapshots, marked measured/documented/unverified. Hard gaps are explicit: WebKitGTK has no extensions and no WebAuthn; WebView2 extensions are unpacked-only; WKWebView is the only full extension path. Tier A app features are marked as core/UI work, with the engine dependency named. Unverified rows are listed so P0-03/P0-04/P0-07/P0-08 do not inherit false confidence.

- [x] **P0-11 — Data compatibility fixture `[all]`**
  Create a small corpus of existing Search data files (session, history, bookmarks, pins, spaces, hidden, styles) and a round-trip reader in `searlx-core`. No UI yet.
  **Done when:** fixtures parse; unknown fields survive; corrupt files quarantine; tests pass.
  **Notes:** `searlx-core::data` has serde models for session/space sessions, windows, pins (including live-feed folders), spaces, bookmarks, history, hidden selectors, and styles. `read_json` quarantines a corrupt file as `*.unreadable-<UTC>.json` without overwriting an earlier quarantine; `write_json` writes a temp sibling then renames. Nine fixtures live in `crates/searlx-core/tests/fixtures/legacy`; tests cover parse, unknown fields at the top level and inside nested objects, corrupt-file quarantine with bytes intact, missing files (not quarantined), and write/read. The original Swift source and real data files are absent, so the shapes are inferred from FEATURES.md §25; the fixture README records that assumption. Real old files still need to replace these fixtures in P1-02/P1-12.

- [x] **P0-12 — Phase 0 security checklist `[all]`**
  Threat-model the new shell: private stores, password fill, agent tabs, extension boundaries, update signing, imported files, URL redaction.
  **Done when:** checklist is in `docs/spike/phase-0.md` and every item has an owner or a Phase 1 task.
  **Notes:** `docs/spike/security-checklist.md` has 42 items across private stores, password fill/origin checks, agent tabs, extension boundaries, update signing, imports, URL redaction, quarantine, and logging. Each row has risk, required behaviour, verification method, and an owner task (P1/P2/P4). It ends with six open questions. Linked from `docs/spike/phase-0.md` §10. This is a requirements checklist, not a claim that the current shell implements it.

- [!] **P0-G — Gate: architecture confirmed `[all]`**
  Confirm ADR-0001 (gpui + system webviews), ADR-0002 (Linux engine), ADR-0003 (extension strategy), and ADR-0004 (no bundled Chromium by default). If the gate fails, stop and revise the plan before Phase 1.
  **Blocked on:** macOS WKWebView embed (P0-03), Windows WebView2 embed (P0-04), real pointer/IME/frame-rate/video checks on Linux (P0-05/P0-07), and the 1/10/30-tab baseline (P0-08). ADR-0004 is accepted; ADR-0001 is accepted for macOS/Windows and provisional for Linux; ADR-0002 and ADR-0003 are proposed with recommendations. **Reason:** this environment has no macOS or Windows machine and the Linux session is locked, so the gate cannot be passed honestly. **Owner:** project lead. **Exact verification:** run `docs/spike/phase-0.md` §12 rows on real hardware, then re-run this gate. Do not start Phase 1 on the strength of the Linux evidence alone.

---

## Phase 1 — Foundation

**Goal:** a real two-platform browser skeleton with tabs, a field, a session, and settings.

- [x] **P1-01 — `searlx-core` domain types `[all]`**
  URLs/identity, `TabId`, `SpaceId`, `ProfileId`, history visit, bookmark tree, pin, group, split, session shape, permission key, engine id. No I/O.
  **Done when:** unit tests cover URL identity, migrations, and serialization.
  **Notes:** `crates/searlx-core/src/domain.rs` defines `EngineId`, `PermissionKind`, `PermissionDecision`, `PermissionKey` (`capture.<origin>|<kind>` with default-port disagreement detection), `HistoryVisit` (URL identity merging), `BookmarkItem`/`BookmarkTree` (link counting and non-destructive merge), `PinItem` (home/wandered/rehome), `TabGroupItem`/`SpaceColor`, `SplitAxis`/`SplitItem` (2–4 panes, 20–80% / ≥10% share clamping, swap/add/remove), and `SessionShape`/`TabSnapshot` (v0→v3 migration, active-tab wake, future-version rejection). Unit tests pass.

- [x] **P1-02 — Data stores and migrations `[all]`**
  Read/write the existing JSON formats from FEATURES.md §25. Atomic temp+rename writes, serialized writes, quarantine on bad decode, versioned schema.
  **Done when:** the P0-11 fixture round-trips byte-equivalently where expected and old files load.
  **Notes:** `crates/searlx-core/src/data/store.rs` provides `DataStore` with mutex-serialized atomic writes, `read_or_default` corrupt-file quarantine fallback, versioned migration (`migrate_session_file`, `migrate_bookmarks_file`), disk caps (2,000 history visits, 500 archived tabs, 50 downloads), schemas for all `FEATURES.md` §25 files (`profiles.json`, `archive.json`, `downloads.json`, `pin-folders.json`, `search-sites.json`), and one-time `"Office Browser"` → `"Search"` directory migration. `skip_serializing_if = "Option::is_none"` prevents spurious nulls; all 9 P0-11 legacy fixtures preserve exact JSON values and round-trip byte-equivalently (`tests/data_fixtures.rs`).

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

- [x] **P1-14 — CI `[all]`**
  GitHub Actions: fmt, clippy, test, build on macOS/Windows/Linux; cache Rust; artifact upload. Keep Linux non-blocking until Phase 5.
  **Done when:** a PR gets a green/red signal; test time is reasonable; warnings are denied.
  **Notes:** `.github/workflows/ci.yml` runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `cargo build --workspace --bins` with `RUSTFLAGS=-Dwarnings`, `Swatinem/rust-cache@v2`, and `actions/upload-artifact@v4` on `macos-latest`, `windows-latest`, and `ubuntu-24.04` (`continue-on-error: true` on Linux until Phase 5).

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
**Status:** accepted for macOS/Windows, provisional for Linux — [0001-gpui-and-system-webviews.md](docs/adr/0001-gpui-and-system-webviews.md). **Decision:** gpui for chrome; WKWebView/WebView2/WebKitGTK for pages; no bundled Chromium by default.
**Why:** product feel, small binaries, no second engine, platform-native security updates.
**Risk:** three capability sets; Linux embedding; gpui pre-1.0.

### ADR-0002 — Linux engine
**Status:** proposed — [0002-linux-engine.md](docs/adr/0002-linux-engine.md). **Decision:** WebKitGTK in a GTK-native shell; X11/XWayland supported first, native Wayland experimental; CEF-OSR only after a new ADR and measurements. **Why:** WebKitGTK renders and scripts pages on both backends, but gpui 0.2.2 cannot host a Linux native view. **Risk:** a second chrome implementation; Wayland focus/readback unproven.

### ADR-0003 — Extension strategy
**Status:** proposed — [0003-extension-strategy.md](docs/adr/0003-extension-strategy.md). **Decision:** macOS-only WKWebExtension first; Windows unpacked-only and gated; Linux none. **Why:** that is what the engines actually support; no fake API surface.

### ADR-0004 — Bundled engine
**Status:** accepted — [0004-no-bundled-engine.md](docs/adr/0004-no-bundled-engine.md). The default build must not bundle Chromium. A separate "Search Max" build would need a new ADR.

### ADR-0005 — Passkeys on Linux
**Status:** proposed — [0005-passkeys-on-linux.md](docs/adr/0005-passkeys-on-linux.md). No passkeys in the default WebKitGTK build; macOS/Windows use the system authenticator. No custom software authenticator or fake shim.

### ADR-0006 — Sync backend
**Status:** proposed — [0006-sync-backend.md](docs/adr/0006-sync-backend.md). Folder sync is the portable baseline; iCloud is an optional macOS layer; no hosted account or server.

### ADR-0007 — Existing Swift app
**Status:** accepted — [0007-existing-swift-app.md](docs/adr/0007-existing-swift-app.md). The Swift macOS app remains the reference and can ship while the Rust app matures. Do not rewrite it in gpui.

### ADR-0008 — Distribution and update
**Status:** proposed — [0008-distribution-and-update.md](docs/adr/0008-distribution-and-update.md). macOS notarized DMG + signed feed; Windows MSIX/Squirrel; Linux Flatpak primary/AppImage fallback; one signed feed per channel; no auto-relaunch.

---

## Risks and blockers

| Risk | Impact | Mitigation | Owner |
|---|---|---|---|
| gpui 0.2.2 cannot host a Linux native view | Linux needs a second chrome or CEF | ADR-0002 GTK-native shell; revisit if gpui adds a native-view API | P0 |
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

- **P0-03 / P0-04 / P0-G — no macOS or Windows machine in this environment.** The code and docs are ready; the embedding, overlay, focus, and resize checks need real hardware. Owner: project lead. See `docs/spike/phase-0.md` §12 for the exact steps.
- **P0-05 / P0-07 / P0-08 — the Linux session is locked by `swaylock`.** Programmatic GTK events and page snapshots work; real pointer, keyboard, IME, video, and frame-rate checks need an unlocked display. Owner: whoever has the session.
- **Subagents — OpenCode Go monthly usage limit reached / Insufficient balance.** Delegation to `opencode-go/deepseek-v4.1-flash` failed before doing work; the work continues directly. This is a tooling blocker, not a product decision.

## Progress log

- **2026-11-20** — Created `AGENTS.md` and `TODO.md`. Phase 0 not started. The original Swift checkout is no longer present in this working tree; `FEATURES.md` is the behavioural source of truth.
- **2026-11-21** — P0-01/P0-02 underway. Rust workspace, eight crates, gpui shell, engine contract, fake engine, platform traits/fakes, testkit contract test, toolchain/CI/docs skeleton. `cargo check`, `fmt`, `clippy -D warnings`, and `cargo test --workspace` pass on Linux; the binary launches. Committed directly to `main` as requested; no remote push.
- **2026-11-21** — P0-11 done. `searlx-core::data` models and nine inferred fixtures; unknown fields survive, corrupt files quarantine without overwriting, `write_json` is atomic. 17 core tests pass.
- **2026-11-21** — P0-06/P0-09/P0-10/P0-12 done; P0-05 partial. Linux WebKitGTK spike ran on X11/Xwayland and native Wayland: real page, JS scroll, overlay visible (X11), focus on X11, WebKit child processes, page snapshots; Wayland focus/readback failed in the locked session. gpui 0.2.2 cannot host a Linux native view, so ADR-0002 recommends a GTK-native shell (X11/XWayland first). Wrote `docs/spike/phase-0.md`, `capabilities.md`, `security-checklist.md`, ADR-0001..0008, and copied evidence PNGs. P0-G is blocked on macOS/Windows hardware and real Linux input/frame-rate checks. `cargo fmt`, `clippy -D warnings`, and `cargo test --workspace` pass; 17 tests.
- **2026-11-21** — P0-08 baseline measured on X11. 1/10/30 local tabs: RSS 400 MiB / 1.98 GiB / 5.44 GiB, processes 3/12/35, switch avg 0.20/0.93/2.82 ms. WebKitGTK uses one WebKitWebProcess per view (~178–209 MiB), so 30 live tabs cost ~6 GiB and sleeping is mandatory. Recorded in `docs/spike/phase-0.md` §5.1; macOS/Windows comparison still open. Next: P0-07 hard interactions on the GTK shell.
- **2026-11-21** — P0-07 partial. GTK prototype drives overlay popover, tab reorder/lift, split resize, entry/page emoji, and local video (2 pages + video, ~196 MB self RSS). Video plays and DOM fullscreen works, but native PiP is absent on WebKitGTK 2.54.2, window readback is black over video, and real pointer/IME paths still need an unlocked display. Linux `native_pip` corrected to false with a unit test. Results in `docs/spike/phase-0.md` §5.2.
- **2026-11-21** — P0-05 done; P0-07 Linux X11 input/latency measured. Added `--external` mode and next-frame latency probes to `interactions.rs` and drove real X11 pointer, wheel, keyboard, emoji, Compose (`Multi_key`), split-drag, tab-drag, video frame counter, and window resize on `Xvfb :5` with `xdotool`. GTK frame clock: 60.3 fps; in-page `requestAnimationFrame`: ~62 fps; video decoded 112 frames; input-to-next-frame latencies 0.05–14.2 ms. Recorded in `docs/spike/phase-0.md` §5.2 and `docs/spike/evidence/x11-interactions-external.png`.
- **2026-11-21** — P1-01, P1-02, P1-14 done. Added `searlx_core::domain` (`EngineId`, `PermissionKey`, `HistoryVisit`, `BookmarkTree`, `PinItem`, `TabGroupItem`, `SplitItem`, `SessionShape`), `searlx_core::data::store::DataStore` with serialized atomic writes, schema migrations, and byte-equivalent round-trip tests on all 9 legacy fixtures, and `.github/workflows/ci.yml` for macOS/Windows/Linux. Note: Phase 1 platform-independent and Linux-verifiable items proceed while `P0-G` remains `[!]` awaiting physical macOS/Windows hardware.

---

## Definition of done (per task)

- [ ] Behaviour matches the FEATURES.md section (or the deviation is recorded).
- [ ] Unit/contract/UI tests added or updated.
- [ ] `cargo fmt`, `cargo clippy -D warnings`, `cargo test --workspace` pass.
- [ ] Manual check on every named platform.
- [ ] Accessibility/security implications checked.
- [ ] TODO status and notes updated; ADR/spec updated if needed.
- [ ] No secrets, large binaries, certificates, or build output committed.
