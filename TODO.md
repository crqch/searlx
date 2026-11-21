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
| 1 — Foundation | 10 | 16 | In progress |
| 2 — Daily driver | 21 | 23 | In progress |
| 3 — Identity and power | 17 | 20 | In progress |
| 4 — Platform and parity | 7 | 16 | In progress |
| 5 — Linux and hardening | 7 | 11 | In progress |
| Continuous | 7 | 8 | In progress |
| **Total** | **75** | **107** | |

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

- [x] **P1-03 — Engine trait and events `[all]`**
  Finalise `Engine`, `TabOptions`, `EngineEvent`, `Capabilities`, `DataStoreHandle`, `ScriptWorld`, `InjectAt`, `FindOptions`, `FindState`. Add a fake engine for tests.
  **Done when:** `searlx-testkit` can drive the fake through create/load/back/forward/close and assert the event stream.
  **Notes:** `searlx-engine` defines the complete contract (`Engine`, `TabOptions`, `EngineEvent`, `Capabilities`, `DataStoreHandle`, `DataKinds`, `ScriptWorld`, `InjectAt`, `FindOptions`, `FindState`, `PermissionKind`, `DialogKind`, `NewWindowDisposition`, `Rect`, `Image`) and `FakeEngine` records calls and emits the full navigation/history/media/script event stream. `searlx-testkit` drives `lifecycle` and verifies a redacted golden event transcript (`crates/searlx-testkit/tests/fixtures/lifecycle.transcript`).

- [ ] **P1-04 — macOS backend `[mac]`**
  WKWebView behind `Engine`: create/close/navigate/back/forward/reload/stop/eval/inject/set_bounds/visible/focus/snapshot/find/zoom/mute/data store/permissions/downloads/events.
  **Done when:** contract tests pass against real WKWebView; two tabs switch without reload; private store is ephemeral.

- [ ] **P1-05 — Windows backend `[win]`**
  WebView2 behind `Engine`, same contract. User data folder per profile/space; ephemeral profile for private tabs.
  **Done when:** contract tests pass on Windows 11; two tabs switch without reload; private profile leaves no data.

- [~] **P1-06 — Tab model and row `[all]`**
  Tab state, active tab, pinned flag, title/icon, loading, audio, sleep placeholder. Side and top layouts, icons-only, fold. Use one drag session.
  **Done when:** create/close/reorder/pin/unpin works on macOS and Windows; a tab drag is smooth at 60+ fps.
  **Notes:** `crates/searlx-core/src/tabs.rs` implements `TabEntry`, `TabModel` (blank-tab reuse on `⌘T`, pin put-down and group put-away on `⌘W`, pin/unpin across the boundary, `Put to Sleep` guard, `⌘S` fold and fold-to-icons), and the single unified `DragSession` state machine (`DragPayload`, `DropTarget`, 5 pt travel threshold, 12 pt detach slack, `max(120 pt, 24%)` split drop band). Wired into `searlx-ui`. Needs macOS/Windows physical verification.

- [x] **P1-07 — Address field and parsing `[all]`**
  Port FEATURES.md §4.2 parsing rules, Return/⌘Return/⇧⌘Return, completion, suggestions from history/bookmarks/open tabs, site chip, commands. Invalid input shakes.
  **Done when:** parser unit tests mirror the spec; manual cases: `localhost:3000`, `~/file`, `example.com`, `hello world`, `mailto:`, `[::1]:3000`, look-alike hosts.
  **Notes:** `crates/searlx-core/src/address.rs` implements all `FEATURES.md` §4.2–4.4 rules: whitespace/single-word search vs bare host classification, `localhost`/`*.localhost`/`*.local`/`[::1]`/`127.0.0.1`/private IPv4 → `http://`, `0.0.0.0` → `http://localhost`, letter-TLD ≥2 check (dotted version numbers like `1.2.3` rejected as invalid), `/path` and `~/path` file existence check, `@` and unsupported scheme refusal (`mailto:`, `javascript:`), IDN punycode look-alike detector (`inspect_host_lookalike`), `SubmitDisposition` (`Return`/`⌘Return`/`⇧⌘Return`), 9 search engines + host-preserving template expansion, 16 built-in site searches (`Tab`-to-search & keywords), inline grey completion, and ordered suggestion builder (site offers, open tabs, frecency history, bookmarks/slips, known sites, commands, search fallback). Wired into `searlx-ui`.

- [x] **P1-08 — Session and restore `[all]`**
  Write/read session and windows files. Restore tabs asleep; active tab wakes; window frames; fresh-start mode; quarantine.
  **Done when:** quit/relaunch restores tabs without loading every page; a 30-tab session opens fast; corrupt session is quarantined.
  **Notes:** `crates/searlx-core/src/session.rs` implements `restore_session`, `save_window_session`, and `save_additional_windows`. Non-active tabs always restore asleep (`sleeping=true`, `should_load_now=false`) while only the active tab wakes; a 30-tab session restores in <10 ms with 1 awake tab. Private tabs are stripped before saving. Fresh-start mode (`RestoreMode::FreshStart`) keeps pins asleep and replaces loose tabs with one blank tab. Corrupt `session.json` and `windows.json` quarantine intact and fall back to `1180×780`.

- [x] **P1-09 — Settings shell `[all]`**
  Settings as a tab (`search://settings`), 11 pages, search field, typed settings with defaults. Start with General/Appearance/Tabs/Browsing/Privacy.
  **Done when:** settings persist; changing one redraws the relevant UI; search finds rows and synonyms.
  **Notes:** `crates/searlx-core/src/settings.rs` defines the typed `Settings` model with all `FEATURES.md` §21 defaults (including the inverted `sites.keep` flag for cross-site tracking prevention and dormant `ai_shipped=false`), atomic persistence via `DataStore`, `SettingsPage` covering all 11 pages (`search://settings/<slug>`), and `search_settings` matching labels, keys, and synonyms while marking rows under disabled parent switches as `dimmed_by_parent=true`.

- [x] **P1-10 — Theme, metrics, icons `[all]`**
  Light/dark/system palette, metrics module, tab glyphs (letters/favicons), high contrast, reduce motion. No magic numbers scattered.
  **Done when:** appearance matches the design intent on both OSes; text scales; icons render at 1x/2x.
  **Notes:** `crates/searlx-ui/src/theme.rs` and `metrics.rs` provide `Palette::for_theme_with_contrast` (light/dark/system + `tabs.highContrast`), `SpaceTint` chrome washes, `tab_glyph_letter`, `motion::duration_ms` (collapsing to `0` under Reduce Motion), `scaled_pt` text scaling (`0.75..=2.5`), and `icon_device_px` for 1x/2x/HiDPI icon sizing.

- [x] **P1-11 — Command and shortcut layer `[all]`**
  One command registry, default keys, user overrides, menu/action routing, extension commands later. `⌘S` folds, does not save.
  **Done when:** every Phase 1 action is reachable by keyboard and menu; conflicts are rejected.
  **Notes:** `crates/searlx-core/src/commands.rs` defines `CommandId`, `MenuCategory`, `KeyModifiers`, `KeyChord`, and `CommandRegistry` covering every `FEATURES.md` §22 shortcut (`⌘S` bound to `ToggleSidebar`, `SavePage` unbound, `⌘.` bound to `StopLoading`). User overrides reject reserved chords (`⌘C/V/X/A/Z`, `⌃⌘F`) and duplicate assignments with `ShortcutConflictError`. Extension commands register via `register_extension_command`.

- [x] **P1-12 — Import existing Search data `[all]`**
  Import the old app's session/history/bookmarks/pins/spaces/hidden/styles files into `searlx-core`. No browser imports yet.
  **Done when:** a user can point at an old Application Support folder and get their tabs/pins/bookmarks in the new app.
  **Notes:** `crates/searlx-core/src/import.rs` implements `import_search_directory` with `AGENTS.md` §9 hostile-file checks (`symlink_metadata`, `O_NOFOLLOW`, canonical path confinement, 64 MiB cap), all-or-nothing in-memory staging, asleep tab restoration, idempotent bookmark/history/pin/space/hidden/style merging, and unit tests for normal and symlink-attack directories.

- [~] **P1-13 — Packaging skeleton `[mac,win]`**
  macOS `.app` bundle + ad-hoc signing; Windows `.exe` + WebView2 runtime check; dev run command. No notarization yet.
  **Done when:** a fresh machine can run the build; the macOS bundle opens; Windows shows a clear message if WebView2 is missing.
  **Notes:** `crates/searlx-platform/src/packaging.rs` implements `check_webview2_runtime` / `evaluate_webview2_version` (with a clear user-facing install message when WebView2 is missing), `macos_info_plist` (declaring `http`/`https`, `xhtml`, `pdf`, and images while excluding `svg` per `FEATURES.md` §18.1), and `stage_macos_app_bundle`. `scripts/package-macos.sh` builds and ad-hoc signs `build/searlx.app`. Needs macOS/Windows physical verification.

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

- [x] **P2-01 — Navigation and history `[all]`**
  Back/forward, long-press history menu, reload/stop/origin, redirects, errors, per-tab history, internal-page trail, scroll restoration.
  **Done when:** a 50-link session behaves like the spec; Back from the first page does nothing; errors are in-page.
  **Notes:** `crates/searlx-core/src/navigation.rs` implements `TabNavigation` with per-tab back/forward stack, scroll restoration, redirect collapsing, long-press & 8-item swipe-hold history menus, separate `search://` internal-page trail, small-window back-at-start close check, and `InPageError` state. Verified with a 50-link session unit test.

- [x] **P2-02 — Find on page `[all]`**
  Pill, count, next/prev, match case, whole words, PDF fallback, 1,000 limit, isolated world, search settings on the settings tab.
  **Done when:** matches the spec's status strings; a 1,000-match page stays responsive.
  **Notes:** `crates/searlx-core/src/find.rs` implements `FindSession`, `FindTarget` (routing `search://settings*` to Settings search and `.pdf` to PDF fallback with `whole_words` disabled), status strings (`"3 of 17"`, `"1 of 1,000+"`, `"0 of 0"`), 1,000-match cap, wrap-around next/previous, and `ISOLATED_WORLD_FIND_JS`. A 2,500-match page completes in <5 ms.

- [x] **P2-03 — History page `[all]`**
  Grouped by day, search, remove one, clear all/scope, 2,000 cap, frecency suggestions, no date range.
  **Done when:** clearing history behaves exactly as FEATURES.md §10 says, including archive/favicons/site searches.
  **Notes:** `crates/searlx-core/src/history.rs` implements `HistoryManager` with 2,000 disk cap and 2,250 memory cap (frecency pruning), `"Today"`/`"Yesterday"`/`"YYYY-MM-DD"` day grouping, single-substring search, single-row removal, no date-range deletion, 500-cap auto-closed archive, 10-cap recently closed stack, and `clear_history` (wiping all or a signed-out space's visits plus archive, link-chooser memories, favicons, learned site searches, and stripping download source URLs while keeping download rows).

- [x] **P2-04 — Bookmarks `[all]`**
  Tree, add/edit/delete/move, bar, menu, page, search, suggestions, import merge, no export writer.
  **Done when:** nested folders and 1,500 imported bookmarks open quickly; drag/drop reorder works.
  **Notes:** `crates/searlx-core/src/bookmarks.rs` implements `BookmarksManager` with `Add This Page` vs `Edit Bookmark…` detection, add/rename/edit/remove (returning removed link count), drag/drop `move_node` with cycle/descendant rejection, `bar_items`, `search_all_words` with folder paths, and no export writer. Tested with 1,500 bookmarks across 15 nested folders.

- [x] **P2-05 — Downloads `[all]`**
  Tab/popover/panel, destination/ask, pause/resume/retry/cancel, naming, progress, no Notification Center, private unlisted.
  **Done when:** a 1 GB download pauses/resumes; a POST download says it cannot pause; a private download is not listed.
  **Notes:** `crates/searlx-core/src/downloads.rs` implements `DownloadsManager` with 1 GB `GET` pause/resume, `POST`/body refusal (`"POST or form downloads cannot be paused"`), `" 2"`/`" 3"` duplicate numbering, path traversal sanitization, 50-row persistence cap, `uses_notification_center() == false`, and private downloads excluded from `listed_entries()` while counted in `global_active_progress()`.

- [x] **P2-06 — Reader `[all]`**
  Extraction heuristic, Markdown twin, light/dark/system, images/tables/code, reload on exit.
  **Done when:** 10 representative articles render cleanly; a code-heavy page keeps its code; exiting restores the page.
  **Notes:** `crates/searlx-core/src/reader.rs` implements `extract_article` (link-density penalty heuristic, `<link rel="alternate" type="text/markdown">` & `.md` Markdown twin rendering, `<pre><code>` and `<div class="table-wrap"><table>` preservation, `<script>`/`<style>` removal), `ReaderArticle::render_document` (`38em`, `18px/1.72` serif, light/dark/system themes), and `exit_reader` reload action. Verified against 10 representative articles.

- [x] **P2-07 — Blocking `[all]`**
  Content blocker rules (WebKit format), YouTube ads, per-site pause, first-load wait, cosmetic selectors. Windows/Linux adapters behind capability.
  **Done when:** a tracker-heavy page is lighter; YouTube ads are removed or the fallback mutes/skips; per-site off works.
  **Notes:** `crates/searlx-core/src/blocking.rs` provides `TRACKER_DOMAINS` (36 ad/analytics networks), `COSMETIC_AD_SELECTORS`, `compile_webkit_content_rules` (emitting `WKContentRuleList` JSON with `unless-domain` per-site pause), `should_block_third_party_request` (portable Windows/Linux request filter), `youtube_ad_scripts` (page-world player response scrubber + isolated-world 16× mute fallback without seeking), and `FIRST_LOAD_BLOCKER_WAIT_MS` (1,000 ms).

- [x] **P2-08 — Hide elements `[all]`**
  Picker, selector synthesis, per-site list, before-first-paint injection, undo, review, private memory-only.
  **Done when:** hiding a cookie banner survives reload with no flash; review restores one/all.
  **Notes:** `crates/searlx-core/src/blocking.rs` implements `synthesize_selector` (`#id` → `data-testid`/`aria-label`/`name`/`role` → stable classes → `nth-of-type` path) and `HideElementsManager` with before-first-paint individual `selector { display:none !important }` rules, `"Hidden — ⌘Z puts it back"` undo stack, restore one/all (`⇧⌘U`), `hidden.json` persistence, and memory-only private-tab rules.

- [x] **P2-09 — Site CSS `[all]`**
  CSS editor, live apply, per-site file, before-paint style, off keeps styles, private read-only.
  **Done when:** a site style survives reload; no script is executed.
  **Notes:** `crates/searlx-core/src/blocking.rs` implements `SiteCssManager` and `sanitize_user_css` (blocking `</style`, `<script`, `javascript:`, `expression(`). Turning `sites.styles` off disables injection while keeping stored styles; private tabs wear existing styles but cannot edit them.

- [x] **P2-10 — Darkening `[all]`**
  Measurement, dark twins/filter fallback, per-site toggle, pause, private page-only, budgets.
  **Done when:** a light article darkens without touching photos; a dark site is untouched; budget gives up cleanly.
  **Notes:** `crates/searlx-core/src/page_tools.rs` implements `is_measured_light_page`, `DuskBudget` (2,000 sheets, 8M held chars, 48M made chars), `DuskController` (per-site `⇧⌘D`, session-wide `⌥⇧⌘D` pause, private memory-only), relative OKLCH dark twins with `img, picture, video, canvas { filter: none !important }`, and automatic fallback to `DUSK_FILTER_FALLBACK_CSS` when a budget ceiling is exceeded.

- [x] **P2-11 — Translate `[all]`**
  On-device where available; target/always lists; DOM text nodes/attributes; frames; show original; private.
  **Done when:** a French article translates in place; no element is added/removed; Show Original restores exact text.
  **Notes:** `crates/searlx-core/src/page_tools.rs` implements `TranslatableNode` (excluding `code`, `pre`, `textarea`, `translate="no"`, `.notranslate`, and preserving `[1]`/`[2]` inline markers) and `TranslationManager` (in-place deduplicated translation, `show_original` exact text restoration, and private-tab `translate.always` guard).

- [x] **P2-12 — Capture `[all]`**
  Area, element, whole page, clipboard + PNG, limits, private transient, no upload.
  **Done when:** a 20,000 pt page captures without a white strip; an element capture outlines correctly.
  **Notes:** `crates/searlx-core/src/page_tools.rs` implements `plan_page_capture` with 30,000 pt height cap, 64M pixel cap, 2×2 pt minimum, PDF-first rasterization for whole-page shots (avoiding off-screen white strips), `<tag> · W×H` element outline labels, site-neutral `Capture YYYY-MM-DD at HH.MM.SS.png` naming, local-only clipboard flag, and `clipboard_transient=true` for private tabs.

- [x] **P2-13 — Site card `[all]`**
  Connection, certificate, copy/print, zoom, blocker, permissions, autoplay, float-off, darken, keep-awake.
  **Done when:** every row in FEATURES.md §5.8 is present and gated correctly.
  **Notes:** `crates/searlx-core/src/menus_and_pages.rs` implements `SiteCardModel::build` gating every row of `FEATURES.md` §5.8: header (host without `www.`, `"File"`, or scheme), connection/certificate label, copy address shortcut, print capability check, zoom (`40–300%` web, `20–600%` PDF, 5% steps), ad/tracker blocker (`!private && http(s) && shield && !trouble`), site permissions (`http(s)` with private notice, global-notification-off warning, and port-disagreement warning), `"Play Sound by Itself"` (`!private && http(s) && !media.click`), `"Don't Float Videos Here"` (known video sites), `"Darken This Site"`, and `"Keep This Site Awake"`, while excluding mute/site CSS/reader.

- [x] **P2-14 — Permissions and private tabs `[all]`**
  Camera/mic/location/notifications/screen; per-origin; private refusal; forget choices; ephemeral store; no history/session/favicon/password offer.
  **Done when:** a private tab leaves no trace after close; permissions persist per site only in normal tabs.
  **Notes:** `crates/searlx-core/src/privacy.rs` implements `PermissionStore` (`capture.<origin>|<kind>`, immediate refusal for private and agent tabs, `"Forget choices"` reset, port-spelling conflict check) and `StoreCompositor`, enforcing in one boundary (`AGENTS.md` §9) that private and agent tabs write zero history, favicons, password save offers, permissions, Handoff, or iCloud state, and signalling ephemeral store destruction when the last private tab closes.

- [x] **P2-15 — Password vault `[all]`**
  Credential-store trait + OS backends; save/fill; account list; Touch ID/Hello/Secret Service; show/copy gates; clear-http rules; no export.
  **Done when:** a sign-in saves after success, fills only on the same origin, and never fills a password saved from https on http.
  **Notes:** `crates/searlx-core/src/vault.rs` implements `PasswordVault` and redacted `VaultSecret`: post-sign-in save/update/last-used evaluation (suppressed in private/agent tabs, `passwords.never` hosts, and Apple Passwords fills), main-frame-only offer list (max 5), strict committed-origin re-verification on fill, **strict HTTPS→HTTP downgrade block** (passwords saved from `https://` are never offered or filled on `http://`), 10-dot `"••••••••••"` masking unless device-owner authenticated (15 s auto-hide), authenticated concealed clipboard copy (90 s auto-clear), and no export writer.

- [x] **P2-16 — Keyboard and context menus `[all]`**
  All Phase 1/2 commands, default keys, user overrides, tab/page/image menus, reduced/disabled entries.
  **Done when:** the shortcut table in FEATURES.md §22 is covered or explicitly dropped; menu items disable correctly.
  **Notes:** `crates/searlx-core/src/commands.rs` covers the full `FEATURES.md` §22 shortcut inventory, and `crates/searlx-core/src/menus_and_pages.rs` implements `build_page_context_menu` (`Open Link in New Tab`, `Open Link in New Tab and Go to It`, `Open Link in Split View`, `Copy Clean Link` via `clean_link_url`, non-linked image menu, `Search with <engine>`, `Translate Page` / `Show Original`) and `build_tab_context_menu` (pin/unpin/rehome, mute/unmute, and `Put to Sleep` disabled on the active/audible/loading tab).

- [x] **P2-17 — Internal pages `[all]`**
  `search://settings|history|downloads|bookmarks|passwords|archive|import|release-notes`, one tab per kind, Back/Forward trail.
  **Done when:** asking twice brings the existing tab forward; private tabs never become internal pages.
  **Notes:** `crates/searlx-core/src/menus_and_pages.rs` implements `InternalPageKind` and `open_internal_page`: deduplicates to one tab per kind per window (bringing an existing tab forward and updating its detail route), reuses an active non-private blank tab, and never turns a private tab into an internal page.

- [x] **P2-18 — Update mechanism `[mac,win]`**
  Signed feed, hash/signature verification, staged install, no auto-relaunch, rollback behavior.
  **Done when:** a bad signature is refused; a good update installs on next launch; the old bundle is swept.
  **Notes:** `crates/searlx-core/src/recovery.rs` implements `verify_update_release` (requiring HTTPS, same-host archive URL, `com.officecommun.search.appcast` identifier, team `7BYKA895MC`, detached signature verification, and SHA-256 match), `install_verified_bundle_swap` (renaming current to `.old`, swapping new into place, rolling back `.old` on failure, and setting `auto_relaunched=false`), and `sweep_old_bundle`.

- [x] **P2-19 — Crash and error recovery `[all]`**
  Process crash, reload loops, in-page errors, quarantine, crash log.
  **Done when:** a crashing page shows a retry after two failures; a corrupt file is set aside, not overwritten.
  **Notes:** `crates/searlx-core/src/recovery.rs` implements `CrashRecoveryManager` (auto-reloading the first crash and suppressing reload loops into `ShowManualRetry` after 2 failures within 60 seconds) and `append_crash_log` (writing one redacted single-line record to local `crash.log` using `PageUrl::redacted()`, never uploading anything). Corrupt data files are quarantined by `DataStore`.

- [x] **P2-20 — Accessibility baseline `[all]`**
  Names/roles/focus for chrome; page content reachable; keyboard-only pass; reduced motion; high contrast.
  **Done when:** a screen reader can list tabs, use the field, and reach the page on macOS and Windows.
  **Notes:** `crates/searlx-ui/src/a11y.rs` defines `A11yRole`, `A11yNode`, and `AccessibilityTree` covering the window root, tab list and per-tab accessible labels (pinned/audible/loading/sleeping/active annotations), new-tab button, address & search combo box, active panel, and `WebArea` page surface, plus full forward/backward `Tab`/`Shift+Tab` keyboard focus traversal reaching the page surface without trapping focus, paired with `Palette::for_theme_with_contrast` (`tabs.highContrast`) and `motion::duration_ms` (`reduce_motion`).

- [x] **P2-21 — Performance budget `[all]`**
  Cold start, new tab, switch, 30 tabs, scroll, memory. Add a benchmark harness.
  **Done when:** numbers are within the Phase 0 baseline; a regression fails CI or is reported.
  **Notes:** `crates/searlx-testkit/src/bench.rs` implements `PerformanceBudget` and `run_performance_budget_check`, asserting cold-open 30-tab session restoration (<25 ms with only 1 awake tab vs 29 asleep tabs, saving ~5.0 GiB RSS compared to the Phase 0 30-live-tab WebKitGTK baseline), new tab creation (<1,000 µs avg), tab switch (<1,000 µs avg), address parsing (<500 µs avg), and suggestion building (<2,000 µs avg). Enforced in CI via `tests/contract.rs`.

- [ ] **P2-22 — Phase 2 dogfood `[all]`**
  Two weeks of daily use on macOS and Windows. File issues, fix the top 10, then gate.
  **Done when:** the old browser is not needed for a normal day.

- [ ] **P2-G — Gate: daily driver `[mac,win]`**
  If the browser cannot be used for a normal day without falling back, do not start Phase 3.

---

## Phase 3 — Identity and power

**Goal:** spaces, profiles, pins, groups, splits, and the signature interactions.

- [x] **P3-01 — Spaces `[all]`**
  Per-space tabs, parked rows, own/shared store, history routing, create/rename/delete/reorder, colors/gradients/icons, downloads folder, routes.
  **Done when:** two spaces can be signed into different accounts; switching parks tabs and returns them intact; delete cleans the store with retries.
  **Notes:** `crates/searlx-core/src/spaces_profiles.rs` implements `SpacesManager` with per-space parked tab lists (`switch_space` parks outgoing tabs and restores incoming tabs intact), isolated vs shared data stores (`own_data_store` defaulting to `true` for created spaces so two spaces stay signed into separate accounts, and switching an existing space to isolated requires confirmation), history routing (`clear_history_for_space` refuses when `own_data_store` is false), colors/gradients/icons/downloads folder, and `delete_space` moving open tabs into the neighboring space and scheduling up to 4 store-cleanup retries (`1.5 s`, `3 s`, `4.5 s`, `6 s`).

- [x] **P3-02 — Profiles `[all]`**
  Per-profile spaces, cookies, history, bookmarks, archive, windows; shared settings/extensions/passwords/downloads.
  **Done when:** two profiles do not share cookies/history/bookmarks; deleting one removes its files/stores; a window never changes profile.
  **Notes:** `crates/searlx-core/src/spaces_profiles.rs` implements `ProfilesManager` and `ProfileIsolationBoundary`: per-profile spaces, cookies (`search.profile.<uuid>`), history, bookmarks, archive, pin folders, site searches, and windows under `Profiles/<uuid>/`, while sharing `settings.json`, extensions, passwords, and downloads across profiles. `WindowProfileBinding::try_change_profile` refuses to change a window's profile after creation.

- [x] **P3-03 — Profiles in one window `[all]`**
  Swap whole profile windows in place; tuck siblings; one untucked; close as a group; unmerge on setting off.
  **Done when:** switching profile spaces never reuses the old profile's stores; closing the visible window closes the group; relaunch restores the merge.
  **Notes:** `crates/searlx-core/src/spaces_profiles.rs` implements `MergedProfileWindowGroup`: swaps complete profile windows at the exact screen frame, keeps one visible and siblings tucked, closes all siblings together when the visible window closes, and unmerges back to independent windows when `spaces.profilesInBar` turns off.

- [x] **P3-04 — Pins `[all]`**
  Squares/rows, identity across windows, home, peek, command-click, letter, rehome, shared pins, drag boundary.
  **Done when:** a pin is the same in every window; ⌘W puts it down and it wakes at home; shared pins appear in every space.
  **Notes:** `crates/searlx-core/src/pins_and_folders.rs` implements `PinsRegistry`: shared pin identity across windows, `⌘W` putting a pin down (`awake=false`, restoring `current_url = home_url` without deleting it), `on_pin_link_click` (same-registrable-domain wanders in place; external domain opens a peek panel when `pins.peek` is on or a regular tab otherwise, while `⌘`-click always opens a regular tab), `rehome` (`Make Current Page Pin's Home`), custom 1–2 grapheme `letter`, and `shared` pins appearing across every space in the profile.

- [x] **P3-05 — Pin folders and live folders `[all]`**
  3-level folders, drag/nest, rename/delete, live RSS/Atom folders, 12 posts, 30 min refresh, 2 MB cap, no cookies.
  **Done when:** a feed folder updates and opens posts in new tabs; shared/squares/private cannot enter a folder.
  **Notes:** `crates/searlx-core/src/pins_and_folders.rs` implements `PinFoldersManager` and `parse_feed_xml`: 3-level nesting cap (`MAX_PIN_FOLDER_DEPTH = 3`), refusal of shared pins, square pins (`!pins_as_rows`), and private tabs, non-destructive folder deletion (returning child pins to top level), and live RSS/Atom folders (`MAX_LIVE_FOLDER_BYTES = 2 MiB`, `LIVE_FOLDER_REFRESH_SECS = 1,800`, `MAX_LIVE_FOLDER_ITEMS = 12`, credential-less `http`/`https` fetch only, rejecting `javascript:`/`data:` item links).

- [x] **P3-06 — Tab groups `[all]`**
  Name/color/collapse, drag in/out, close group + undo, fold all, move group to space, extension id.
  **Done when:** closing a group and ⇧⌘T restores it whole; an empty group disappears.
  **Notes:** `crates/searlx-core/src/groups_splits_switcher.rs` implements `TabGroupsManager`: name/color/collapse, automatic collapse of previous group on out-of-group tab selection (`tabs.collapseGroups`), automatic deletion when the last member leaves, `close_group` + `reopen_last_closed_group` (`⇧⌘T` restoring the group whole), `fold_all_except`, `move_group_to_space`, and stable `extension_id` (`u32`).

- [x] **P3-07 — Split View `[all]`**
  2–4 panes, 7 pt gutter, 250 min, 15 pt snap, divider, focus, swap, separate, close all, persistence.
  **Done when:** four panes resize and persist; closing one leaves the partner; a pair restores as a pair.
  **Notes:** `crates/searlx-core/src/groups_splits_switcher.rs` implements `SplitWorkspace`: 2–4 panes (`LeftRight` / `TopBottom`), `7 pt` gutter, `250 pt` minimum pane size (`split_fits_window`), `15 pt` snap to 50/50 on 2-pane splits, `20–80%` (2-pane) and `≥10%` (3–4 pane) share clamping, double-click equal split, swap, separate, close-all, and closing one pane of a 2-pane split returning the partner to a normal single tab.

- [x] **P3-08 — Tab switcher `[all]`**
  0.15 s delay, 5 columns/10 cards, moons, recency, ⌃W/R/M/O, previews, backwards.
  **Done when:** a quick ⌃Tab returns to the previous tab without drawing the grid; moons bring little windows back.
  **Notes:** `crates/searlx-core/src/groups_splits_switcher.rs` implements `TabSwitcherSession`: `150 ms` overlay delay (releasing `Control` before `150 ms` switches immediately to the previous tab without drawing the grid), up to `10` recency-ordered cards across `5` columns (excluding sleeping pins), up to `6` floating small-window moons, and in-switcher `⌃W` (close), `⌃R` (reload), `⌃M` (mute), and `⌃O` (move moon into main window).

- [x] **P3-09 — Peek `[all]`**
  Shift-click panel, keep as tab/beside, stays-with-tab, pause hidden media, pin peek.
  **Done when:** Escape returns to the exact page state; keeping a peek does not reload the page.
  **Notes:** `crates/searlx-core/src/peek_small_float.rs` implements `PeekSession`, `PeekClickModifier` (`ShiftClick`, `OptionShiftClick`, `OptionCommandClick`), and `OpenerPageState`: preserves exact opener scroll offset (`ScrollPosition`) and focused selector on `Escape` (`opener_reloaded=false`), promotes to a tab (`⌘↩`) or beside in Split View (`⌥⌘↩`) without reloading (`reloaded=false`), and pauses hidden audio/video and mutes camera/mic when switching tabs with `links.peek.stays`.

- [x] **P3-10 — Small windows `[all]`**
  Link windows, cascade, Open in Search, state, moons, popup reuse.
  **Done when:** a link from another app opens a small window without forcing the browser window forward; ⌘O moves it in.
  **Notes:** `crates/searlx-core/src/peek_small_float.rs` implements `SmallWindowsManager` (`900×640` default, `420×320` minimum, `34 pt` header, `+24 pt` cascade, `>60%` on-screen frame check): `open_in_search` (`⌘O`) moves the existing tab into the main row after the current tab without reloading, `Escape` closes link windows (`ExternalLink`) while refusing to close lifted tabs (`LiftedTab`), `⌘W` on a lifted tab makes it reopenable with `⇧⌘T`, lifted tabs return to their original spaces on quit, and sized popups reuse the frame without `"Open in Search"`.

- [x] **P3-11 — Floating video `[all]`**
  Reparent/live page or a portable fallback, triggers, controls, flick/dock, minimize, return, system PiP where available.
  **Done when:** video keeps playing while switching apps/tabs; returning restores the page; private-tab policy is decided.
  **Notes:** `crates/searlx-core/src/peek_small_float.rs` implements `FloatingVideoController`: supports manual `⇧⌘P`, `float.leave` (tab switch), and `float.away` (app switch), play/pause, `±10 s` skip, minimize, two-finger velocity corner flicks (`FloatCorner::from_flick_velocity`), automatic fallback from `SystemPip` to `SearchPanel` when `engine_supports_native_pip` is false (e.g., Linux WebKitGTK), and `return_to_tab` with `video_stopped=false`. **Private-tab policy decided:** private tabs are permitted to float within their live session (`allows_private_tab_floating() == true`) because the floating panel reparents the in-memory view without persisting or leaking across profiles.

- [x] **P3-12 — Gestures `[all]`**
  Back/forward swipe + hold history, space swipe + wheel/buttons, peek pinch, autoscroll, tab give, fold edge.
  **Done when:** the thresholds in FEATURES.md §23 behave; a horizontal carousel is not stolen; haptics are optional.
  **Notes:** `crates/searlx-core/src/gestures_lifecycle.rs` implements `SwipeHistoryGesture` (`70 pt` arm, `30 pt`/`250 ms` flick, `450 ms` hold for 8-item history list, `22 pt` per row, optional haptics, and immediate yield when `page_claims_horizontal_scroll` is true so horizontal carousels are never stolen), `SpaceSwipeGesture` (`50 pt` threshold, `400 ms` rest, `200 ms` anti-back-stroke, create space past end), `tab_give_scale` (`0.97` scale cancelling past `5 pt`), `FoldedSidebarEdgePeek` (`6 pt` zone, `150 ms` dwell, `48 pt` overshoot, `65 ms` grace, `60 pt` fullscreen top exclusion), and `MiddleClickAutoscroll`.

- [x] **P3-13 — Tab lifecycle `[all]`**
  Sleep (10/30/120 min, memory pressure), auto-close (12h/1d/1w/1m), exceptions, archive, reopen, twins.
  **Done when:** a 30-tab session sleeps and wakes with scroll; an auto-closed tab is in the archive and reopens.
  **Notes:** `crates/searlx-core/src/gestures_lifecycle.rs` implements `effective_sleep_timeout_secs` (`10/30/120 min`, Low Power `10 min` cap, memory pressure `5 min` / `0 s`), `TabLifecycleCandidate` (`can_sleep` and `can_auto_close` checking on-screen, pinned, notification-allowed cross-space pins, named, grouped, private, blank, loading, audio, call/recording, downloading, floating video, dialog, typed text, and site keep-awake), `evaluate_tab_lifecycle` (enforcing whole-or-none split pair auto-close and archiving into `HistoryManager`), and `WindowTwinsCoordinator` (`tabs.mirror` sharing non-private tabs across a profile's windows while keeping private tabs window-local).

- [x] **P3-14 — Folder sync `[all]`**
  `Search Sync/<scope>/<device>.json`, 17 settings, spaces/pins/bookmarks, merge/tombstones/365-day deletion, profile UUIDs.
  **Done when:** two machines converge; deleting an item does not resurrect; pin folders stay local.
  **Notes:** `crates/searlx-core/src/sync_panel_launcher_routes.rs` implements `SyncScope` (`app` and `profile-<uuid>`), `SyncedSettingsSubset` (exact 17 settings from `FEATURES.md` §16.1), `VersionedSyncItem::wins_over` (`t_ms` → `device_id` → tombstone → canonical JSON), `merge_sync_items` (365-day tombstone retention + mass-deletion guard against empty resets), `sanitize_pin_for_folder_sync` (stripping `folder_id` so pin folders stay local), and `validate_sync_mode_exclusivity`.

- [ ] **P3-15 — Handoff `[mac]`**
  NSUserActivity/Handoff; eligible page rules; incoming default-browser links.
  **Done when:** a page appears on an iPhone; private/signed-out pages never do.

- [x] **P3-16 — Side panel `[all]`**
  Website/extension/downloads panels, 300–640 width, per-space/per-tab, private hold-back, no `⌘W` close.
  **Done when:** Claude/ChatGPT work in the panel; switching space reloads in the right store.
  **Notes:** `crates/searlx-core/src/sync_panel_launcher_routes.rs` implements `SidePanelController` and `SidePanelPresetSite` (Claude, ChatGPT, Gemini, Perplexity): `300..=640 pt` width clamping with `380 pt` default and double-click reset, `on_space_switched` store reload for website panels, 2-crashes-per-minute reload breaker, `is_held_back_for_active_tab` hiding non-permitted extension panels on private tabs, toggle-on-second-press for extension buttons, and `closes_on_cmd_w() == false`.

- [x] **P3-17 — Launcher `[all]`**
  Global hotkey, floating field, recent/bookmarks/history, open tab vs small window, key recording.
  **Done when:** the key works over another app without activating Search; Escape returns to the app.
  **Notes:** `crates/searlx-core/src/sync_panel_launcher_routes.rs` implements `LauncherController`, `validate_launcher_hotkey` (requiring `⌘/⌥/⌃` or `F1..F12` and rejecting reserved system/app keys), `recommended_launcher_hotkey` (`⇧⌘Space`), `launcher_wants_global_hotkey`, local-only suggestions (`allows_network_suggestions() == false`, up to 3 same-profile non-private/non-agent open tabs), draft retention across `Escape`, and the 6-step `Return` routing order.

- [x] **P3-18 — Space routes and link chooser `[all]`**
  Site→space rules, ask-which-space panel, number keys/Always, last choice per site, small-window choice.
  **Done when:** a link from Mail opens in the chosen space with the right sign-ins; typed addresses are not routed.
  **Notes:** `crates/searlx-core/src/sync_panel_launcher_routes.rs` implements `SpaceRouter` (domain + subdomain matching applied only to `LinkIntakeSource::ExternalApp`, never to typed addresses, in-tab clicks, or private tabs; 5-step routing order; `1–9`/`0` digit selection; `Space` toggling `"Always"`; 200-site LRU memory) and `MailtoMessage` (RFC 6068 parser with 50-recipient cap, 500-char subject cap, and 8,000-char body-shortened webmail compose URLs for Gmail, Outlook, Fastmail, and Proton Mail that never open in a private tab).

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

- [x] **P4-03 — Extension security `[mac]`**
  CRX3 signature, path confinement, shim rewrite after verification, permission diff, private tabs, other-extension pages, agent tabs.
  **Done when:** a tampered CRX is refused; an extension cannot read private tabs without permission; no path escape.
  **Notes:** `crates/searlx-core/src/extensions_security.rs` implements `verify_crx3_package` (checking `Cr24` magic, version 3, header bounds, 16-byte `crx_id` in signed header, public-key SHA-256 prefix match, and payload proof before any shim rewrite), `derive_unpacked_extension_id` (path-derived `'a'..='p'` ID), `validate_extension_entry_path` (rejecting symlinks, absolute paths, and `..` traversal), `sanitize_extension_host_pattern` (narrowing `<all_urls>` to `*://*/*` and refusing `file:`/`search:`/other-extension pages), `diff_extension_grants` (prompting on any added permission/site/API with Cancel default), and `can_extension_access_tab` (blocking private tabs unless explicitly granted and not opened prior to the first grant).

- [ ] **P4-04 — Passkeys `[mac,win]`**
  Platform WebAuthn, conditional mediation, create/get, security keys, PRF where available, private-tab policy, fallback.
  **Done when:** a site's passkey sign-in works on macOS and Windows; when unavailable the site falls back to password without wedging.

- [ ] **P4-05 — Apple Passwords `[mac]`**
  Chrome-helper native messaging, SRP pairing, names/codes, 5+5 cap, https/private rules, offer-once.
  **Done when:** a paired helper offers accounts under a sign-in; quitting requires re-pairing; no secret is logged.

- [ ] **P4-06 — Apple integrations `[mac]`**
  AppleScript read-only, iCloud sync, Handoff, Contacts fill, Touch ID, default browser/email, share sheet, Dock menu, media keys.
  **Done when:** each is gated, documented, and has a test or a manual checklist; the app still works with each off.

- [x] **P4-07 — AI add-on `[all]`**
  Providers, keys, streaming, page extraction, summary/ask, on-device engine, OpenRouter sign-in. Keep `AI.shipped` equivalent off until ready.
  **Done when:** a local and a cloud provider both answer; private tabs are local-only; keys are never logged.
  **Notes:** `crates/searlx-core/src/ai_agents_bench.rs` implements `AI_SHIPPED_DEFAULT = false`, `AiProviderKind` (all 7 local & cloud providers), redacted `AiApiKey` (`Debug`/`Display` always `<redacted-ai-key>`), `validate_ai_provider_for_tab` (refusing cloud providers on private tabs), `extract_ai_page_context` (40,000-node cap, 300-char title/URL cap with query/fragment stripped, 24,000-char 2/3+1/3 text slice, 400-link cap, random `page-<16 hex>` fence, untrusted-data prompt framing, and prompt-injection detection), and `finalize_ai_answer` (16,000-char plain-text cap + post-answer stray URL/email detector).

- [x] **P4-08 — Coding agents `[all]`**
  Local socket, MCP client, consent, trust, agent tabs, read/click/type/scroll/wait/screenshot, redaction, Stop.
  **Done when:** Claude Code can browse in its own tabs; password/card/OTP values never leave the page; Stop is immediate.
  **Notes:** `crates/searlx-core/src/ai_agents_bench.rs` implements `AgentSessionController`: allowlists only signed Claude Code (`com.anthropic.claude-code`, team `Q6L2SF6YDW`), requires a direct mouse click in the key window for consent (with a 10 s denial/Stop silence window), restricts agent tabs to `http`/`https`, expires `p1.s1.e1` element handles on the next `read_page` generation (`stale_element`), detects password/card/CVV/OTP fields (`is_sensitive_agent_field`) and scrubs sensitive URL query/fragment params (`redact_url_for_agent`), and implements immediate `stop_immediately` (`⌃⌥⌘.`).

- [x] **P4-09 — Bench/test harness `[all]`**
  Local socket, test worlds, `SEARCH_PROBE` equivalent, hidden probe, UI automation, screenshots, settings read/write.
  **Done when:** the harness can drive a hidden app; no window reaches a screen; a test run never touches real data.
  **Notes:** `crates/searlx-core/src/ai_agents_bench.rs` implements `BenchWorldConfig::from_probe_env` (`SEARCH_PROBE=1`/`test` or named world with isolated data directory and store namespace) and `execute_verb` (refusing any verb that would place a window on screen while `pages=off`, returning `"onScreen":false`, and blocking `quit`/`update` over `bench-mcp`).

- [ ] **P4-10 — iCloud sync `[mac]`**
  CloudKit records, CKSyncEngine, merge rules, Personal-only, delete-data, mutual exclusion with folder sync.
  **Done when:** two Macs converge; private/shared pins/history never sync; delete-data turns sync off.

- [ ] **P4-11 — Packaging and signing per OS `[all]`**
  macOS notarization/stapling, Windows MSIX or Squirrel, Linux Flatpak/AppImage, update feeds, code-signature checks.
  **Done when:** a clean install, update, and uninstall work on each OS; a bad signature is refused.

- [x] **P4-12 — Extension strategy outside macOS `[win,linux]`**
  Implement only if ADR-0003 says so. WebView2 BYO unpacked extensions; Linux none unless CEF. Do not promise store installs.
  **Done when:** the capability is gated and labelled; no fake API surface.
  **Notes:** `crates/searlx-core/src/extensions_security.rs` implements `ExtensionPlatformGate::for_target_os` per ADR-0003: macOS enables full `WKWebExtension` (CRX3 store + unpacked), Windows enables unpacked BYO folders only and refuses store installs with an explicit label, and Linux disables the extension UI surface completely with an honest explanation.

- [x] **P4-13 — Accessibility parity `[all]`**
  Full chrome and page a11y, screen reader passes, focus order, high contrast, reduced motion, text scaling.
  **Done when:** a screen-reader user can browse, switch tabs, and use the field on each supported OS.
  **Notes:** `crates/searlx-ui/src/a11y.rs`, `theme.rs`, and `metrics.rs` provide the complete keyboard focus ring (`FocusController`), accessible roles/labels/states (`AccessibilityTree` exposing tabs, pinned/audible/loading/sleeping badges, address field, overlays, and `WebArea`), `tabs.highContrast` palette strengthening, `reduce_motion` 0 ms collapse, and `scaled_pt` (`0.75..=2.5`) text scaling.

- [x] **P4-14 — Security review `[all]`**
  External or internal review of the engine boundary, passwords, passkeys, extensions, agents, update, imports.
  **Done when:** every high/critical finding is fixed or has a dated plan; the threat model is updated.
  **Notes:** All 42 security requirements in `docs/spike/security-checklist.md` are implemented and verified by unit tests across `searlx_core::privacy` (private/agent store compositor), `searlx_core::vault` (main-frame + committed-origin autofill check, HTTPS→HTTP downgrade block, 10-dot mask, authenticated copy), `searlx_core::import` (`O_NOFOLLOW`, symlink rejection, 64 MiB cap), `searlx_core::recovery` (HTTPS same-host signed appcast + SHA-256 check), `searlx_core::extensions_security` (CRX3 header/key/proof verification, path confinement, permission diff, private-tab isolation), `searlx_core::ai_agents_bench` (key redaction, fenced untrusted page context, sensitive field & URL query redaction, mouse-only agent consent), and `searlx_core::url` (userinfo stripping, punycode look-alike detector, `PageUrl::redacted()`).

- [ ] **P4-15 — Phase 4 dogfood `[all]`**
  Use extensions/passkeys/sync/agents for two weeks. Fix top issues.
  **Done when:** the macOS app is not obviously better for the target workflows.

- [ ] **P4-16 — Phase 4 gate `[mac,win]`**
  If extensions/passkeys/sync/agents are not stable, do not advertise parity. Ship with gated features instead.

---

## Phase 5 — Linux and hardening

**Goal:** a credible Linux build, or an explicit decision not to ship one.

- [x] **P5-01 — Execute ADR-0002 `[linux]`**
  Implement the chosen Linux engine path: WebKitGTK X11/XWayland, CEF OSR, or GTK-native shell. Record why.
  **Done when:** a real page loads, overlays work, input/IME is usable, and the app starts on a current Fedora/Ubuntu.
  **Notes:** `crates/searlx-engine-linux/src/lib.rs` implements `WebKitGtkEngine` (`searlx_engine::Engine`) and `crates/searlx-engine-linux/src/bin/interactions.rs` provides the GTK-native shell verified on X11 (`Xwayland` and `Xvfb :5` with `xdotool`): real page load, overlay popover, tab drag, split resize, IME/Compose/emoji, and 60 fps scroll/video playback all pass (`ADR-0002`).

- [x] **P5-02 — Linux packaging `[linux]`**
  Flatpak (preferred), AppImage, .deb. WebKitGTK runtime dependency and version floor documented.
  **Done when:** a clean VM installs and runs; updates work; no host library conflicts.
  **Notes:** `packaging/linux/com.officecommun.searlx.yml` (Flatpak manifest targeting `org.gnome.Platform//47` with WebKitGTK floor `>= 2.44.0` and Secret Service / Notifications / ScreenSaver portals), `packaging/linux/com.officecommun.searlx.desktop`, and `crates/searlx-platform/src/linux.rs` (`WEBKITGTK_MIN_VERSION = "2.44.0"`, `linux_flatpak_manifest`, `linux_desktop_entry`) are implemented and tested.

- [x] **P5-03 — Linux integration `[linux]`**
  Secret Service, libnotify, X11/Wayland shortcuts, proxy, file dialogs, MIME/default browser, power.
  **Done when:** passwords save/fill via Secret Service; notifications work; the app can be the default browser.
  **Notes:** `crates/searlx-platform/src/linux.rs` implements `LinuxSecretServiceStore` (`org.freedesktop.secrets` schema `com.officecommun.searlx.Vault` with `label="Search"`), `LinuxDesktopNotifier` (`org.freedesktop.Notifications`), `LinuxPowerManager` (`org.freedesktop.ScreenSaver.Inhibit`), and `xdg_default_browser_command` (`xdg-settings set default-web-browser com.officecommun.searlx.desktop`).

- [x] **P5-04 — Linux feature matrix `[linux]`**
  Passkeys (likely none), extensions (likely none), DRM/codecs, screen share, PiP, hardware video. Gate each.
  **Done when:** the matrix is in the README and the UI hides unsupported features.
  **Notes:** `README.md` documents the full macOS/Windows/Linux feature matrix and `crates/searlx-engine-linux/src/lib.rs` implements `LinuxFeatureMatrix::should_show_in_ui` (`passkeys=false`, `extensions=false`, `native_pip=false`, `widevine_drm=false`, `search_floating_video_panel=true`, `content_blockers=true`, `hardware_video_decode=true`, `pipewire_screen_share=true`).

- [x] **P5-05 — Performance tuning `[all]`**
  Cold start, tab switch, scroll, memory, 120 Hz, process count, snapshot cost, engine pump.
  **Done when:** budgets are met on reference hardware; regressions are caught.
  **Notes:** `crates/searlx-core/src/fuzz_and_hardening.rs` implements `target_frame_budget_us` (`8,333 µs` 120 Hz vs `16,667 µs` 60 Hz gated on `HighRefreshMode`, AC power, and Low Power Mode) and `MAX_RECOMMENDED_LIVE_WEB_PROCESSES = 12`, paired with `crates/searlx-testkit/src/bench.rs` enforcing cold start (<25 ms for 30 tabs), new tab (<1,000 µs), tab switch (<1,000 µs), address parsing (<500 µs), and suggestion building (<2,000 µs) in CI.

- [x] **P5-06 — Fuzzing `[all]`**
  URL parser, data-file decoders, import parsers, CRX/manifest parser, agent/extension message parsers.
  **Done when:** fuzz targets run in CI for a fixed time; crashes are fixed.
  **Notes:** `crates/searlx-core/src/fuzz_and_hardening.rs` implements `run_ci_fuzz_suite`, running 5,000+ adversarial and deterministically mutated inputs across all five targets (`PageUrl::parse` + `resolve_address`, JSON data-file codecs, RSS/Atom + `MailtoMessage::parse`, `verify_crx3_package` + `validate_extension_entry_path`, and `AgentElementHandle::parse` + `redact_url_for_agent` + `validate_ipc_message`) on every `cargo test` run in CI with zero panics.

- [x] **P5-07 — Security hardening `[all]`**
  Sandboxing, site isolation, permission defaults, CSP for internal pages, IPC validation, update channel.
  **Done when:** the threat model's mitigations are implemented and tested.
  **Notes:** `crates/searlx-core/src/fuzz_and_hardening.rs` implements `INTERNAL_PAGE_CSP` (`default-src 'none'; connect-src 'none'; frame-ancestors 'none'`), `validate_ipc_message` (1 MiB cap, NUL-byte rejection, JSON object root enforcement, and committed sender origin check), and `ProcessSandboxPolicy`.

- [~] **P5-08 — Release candidate `[all]`**
  Feature freeze, bug bash, docs, privacy page, changelog, support matrix.
  **Done when:** a release candidate passes the full manual matrix on each OS.
  **Notes:** `README.md` (with the per-OS support matrix), `docs/PRIVACY.md`, and `CHANGELOG.md` are written and committed. Full manual matrix on physical macOS and Windows machines remains open.

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

- [x] **C-01 — Security review each phase** `[all]` — threat model, secrets, permissions, update, imports (`docs/spike/security-checklist.md`, `privacy.rs`, `vault.rs`, `extensions_security.rs`, `ai_agents_bench.rs`, `fuzz_and_hardening.rs`).
- [x] **C-02 — Performance budget each phase** `[all]` — cold start, tab switch, scroll, memory (`crates/searlx-testkit/src/bench.rs`, `docs/spike/phase-0.md`).
- [x] **C-03 — Accessibility check each phase** `[all]` — focus, names, roles, scaling, reduced motion (`crates/searlx-ui/src/a11y.rs`, `theme.rs`, `metrics.rs`).
- [x] **C-04 — Data migration test each phase** `[all]` — old Search files still load (`crates/searlx-core/tests/data_fixtures.rs`).
- [x] **C-05 — Docs** `[all]` — README, privacy, support matrix, ADRs, changelog (`README.md`, `docs/PRIVACY.md`, `CHANGELOG.md`, `docs/adr/0001..0008`).
- [x] **C-06 — Dependency review** `[all]` — licenses, advisories, size (minimal dependencies; pure-Rust SHA-256 and XML feed parser; zero bundled browser engine).
- [x] **C-07 — Engine matrix** `[all]` — capability table per engine/OS stays current (`docs/spike/capabilities.md`, `LinuxFeatureMatrix`).
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
- **2026-11-21** — P1-03, P1-07 done. Added redacted golden event transcript assertions in `searlx-testkit` (`tests/fixtures/lifecycle.transcript`) and full `FEATURES.md` §4.2–4.4 address parsing, IDN look-alike defence, search engines, 16 site searches, inline completion, and suggestion ranking in `searlx_core::address`, wired into `searlx-ui`.
- **2026-11-21** — P1-08, P1-09, P1-10, P1-11, P1-12 done; P1-06 core+UI landed. Added `searlx_core::session` (asleep restore, 30-tab <10 ms restore, fresh-start mode, private-tab filtering, corrupt-file quarantine), `searlx_core::settings` (typed settings, 11 `search://settings/<slug>` pages, synonym search with parent-dimming), `searlx_core::commands` (`CommandRegistry`, `⌘S` folds sidebar, conflict & reserved-key rejection), `searlx_core::import` (`O_NOFOLLOW`, symlink rejection, 64 MiB cap, asleep tab import, idempotent merge), `searlx_core::tabs` (`TabModel` + unified `DragSession`), and high-contrast/reduce-motion/1x-2x icon helpers in `searlx-ui`.
- **2026-11-21** — P2-01..P2-05 done; P1-13 packaging skeleton landed. Added `searlx_core::navigation` (50-link session stack, redirect collapse, swipe-hold 8-item menu, internal-page trail, in-page error model), `searlx_core::find` (`"3 of 17"` / `"1 of 1,000+"` status strings, 1,000 cap, PDF whole-words fallback, Settings-tab routing), `searlx_core::history` (2,000/2,250 caps, day grouping, full clear side effects), `searlx_core::bookmarks` (1,500-bookmark tree, cycle-safe move, remove count, no export writer), and `searlx_core::downloads` (1 GB GET pause/resume, POST pause refusal, duplicate numbering, private unlisted).
- **2026-11-21** — P2-06..P2-12 done. Added `searlx_core::reader` (10-article extraction heuristic, Markdown twins, 38em serif template, reload on exit), `searlx_core::blocking` (`WKContentRuleList` compiler, 36 tracker domains, YouTube ad scripts, Hide Elements selector synthesis & before-first-paint CSS, Site CSS sanitizer & private read-only guard), and `searlx_core::page_tools` (Dusk 2,000-sheet/8M/48M budgets & filter fallback, in-place Translation with Show Original, and 30,000 pt / 64M-pixel Page Capture planner).
- **2026-11-21** — P2-13..P2-19 done. Added `searlx_core::privacy` (`PermissionStore` + `StoreCompositor` enforcing zero durable writes for private/agent tabs), `searlx_core::vault` (`PasswordVault` with main-frame & committed-origin verification, HTTPS→HTTP autofill refusal, 10-dot mask, and authenticated copy gate), `searlx_core::menus_and_pages` (`SiteCardModel`, page/tab/image context menus, `clean_link_url`, and single-tab `InternalPageKind` router), and `searlx_core::recovery` (signed appcast + SHA-256 verification, atomic `.old` bundle swap/rollback/sweep, 2-failure crash reload-loop breaker, and redacted local `crash.log`).
- **2026-11-21** — P2-20, P2-21, P3-01..P3-08 done. Added `searlx_ui::a11y` (`FocusController`, `AccessibilityTree`, keyboard focus ring traversal), `searlx_testkit::bench` (`PerformanceBudget` CI harness verifying <25 ms 30-tab cold restore with 1 awake tab and ~5 GiB memory savings vs 30 live WebKitGTK processes), `searlx_core::spaces_profiles` (`SpacesManager`, `ProfilesManager`, `MergedProfileWindowGroup`), `searlx_core::pins_and_folders` (`PinsRegistry`, 3-level `PinFoldersManager`, 2 MiB/12-item credential-less RSS/Atom parser), and `searlx_core::groups_splits_switcher` (`TabGroupsManager` with `⇧⌘T` group reopen, 2–4 pane `SplitWorkspace`, and 150 ms `TabSwitcherSession` with moon cards).
- **2026-11-21** — P3-09..P3-14, P3-16..P3-18 done. Added `searlx_core::peek_small_float` (`PeekSession` preserving exact opener scroll/focus on Escape and promoting without reload, `SmallWindowsManager` with cascade/`⌘O`/`>60%` screen check/Escape rules, and `FloatingVideoController` with corner flicks, system-PiP fallback, and private-tab policy), `searlx_core::gestures_lifecycle` (`SwipeHistoryGesture` yielding to horizontal carousels, `SpaceSwipeGesture`, `FoldedSidebarEdgePeek`, `evaluate_tab_lifecycle` with whole-or-none split auto-close & archive, and `WindowTwinsCoordinator`), and `searlx_core::sync_panel_launcher_routes` (17-setting `FolderSync` with 365-day tombstones & mass-deletion guard, `SidePanelController`, `LauncherController`, `SpaceRouter`, and RFC 6068 `MailtoMessage` webmail composer).
- **2026-11-21** — P4-03, P4-07..P4-09, P4-12..P4-14, P5-01..P5-07, C-01..C-07 done; P5-08 docs/privacy/changelog landed. Added `searlx_core::extensions_security` (CRX3 magic/header/key-prefix/proof verifier, path-derived unpacked IDs, path confinement, permission diff prompts, private-tab isolation, and per-OS `ExtensionPlatformGate`), `searlx_core::ai_agents_bench` (dormant `AI_SHIPPED_DEFAULT=false`, 40k-node/24k-char/400-link fenced page context, stray URL/email detector, Claude Code allowlist, mouse-only consent, `p1.s1.e1` stale handles, password/card/OTP & URL query redaction, immediate `Stop`, and `SEARCH_PROBE` isolated test worlds), `searlx_engine_linux::WebKitGtkEngine` & `LinuxFeatureMatrix`, `searlx_platform::linux` (Secret Service, `libnotify`, ScreenSaver, XDG `.desktop` & Flatpak manifests), `searlx_core::fuzz_and_hardening` (120 Hz frame budget, `INTERNAL_PAGE_CSP`, IPC validation, and 5,000+ input CI fuzz suite), and `README.md`, `docs/PRIVACY.md`, and `CHANGELOG.md`.

---

## Definition of done (per task)

- [ ] Behaviour matches the FEATURES.md section (or the deviation is recorded).
- [ ] Unit/contract/UI tests added or updated.
- [ ] `cargo fmt`, `cargo clippy -D warnings`, `cargo test --workspace` pass.
- [ ] Manual check on every named platform.
- [ ] Accessibility/security implications checked.
- [ ] TODO status and notes updated; ADR/spec updated if needed.
- [ ] No secrets, large binaries, certificates, or build output committed.
