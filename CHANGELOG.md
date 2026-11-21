# Changelog — searlx

## 0.1.0-rc.1 (2026-11-21)

### Phase 0 — Feasibility Spike & Architecture Decisions
- Scaffolded the 8-crate Rust 2024 workspace (`searlx-core`, `searlx-engine`, `searlx-engine-macos`, `searlx-engine-windows`, `searlx-engine-linux`, `searlx-platform`, `searlx-ui`, `searlx-testkit`, `apps/searlx`).
- Measured WebKitGTK 2.54.2 on X11 (`Xwayland` and `Xvfb`) and native Wayland (`spike`, `bench`, `interactions --external`): 60.3 fps GTK frame clock, ~62 fps `requestAnimationFrame`, 0.05–14.2 ms input-to-next-frame latencies, and 1/10/30-tab memory baseline showing why background tab sleeping is mandatory.
- Authored ADR-0001 through ADR-0008, per-engine capability matrix (`docs/spike/capabilities.md`), and 42-item security checklist (`docs/spike/security-checklist.md`).

### Phase 1 — Foundation
- Implemented domain models (`searlx_core::domain`), versioned atomic `DataStore` with corrupt-file quarantine and byte-equivalent legacy fixture round-trips (`searlx_core::data`), and hostile-file-safe Search directory importer (`searlx_core::import`).
- Implemented the full `FEATURES.md` §4.2–4.4 address parser, IDN punycode look-alike detector, 9 search engines, 16 site searches, inline completion, and suggestion ranking (`searlx_core::address`).
- Added asleep session restore (`searlx_core::session`), 11-page typed settings with synonym search (`searlx_core::settings`), rebindable command & shortcut registry (`searlx_core::commands`), unified tab drag session (`searlx_core::tabs`), theme/metrics/icons (`searlx_ui`), packaging skeleton (`searlx_platform::packaging`), and multi-OS GitHub Actions CI.

### Phase 2 — Daily Driver
- Added per-tab back/forward navigation stack & internal-page trail (`searlx_core::navigation`), isolated-world 1,000-match Find on Page (`searlx_core::find`), 2,000-cap History manager with full clear side effects (`searlx_core::history`), 1,500-bookmark tree manager (`searlx_core::bookmarks`), and 1 GB GET pause/resume Downloads manager (`searlx_core::downloads`).
- Added Reader extraction & Markdown twins (`searlx_core::reader`), `WKContentRuleList` compiler, 36 tracker domains, YouTube ad scripts, Hide Elements selector synthesis, and sanitized Site CSS (`searlx_core::blocking`).
- Added Dusk page darkening with 2,000-sheet/8M/48M budgets, in-place Translation, and 30,000 pt / 64M-pixel PDF-first Page Capture (`searlx_core::page_tools`).
- Added `PermissionStore` & zero-trace `StoreCompositor` (`searlx_core::privacy`), origin-checked `PasswordVault` blocking HTTPS→HTTP autofill downgrades (`searlx_core::vault`), `SiteCardModel` & context menus (`searlx_core::menus_and_pages`), signed appcast updater & crash loop breaker (`searlx_core::recovery`), `AccessibilityTree` & `FocusController` (`searlx_ui::a11y`), and CI `PerformanceBudget` harness (`searlx_testkit::bench`).

### Phase 3 — Identity and Power
- Implemented Spaces, Profiles, and Merged one-window profile swapping (`searlx_core::spaces_profiles`).
- Implemented Pins, 3-level Pin Folders, and 2 MiB / 12-item credential-less RSS/Atom Live Folders (`searlx_core::pins_and_folders`).
- Implemented Tab Groups with `⇧⌘T` whole-group restore, 2–4 pane Split View with 15 pt snap / 20 pt unsnap, and 150 ms `⌃Tab` Switcher with log₂-scaled small-window moons (`searlx_core::groups_splits_switcher`).
- Implemented Link Peek (`Shift`-click), Small Windows (`⌘O`, cascade, Escape rules), and Floating Video with corner flicks & private-tab policy (`searlx_core::peek_small_float`).
- Implemented swipe-back 8-item hold history (yielding to horizontal carousels), space swipe, folded sidebar edge dwell, tab sleep/auto-close lifecycle, and window twins (`searlx_core::gestures_lifecycle`).
- Implemented 17-setting Folder Sync with 365-day tombstones & mass-deletion guard, Side Panel, Global Launcher, Space Routes, Link Chooser, and RFC 6068 Webmail compose URLs (`searlx_core::sync_panel_launcher_routes`).

### Phase 4 & Phase 5 — Platform Security, Linux Integration, and Hardening
- Implemented CRX3 container verification, path-derived unpacked extension IDs, symlink/traversal rejection, permission diff prompts, private-tab isolation, and per-OS extension capability gating (`searlx_core::extensions_security`).
- Implemented dormant-by-default AI add-on page-context extractor (40k node / 24k char / 400 link caps, `page-<16 hex>` fence, prompt-injection & strays detectors, private-tab local-only guard), Coding Agents controller (Claude Code allowlist, mouse-only consent, `p1.s1.e1` stale handles, password/card/OTP & URL query redaction, immediate `Stop`), and `SEARCH_PROBE` isolated test worlds (`searlx_core::ai_agents_bench`).
- Implemented `WebKitGtkEngine` & `LinuxFeatureMatrix` (`searlx_engine_linux`), Linux Secret Service / `libnotify` / ScreenSaver / XDG `.desktop` / Flatpak integration (`searlx_platform::linux`, `packaging/linux/`), internal-page CSP & IPC validation, and a 5,000+ input deterministic CI fuzz suite (`searlx_core::fuzz_and_hardening`).
