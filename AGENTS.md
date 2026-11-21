# AGENTS.md — Search rewrite

**Status:** pre-alpha. The Rust workspace does not exist yet.
**Read first:** [FEATURES.md](FEATURES.md) is the behavioural specification of the existing macOS app. [TODO.md](TODO.md) is the live progress tracker. This file is how agents work.

---

## 1. What we are building

A fast, quiet, cross-platform web browser: tabs down the side or across the top, one field for addresses and searches, no toolbar, no start page, no account, no telemetry. Pages run in the operating system's own web engine — WebKit on macOS, WebView2 on Windows, WebKitGTK on Linux — not in a bundled Chromium.

macOS is the reference platform and the first target. Windows follows. Linux is experimental until the Phase 0 engine spike passes; do not assume parity.

The project is **searlx** (Search for Linux). The binary is `searlx`; the Rust workspace uses `searlx-*` crate names. The original macOS app, **Search**, is the reference implementation, not a codebase to transliterate.

## 2. Non-negotiables

1. **System webview per OS.** No vendored Chromium/CEF in the default build. A change to this requires an ADR and an explicit product decision.
2. **No telemetry, analytics, crash upload, or account.** The only network traffic is the pages the user asks for, optional sync/AI/update requests the user enables, and favicon fetches.
3. **No secrets in the repository, logs, error messages, or crash output.** Passwords, passkeys, AI keys, tokens, cookies, and card/OTP values are never logged. Redact URL query/fragment values that look like credentials.
4. **Do not fake parity.** A macOS-only feature is gated and labelled. Extensions, passkeys, iCloud, AppleScript, Handoff, and Apple Passwords are not promised on Linux or (for extensions/iCloud) Windows.
5. **No `unwrap()`/`expect()` on user input, network data, file data, or platform callbacks in production paths.** Use `Result`, `thiserror` at boundaries, and explicit fallbacks.
6. **Every behaviour change updates the spec or the tracker.** If implementation deviates from FEATURES.md, say why in the TODO decision log and update the spec.
7. **Security is a feature.** A browser handles passwords and arbitrary web content. Treat every engine callback, extension message, agent command, and imported file as hostile.

## 3. Architecture

Target workspace:

```text
searlx/
  Cargo.toml
  crates/
    searlx-core/          # pure data/logic; no UI or engine types
    searlx-engine/        # engine trait, events, capability descriptors
    searlx-engine-macos/  # WKWebView via objc2 / objc2-web-kit
    searlx-engine-windows/# WebView2 via windows-rs (or wry behind the trait)
    searlx-engine-linux/  # WebKitGTK via gtk-rs/wry; CEF is an ADR question
    searlx-platform/      # keychain, notifications, menus, dialogs, updater,
                          # proxy, passkeys, power, global hotkey
    searlx-ui/            # gpui chrome: window, tabs, sidebar, split, panels
    searlx-testkit/       # headless harness, fixtures, contract tests
  apps/
    searlx/               # composition root; platform wiring; main()
  docs/
    adr/                  # decisions
    spike/                # Phase 0 measurements
```

### 3.1 Crate rules

- `searlx-core` must compile and test on every OS with no platform imports. It owns URLs, history, bookmarks, sessions, imports, fuzzy matching, rules, data-file codecs, and migrations.
- `searlx-engine` defines the contract only. It must not depend on `searlx-ui` or on a platform crate.
- `searlx-engine-*` owns unsafe/platform code. It exposes only safe types. No engine type leaks into `searlx-ui`.
- `searlx-ui` owns gpui views and UI state. It talks to the engine through commands/events, never through a concrete webview.
- `searlx-platform` owns OS services behind traits so tests can substitute fakes.
- `apps/searlx` is the only crate that composes concrete implementations.

### 3.2 Engine contract (first draft)

Design for the union of what the three engines can do, with explicit capability flags. At minimum:

```rust
pub trait Engine: Send + 'static {
    fn create_tab(&mut self, opts: TabOptions) -> Result<TabId>;
    fn close_tab(&mut self, id: TabId) -> Result<()>;
    fn navigate(&mut self, id: TabId, url: Url) -> Result<()>;
    fn back(&mut self, id: TabId) -> Result<()>;
    fn forward(&mut self, id: TabId) -> Result<()>;
    fn reload(&mut self, id: TabId, from_origin: bool) -> Result<()>;
    fn stop(&mut self, id: TabId) -> Result<()>;
    fn eval(&mut self, id: TabId, world: ScriptWorld, js: &str) -> Result<()>;
    fn inject(&mut self, id: TabId, world: ScriptWorld, at: InjectAt, js: &str) -> Result<()>;
    fn set_bounds(&mut self, id: TabId, rect: Rect) -> Result<()>;
    fn set_visible(&mut self, id: TabId, visible: bool) -> Result<()>;
    fn focus(&mut self, id: TabId) -> Result<()>;
    fn snapshot(&mut self, id: TabId, rect: Rect) -> Result<Image>;
    fn find(&mut self, id: TabId, query: &str, opts: FindOptions) -> Result<FindState>;
    fn set_zoom(&mut self, id: TabId, zoom: f64) -> Result<()>;
    fn set_muted(&mut self, id: TabId, muted: bool) -> Result<()>;
    fn data_store(&self, space: SpaceId) -> Result<DataStoreHandle>;
    fn clear_data(&mut self, store: DataStoreHandle, kinds: DataKinds) -> Result<()>;
    fn capabilities(&self) -> Capabilities;
    fn pump(&mut self) -> Result<Vec<EngineEvent>>;
}
```

`EngineEvent` covers navigation start/commit/finish/fail, title/URL/favicon changes, loading, history changes, new-window requests, downloads, permission requests, JS dialogs, file pickers, auth challenges, certificate errors, media/audio state, fullscreen, process crash, and script messages.

Rules:

- One tab owns one webview and one data store. Sleeping a tab releases the view and keeps a serialisable snapshot/state.
- The engine never owns app-level state. The app tells it what to do and reconciles events.
- Capability flags are per engine, not per OS guesswork: `extensions`, `passkeys`, `content_blockers`, `native_pip`, `site_isolation`, `custom_user_agent`, `print`, `pdf`, `find_while_pdf`, `downloads_pause`, `cookie_import`, `wayland_overlay`.
- If a capability is absent, the UI hides or disables the feature; it must not silently do nothing.

### 3.3 UI/state rules

- gpui is the chrome toolkit, not a browser engine. The page is a native surface placed between gpui's base scene and its overlay scene.
- Keep UI state in `searlx-ui`; keep durable state in `searlx-core`; keep engine state in the engine.
- Commands flow UI → app → engine. Events flow engine → app → UI. No direct engine calls from views.
- Use one drag session model for tabs, pins, groups, folders, and split drops. The original has six overlapping drag systems; do not repeat that.
- Use one focus model: the page that has the keyboard, the chrome field, or a panel. Make it explicit and testable.
- Respect reduced motion, high contrast, and platform text scaling from day one.

### 3.4 Platform services

`searlx-platform` exposes traits for:

- credential store (Keychain / Credential Manager / Secret Service)
- notifications
- menus and global hotkeys
- file dialogs and print
- auto-update and code-signature verification
- proxy configuration
- passkeys / platform authenticator
- power assertions and frame-rate hints
- clipboard, share sheet, default browser/email registration
- screen/audio permissions
- OS theme and accessibility

Each trait has a fake for tests. A service that a platform cannot provide returns `Unsupported`, not a panic.

## 4. Platform matrix

| Platform | Engine | Status | Hard rules |
|---|---|---|---|
| macOS | WKWebView | Primary | Keep the original Swift app as reference where available; do not delete or rewrite it. Apple services are gated and optional. |
| Windows | WebView2 (Evergreen) | Second | WebView2 runtime assumed; no Edge Store extension installs; passkeys via Windows Hello; extension model is BYO only. |
| Linux | WebKitGTK (X11/XWayland first) | Experimental | No WebAuthn in WebKitGTK today. gpui 0.2.2 has no Linux native-view API, so ADR-0002 proposes a GTK-native shell; Wayland focus/readback are unverified. CEF OSR is the fallback only after an ADR. |

Do not add a fourth engine. Do not bundle Chromium to make the three match.

## 5. Feature tiers

Use [FEATURES.md §28](FEATURES.md) as the authority. In short:

- **Tier A (must exist for 1.0):** field, tabs, spaces/profiles, pins, split view, session/sleep/reopen, history/bookmarks/downloads/find/reader/zoom, ad+tracker blocking and hiding elements, private tabs, passwords, imports, update.
- **Tier B (can lag):** pin rows/folders/live folders, tab groups, switcher previews, Markdown twins, translate, capture, site CSS, darkening, sync, small windows, launcher, 120 Hz, glass.
- **Tier C (platform-only or drop):** extensions, passkeys (mac/win), AppleScript, Apple Passwords, iCloud/CloudKit, Handoff, Contacts/CoreLocation, Touch ID, Carbon hotkeys, IOKit power, `NSPanel` specifics, private WebKit selectors.

## 6. Implementation order

### Phase 0 — Feasibility spike (2–4 weeks, timeboxed)

Build one gpui window with two tabs, an address field, a split, an overlay menu above a page, IME, and video on macOS, Windows, and Linux (X11 and Wayland). Measure input latency, resize cost, memory, a11y, and IME. Write `docs/spike/phase-0.md`. **Gate:** if Linux cannot host a real page with acceptable input/overlay behaviour, choose CEF-OSR or a GTK-native shell before Phase 1.

### Phase 1 — Foundation (6–10 weeks)

Workspace, core models, engine trait, macOS + Windows backends, tab model, session, field, basic chrome, settings, import of existing Search JSON, CI, packaging skeleton.

### Phase 2 — Daily driver (8–12 weeks)

Navigation, history/bookmarks/downloads, find, reader, blocking/hiding, site card, zoom, permissions, private tabs, password vault, shortcuts, context menus, updates.

### Phase 3 — Identity and power (8–12 weeks)

Spaces/profiles, pins/folders, groups, splits, switcher, peek, small windows, floating video, gestures, folder sync, Handoff (mac), tab lifecycle.

### Phase 4 — Platform and parity (6–12 weeks)

Extensions (macOS first), passkeys (mac/win), Apple integrations, AI/agents/bench, iCloud, per-OS packaging/signing, accessibility, security review.

### Phase 5 — Linux and hardening

Linux engine decision, packaging (Flatpak/AppImage), QA, performance, fuzzing, security review.

## 7. Coding rules

- **Rust edition 2024, stable toolchain.** `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- **Errors:** `thiserror` in libraries, `anyhow` only at the app boundary. No `unwrap`/`expect` in production paths.
- **Async:** never block the UI thread. gpui's foreground executor for UI work; a bounded worker pool for I/O; no `block_on` on the UI thread.
- **Unsafe:** only in platform crates, always with a `// SAFETY:` comment explaining the invariant. Prefer safe wrappers (`objc2`, `windows`, `gtk-rs`).
- **Dependencies:** small, maintained, audited. Check licenses. No duplicate functionality. Ask before adding a large framework.
- **Logging:** `tracing` with a `search` target. Never log page content, credentials, tokens, or full URLs with query strings. Use `redact_url()`.
- **Data:** file formats are versioned. Read old formats; write current. A failed decode quarantines, never overwrites.
- **UI:** no magic numbers in three places; put metrics in `searlx-ui/src/metrics.rs`. Reuse one palette/theme module.
- **Commits:** one logical change; imperative subject; body says why. Reference a TODO ID.
- **Docs:** update `FEATURES.md` for behaviour changes, `TODO.md` for status, `docs/adr/` for decisions.

## 8. Testing and verification

- Unit tests in `searlx-core` for parsing, history, bookmarks, sessions, imports, rules, migrations.
- Contract tests for every engine backend: create/load/back/forward/reload/eval/inject/bounds/visibility/focus/find/permissions/downloads/events. Run on each OS in CI where possible; otherwise manual matrix.
- Golden files for engine event transcripts, data migrations, and session round-trips.
- UI tests with gpui's test context for tab switching, drag/drop, split resize, field completion, and keyboard routes.
- Accessibility smoke test on each OS: focus order, names, roles, page content reachable.
- Manual per-OS checklist for the five hard interactions: overlay above page, tab drag, split resize, IME/emoji, video/PiP.
- Security tests: URL redaction, origin checks, private-tab store isolation, permission persistence, extension/agent boundaries.

A task is not done because it compiles. It is done when its acceptance criteria and tests pass on the target platform(s).

## 9. Security rules

- **Origin checks before filling credentials.** The page must still be the committed origin the list was offered on.
- **Private tabs** use a non-persistent store and write no history, session, favicon, password offer, Handoff, iCloud, or extension state. Enforce this in the store compositor, not in each feature.
- **Agent tabs** are web-only: http/https, no downloads, no file pickers, no camera/mic/location/notifications, no password save, no search/settings pages. Redact password/card/OTP fields from reads and screenshots.
- **Extensions** must never read Search's keychain, Apple Passwords, passkeys, private tabs (unless explicitly granted), Settings, agent secrets, or another extension's pages.
- **Update** must verify a signature and a hash before replacing the bundle. Never run an unsigned engine or model.
- **Import** treats files as hostile: `O_NOFOLLOW`, regular-file checks, size caps, path confinement, no live DB reads, copy before SQLite.
- **URLs:** strip userinfo before display, copying, storing, and logging. Punycode/look-alike defence is a UI concern but the engine must not normalise it away.

## 10. Working with TODO.md

- Statuses: `[ ]` todo, `[~]` in progress, `[x]` done, `[!]` blocked, `[-]` dropped.
- Pick one item. Read its FEATURES.md section. If it is larger than a day, add a short plan under the item before coding.
- Update the item's status and notes in the same commit as the change.
- A blocked item gets a reason and a decision owner; do not silently skip it.
- A deviation from FEATURES.md goes in the TODO decision log and in the spec, not just in a commit message.
- Keep the progress table and "Now" section current.

## 11. Agent workflow

1. **Explore** the relevant crates and the FEATURES section. Do not read the whole repo for a one-line fix.
2. **Plan** in 1–5 bullets for anything non-trivial. Name the files you expect to touch.
3. **Implement** in small steps. Prefer adding a trait/test before a platform binding.
4. **Test** the narrowest useful thing first, then the workspace gate.
5. **Update** the TODO and any spec/ADR.
6. **Hand off** with: what changed, what was verified, what is still risky, and the next TODO ID.

If a task needs a platform API you cannot verify in this environment (signing, notarization, a device-only authenticator, Wayland compositor behaviour), implement the trait/fake and leave a `[!]` item with exact verification steps. Do not guess a security-sensitive behaviour.

## 12. Do not

- Do not port the Swift app file-by-file. Use its behaviour and data formats.
- Do not implement extensions/passkeys/iCloud on platforms that cannot support them.
- Do not add a bundled browser engine without an ADR.
- Do not use private WebKit selectors as a foundation; isolate any unavoidable one behind a capability with a fallback.
- Do not put engine types in UI code or UI types in core.
- Do not log secrets, page text, or full URLs with query strings.
- Do not commit certificates, provisioning profiles, models, dSYMs, or build output.
- Do not change the product's "no telemetry / no account" boundary without an explicit decision.

## 13. Open decisions

Tracked as ADRs in [TODO.md](TODO.md):

- ADR-0001: gpui + system webviews (confirm)
- ADR-0002: Linux engine — WebKitGTK X11/XWayland vs CEF-OSR vs GTK-native shell
- ADR-0003: extension strategy — macOS-only vs CEF vs drop
- ADR-0004: whether any platform may bundle Chromium
- ADR-0005: passkey strategy on Linux
- ADR-0006: sync backend — folder vs server vs iCloud-only
- ADR-0007: fate of the existing Swift macOS app (keep as reference / replace later)
- ADR-0008: distribution and update channel per OS

## 14. Commands (target; create in Phase 0)

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p searlx            # native app
cargo run -p searlx -- --probe # test world
```

Until the workspace exists, Phase 0 items in TODO.md are the source of truth for what to scaffold.
