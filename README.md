# searlx

A fast, quiet, cross-platform web browser: tabs down the side or across the top, one field for addresses and searches, no toolbar, no start page, no account, no telemetry. Pages run in the operating system's own web engine — **WKWebView** on macOS, **WebView2** on Windows, **WebKitGTK** on Linux — never in a bundled Chromium.

- **Behavioural specification:** [FEATURES.md](FEATURES.md)
- **Architecture & agent rules:** [AGENTS.md](AGENTS.md)
- **Live tracker:** [TODO.md](TODO.md)
- **Privacy policy:** [docs/PRIVACY.md](docs/PRIVACY.md)
- **Phase 0 spike & measurements:** [docs/spike/phase-0.md](docs/spike/phase-0.md)
- **Per-engine capability matrix:** [docs/spike/capabilities.md](docs/spike/capabilities.md)

---

## Workspace layout

```text
searlx/
  Cargo.toml
  crates/
    searlx-core/           # pure data/logic; URLs, history, bookmarks, sessions,
                           # imports, address parser, blocking, reader, vault,
                           # spaces/profiles, pins/folders, splits, sync, AI/agents
    searlx-engine/         # Engine trait, EngineEvent stream, Capabilities, FakeEngine
    searlx-engine-macos/   # WKWebView backend descriptor (macOS)
    searlx-engine-windows/ # WebView2 backend descriptor (Windows)
    searlx-engine-linux/   # WebKitGTK backend + GTK-native shell prototypes (Linux)
    searlx-platform/       # Keychain / Credential Manager / Secret Service,
                           # notifications, dialogs, power, packaging
    searlx-ui/             # gpui chrome: window, tabs, address field, theme,
                           # metrics, focus & accessibility tree
    searlx-testkit/        # headless contract tests, golden event transcripts,
                           # Phase 2/5 performance budget harness
  apps/
    searlx/                # composition root & binary (`searlx`, `searlx --probe`)
  packaging/
    linux/                 # Flatpak manifest & freedesktop .desktop entry
  scripts/
    package-macos.sh       # macOS .app bundle staging & ad-hoc codesign script
  docs/
    adr/                   # Architecture Decision Records (ADR-0001..0008)
    spike/                 # Phase 0 measurements, capability matrix, security checklist
```

---

## Platform & Linux Feature Matrix (`P5-04`)

Per [AGENTS.md §2.4](AGENTS.md), unsupported engine capabilities are gated and hidden in the UI rather than faked:

| Capability / Feature | macOS (`WKWebView`) | Windows (`WebView2`) | Linux (`WebKitGTK >= 2.44`) |
|---|---|---|---|
| Chrome / Shell | `gpui` | `gpui` | GTK-native shell (`ADR-0002`, X11/XWayland primary, Wayland experimental) |
| Tabs, Spaces, Profiles, Pins, Pin Folders, Live RSS Folders | Supported | Supported | Supported |
| Split View (2–4 panes) & `⌃Tab` Switcher | Supported | Supported | Supported |
| Link Peek (`Shift`-click), Small Windows (`⌘O`), Global Launcher | Supported | Supported | Supported |
| Ad & Tracker Blocking (`Shield`), Hide Elements (`⇧⌘H`), Site CSS | `WKContentRuleList` + scripts | Portable request filter + cosmetic CSS | `WebKitUserContentFilterStore` + portable filter |
| Reader Mode (`⇧⌘R`), Darkening (`⇧⌘D`), In-place Translate, Capture | Supported | Supported | Supported |
| Password Vault | Keychain + Touch ID | Credential Manager + Windows Hello | Secret Service (`org.freedesktop.secrets`) |
| Folder Sync (`Search Sync/<scope>/<device>.json`) | Supported | Supported | Supported |
| Web Extensions (`ADR-0003`) | Full (`WKWebExtension`, CRX3 + unpacked) | Unpacked BYO only (gated & labelled) | **Not supported** (hidden in UI) |
| Passkeys / WebAuthn (`ADR-0005`) | Supported (`AuthenticationServices`) | Supported (`Windows Hello`) | **Not supported** in WebKitGTK (falls back to password) |
| Floating Video / PiP | Search floating panel + native PiP | Search floating panel + native PiP | Search floating panel (`native_pip = false` on WebKitGTK 2.54.2) |
| Hardware Video Decode | VideoToolbox | D3D11 / MediaFoundation | GStreamer (VA-API / V4L2 / NVDEC) |
| Screen Sharing | ScreenCaptureKit | WebView2 picker | PipeWire (`xdg-desktop-portal`) |
| DRM / Widevine | FairPlay | PlayReady / Widevine | **Not bundled** (disabled by default) |
| iCloud Sync, Handoff, Apple Passwords, AppleScript | macOS only (gated) | Not applicable | Not applicable |

---

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p searlx            # launch browser shell
cargo run -p searlx -- --probe # run headless probe check
```
